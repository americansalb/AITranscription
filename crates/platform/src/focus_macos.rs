//! macOS focus tracking via an AXObserver on a dedicated CFRunLoop thread.
//!
//! The observer receives kAXFocusedUIElementChanged for the whole system,
//! reads the element's title, role and value, builds one announcement through
//! the shared `announce` module, and hands it to the caller's sink.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use core_foundation::base::TCFType;
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoopRef, CFRunLoopSourceRef};
use core_foundation::string::{CFString, CFStringRef};

use crate::announce::{build_announcement, now_ms, Deduper, FocusEvent, FocusSink};
use crate::types::ax_role_to_normalized;

static TRACKING_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Monotonic generation counter. Each start() increments it. Cleanup only
/// clears shared state when the generation still matches, so a stale thread
/// cannot overwrite a newer thread's run loop ref or active flag.
static GENERATION: AtomicU64 = AtomicU64::new(0);

// CFRunLoopRef is a raw pointer; CFRunLoopStop is documented thread-safe.
struct SendableRunLoop(CFRunLoopRef);
unsafe impl Send for SendableRunLoop {}

static RUN_LOOP_REF: Mutex<Option<SendableRunLoop>> = Mutex::new(None);

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
        let cf_str: CFString = unsafe { TCFType::wrap_under_get_rule(value as CFStringRef) };
        let result = cf_str.to_string();
        unsafe { CFRelease(value) };
        result
    } else {
        String::new()
    }
}

/// Owned by the observer for the lifetime of one tracking run and passed to
/// the callback through `refcon`.
struct Context {
    sink: FocusSink,
    dedupe: Mutex<Deduper>,
}

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
    let role = ax_role_to_normalized(&get_ax_string(element, "AXRole"));
    let role_name = role.as_str().to_string();
    let value = get_ax_string(element, "AXValue");

    let text = build_announcement(&name, &role_name, &value);
    if text.is_empty() {
        return;
    }

    let ctx = &*(refcon as *const Context);
    {
        let mut dedupe = ctx.dedupe.lock().unwrap();
        if !dedupe.accept(&text) {
            return;
        }
    }

    (ctx.sink)(FocusEvent { text, name, role: role_name, value, timestamp_ms: now_ms() });
}

pub fn start(sink: FocusSink) {
    // Stop any existing observer before starting a new one.
    stop();

    TRACKING_ACTIVE.store(true, Ordering::SeqCst);
    let my_gen = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    std::thread::spawn(move || unsafe {
        let clear_if_current = || {
            if GENERATION.load(Ordering::SeqCst) == my_gen {
                TRACKING_ACTIVE.store(false, Ordering::SeqCst);
            }
        };

        if !AXIsProcessTrusted() {
            eprintln!("[platform/focus_macos] Accessibility not trusted; cannot track focus");
            clear_if_current();
            return;
        }

        let system_wide = AXUIElementCreateSystemWide();
        if system_wide.is_null() {
            eprintln!("[platform/focus_macos] Failed to create system-wide element");
            clear_if_current();
            return;
        }

        let mut observer: AXObserverRef = ptr::null();
        // pid 0 means a system-wide observer.
        let err = AXObserverCreate(0, focus_callback, &mut observer);
        if err != K_AX_ERROR_SUCCESS || observer.is_null() {
            eprintln!("[platform/focus_macos] AXObserverCreate failed: {}", err);
            CFRelease(system_wide);
            clear_if_current();
            return;
        }

        let ctx = Box::new(Context { sink, dedupe: Mutex::new(Deduper::default()) });
        let ctx_ptr = Box::into_raw(ctx) as *mut c_void;

        let notification = CFString::new("AXFocusedUIElementChanged");
        let err = AXObserverAddNotification(
            observer,
            system_wide,
            notification.as_concrete_TypeRef(),
            ctx_ptr,
        );
        if err != K_AX_ERROR_SUCCESS {
            eprintln!("[platform/focus_macos] AXObserverAddNotification failed: {}", err);
            drop(Box::from_raw(ctx_ptr as *mut Context));
            CFRelease(observer);
            CFRelease(system_wide);
            clear_if_current();
            return;
        }

        let source = AXObserverGetRunLoopSource(observer);
        let rl = CFRunLoopGetCurrent();
        {
            let mut stored = RUN_LOOP_REF.lock().unwrap();
            *stored = Some(SendableRunLoop(rl));
        }
        CFRunLoopAddSource(rl, source, kCFRunLoopDefaultMode);

        // Blocks until stop() calls CFRunLoopStop.
        CFRunLoopRun();

        drop(Box::from_raw(ctx_ptr as *mut Context));
        CFRelease(observer);
        CFRelease(system_wide);

        if GENERATION.load(Ordering::SeqCst) == my_gen {
            let mut stored = RUN_LOOP_REF.lock().unwrap();
            *stored = None;
            drop(stored);
            TRACKING_ACTIVE.store(false, Ordering::SeqCst);
        }
    });
}

pub fn stop() {
    TRACKING_ACTIVE.store(false, Ordering::SeqCst);
    let stored = RUN_LOOP_REF.lock().unwrap();
    if let Some(ref wrapper) = *stored {
        unsafe { CFRunLoopStop(wrapper.0) };
    }
}

pub fn is_active() -> bool {
    TRACKING_ACTIVE.load(Ordering::SeqCst)
}
