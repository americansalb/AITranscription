//! macOS AXObserver-based focus tracking.
//!
//! Spawns a dedicated thread with a CFRunLoop that receives
//! kAXFocusedUIElementChanged notifications and emits Tauri
//! "speak-immediate" events for TTS announcements.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static TRACKING_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Monotonic generation counter. Each start() increments this. Cleanup code
/// only clears shared state if the generation still matches, preventing a
/// stale thread from overwriting a newer thread's run loop ref or active flag.
static GENERATION: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
mod ax_focus {
    use super::*;
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::Mutex;

    use tauri::Manager;
    use core_foundation::base::TCFType;
    use core_foundation::runloop::{
        CFRunLoopRef, CFRunLoopSourceRef, kCFRunLoopDefaultMode,
    };
    use core_foundation::string::{CFString, CFStringRef};

    // Wrapper to allow CFRunLoopRef (raw pointer) in a static Mutex across threads.
    // Safety: CFRunLoopRef is safe to send across threads — CFRunLoopStop() is thread-safe.
    struct SendableRunLoop(CFRunLoopRef);
    unsafe impl Send for SendableRunLoop {}

    // Store the run loop ref so stop() can signal it from another thread
    static RUN_LOOP_REF: Mutex<Option<SendableRunLoop>> = Mutex::new(None);

    // AX API FFI
    type AXUIElementRef = *const c_void;
    type AXObserverRef = *const c_void;
    type AXError = i32;
    const K_AX_ERROR_SUCCESS: AXError = 0;

    type AXObserverCallback = unsafe extern "C" fn(
        observer: AXObserverRef,
        element: AXUIElementRef,
        notification: CFStringRef,
        refcon: *mut c_void,
    );

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXObserverCreate(
            application: i32,
            callback: AXObserverCallback,
            observer_out: *mut AXObserverRef,
        ) -> AXError;
        fn AXObserverAddNotification(
            observer: AXObserverRef,
            element: AXUIElementRef,
            notification: CFStringRef,
            refcon: *mut c_void,
        ) -> AXError;
        fn AXObserverGetRunLoopSource(observer: AXObserverRef) -> CFRunLoopSourceRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut *const c_void,
        ) -> AXError;
        fn AXIsProcessTrusted() -> bool;
    }

    extern "C" {
        fn CFRelease(cf: *const c_void);
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
        fn CFRunLoopGetCurrent() -> CFRunLoopRef;
        fn CFRunLoopRun();
        fn CFRunLoopStop(rl: CFRunLoopRef);
    }

    fn get_ax_string(element: AXUIElementRef, attr: &str) -> String {
        let cf_attr = CFString::new(attr);
        let mut value: *const c_void = ptr::null();
        let err = unsafe {
            AXUIElementCopyAttributeValue(element, cf_attr.as_concrete_TypeRef(), &mut value)
        };
        if err == K_AX_ERROR_SUCCESS && !value.is_null() {
            let cf_str: CFString =
                unsafe { TCFType::wrap_under_get_rule(value as CFStringRef) };
            let result = cf_str.to_string();
            unsafe { CFRelease(value) };
            result
        } else {
            String::new()
        }
    }

    fn build_announcement(name: &str, role: &str, value: &str) -> String {
        let mut parts = Vec::new();
        if !name.is_empty() {
            parts.push(name.to_string());
        }
        // Use NormalizedRole names (same as focus_windows.rs)
        let friendly = match role {
            "TextInput" => "edit field",
            "TextArea" => "text area",
            "Button" => "button",
            "Checkbox" => "checkbox",
            "RadioButton" => "radio button",
            "ComboBox" => "combo box",
            "Tab" => "tab",
            "TabItem" => "tab",
            "MenuItem" => "menu item",
            "Link" => "link",
            "ListItem" => "list item",
            "TreeItem" => "tree item",
            "Slider" => "slider",
            "Spinner" => "spin button",
            _ => "",
        };
        if !friendly.is_empty() {
            parts.push(friendly.to_string());
        }
        if !value.is_empty() {
            let display = if value.chars().count() > 50 {
                let truncated: String = value.chars().take(50).collect();
                format!("{}...", truncated)
            } else {
                value.to_string()
            };
            parts.push(display);
        } else if role == "TextInput" {
            parts.push("empty".to_string());
        }
        parts.join(", ")
    }

    // Callback invoked by AXObserver when focus changes
    unsafe extern "C" fn focus_callback(
        _observer: AXObserverRef,
        element: AXUIElementRef,
        _notification: CFStringRef,
        refcon: *mut c_void,
    ) {
        if !TRACKING_ACTIVE.load(Ordering::SeqCst) {
            return;
        }

        let name = {
            let title = get_ax_string(element, "AXTitle");
            if !title.is_empty() {
                title
            } else {
                get_ax_string(element, "AXDescription")
            }
        };

        let role_str = get_ax_string(element, "AXRole");
        let role = crate::a11y::types::ax_role_to_normalized(&role_str);
        let role_name = role.as_str().to_string();

        let value = get_ax_string(element, "AXValue");

        let announcement = build_announcement(&name, &role_name, &value);

        if announcement.is_empty() {
            return;
        }

        // refcon is a Box<AppHandleContext>
        let ctx = &*(refcon as *const AppHandleContext);

        // Deduplicate
        {
            let mut last = ctx.last_announcement.lock().unwrap();
            if *last == announcement {
                return;
            }
            *last = announcement.clone();
        }

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let payload = serde_json::json!({
            "text": announcement,
            "session_id": "focus-tracker",
            "timestamp": ts,
            "priority": "immediate",
        });

        if let Some(window) = ctx.app.get_webview_window("main") {
            let _ = tauri::Emitter::emit(&window, "speak-immediate", &payload);
        }
    }

    struct AppHandleContext {
        app: tauri::AppHandle,
        last_announcement: Mutex<String>,
    }

    pub fn start(app: tauri::AppHandle) {
        // Stop any existing observer before starting a new one
        stop();

        TRACKING_ACTIVE.store(true, Ordering::SeqCst);
        let my_gen = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

        std::thread::spawn(move || {
            unsafe {
                if !AXIsProcessTrusted() {
                    eprintln!("[a11y/focus_macos] Accessibility not trusted — cannot track focus");
                    // Only clear if we're still the current generation
                    if GENERATION.load(Ordering::SeqCst) == my_gen {
                        TRACKING_ACTIVE.store(false, Ordering::SeqCst);
                    }
                    return;
                }

                let system_wide = AXUIElementCreateSystemWide();
                if system_wide.is_null() {
                    eprintln!("[a11y/focus_macos] Failed to create system-wide element");
                    if GENERATION.load(Ordering::SeqCst) == my_gen {
                        TRACKING_ACTIVE.store(false, Ordering::SeqCst);
                    }
                    return;
                }

                let mut observer: AXObserverRef = ptr::null();
                // pid 0 = system-wide observer
                let err = AXObserverCreate(0, focus_callback, &mut observer);
                if err != K_AX_ERROR_SUCCESS || observer.is_null() {
                    eprintln!("[a11y/focus_macos] AXObserverCreate failed: {}", err);
                    CFRelease(system_wide);
                    if GENERATION.load(Ordering::SeqCst) == my_gen {
                        TRACKING_ACTIVE.store(false, Ordering::SeqCst);
                    }
                    return;
                }

                // Context for callback
                let ctx = Box::new(AppHandleContext {
                    app,
                    last_announcement: Mutex::new(String::new()),
                });
                let ctx_ptr = Box::into_raw(ctx) as *mut c_void;

                let notification =
                    CFString::new("AXFocusedUIElementChanged");
                let err = AXObserverAddNotification(
                    observer,
                    system_wide,
                    notification.as_concrete_TypeRef(),
                    ctx_ptr,
                );
                if err != K_AX_ERROR_SUCCESS {
                    eprintln!(
                        "[a11y/focus_macos] AXObserverAddNotification failed: {}",
                        err
                    );
                    // Clean up
                    let _ = Box::from_raw(ctx_ptr as *mut AppHandleContext);
                    CFRelease(observer);
                    CFRelease(system_wide);
                    if GENERATION.load(Ordering::SeqCst) == my_gen {
                        TRACKING_ACTIVE.store(false, Ordering::SeqCst);
                    }
                    return;
                }

                let source = AXObserverGetRunLoopSource(observer);
                let rl = CFRunLoopGetCurrent();

                // Store run loop ref so stop() can signal it
                {
                    let mut stored = RUN_LOOP_REF.lock().unwrap();
                    *stored = Some(SendableRunLoop(rl));
                }

                CFRunLoopAddSource(
                    rl,
                    source,
                    kCFRunLoopDefaultMode,
                );

                // Block until CFRunLoopStop is called
                CFRunLoopRun();

                // Cleanup after run loop exits — only clear shared state if
                // no newer start() has been called (generation still matches)
                let _ = Box::from_raw(ctx_ptr as *mut AppHandleContext);
                CFRelease(observer);
                CFRelease(system_wide);

                if GENERATION.load(Ordering::SeqCst) == my_gen {
                    let mut stored = RUN_LOOP_REF.lock().unwrap();
                    *stored = None;
                    drop(stored);
                    TRACKING_ACTIVE.store(false, Ordering::SeqCst);
                }
            }
        });
    }

    pub fn stop() {
        TRACKING_ACTIVE.store(false, Ordering::SeqCst);
        // Signal the CFRunLoop to exit
        let stored = RUN_LOOP_REF.lock().unwrap();
        if let Some(ref wrapper) = *stored {
            unsafe { CFRunLoopStop(wrapper.0) };
        }
    }
}

#[cfg(target_os = "macos")]
pub fn start(app: tauri::AppHandle) {
    ax_focus::start(app);
}

#[cfg(target_os = "macos")]
pub fn stop() {
    ax_focus::stop();
}

pub fn is_active() -> bool {
    TRACKING_ACTIVE.load(Ordering::SeqCst)
}

#[cfg(not(target_os = "macos"))]
pub fn start(_app: tauri::AppHandle) {
    eprintln!("[a11y/focus_macos] Not on macOS");
}

#[cfg(not(target_os = "macos"))]
pub fn stop() {}
