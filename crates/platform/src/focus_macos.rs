//! macOS focus tracking.
//!
//! One background thread watches which application is in front and keeps an
//! accessibility observer attached to it for focus changes. When the front
//! application changes, the observer moves with it and the newly focused
//! element is announced, so the user always knows where they landed.
//!
//! The thread runs its own run loop in short slices, so stopping is just a
//! flag: no cross-thread run loop handles, no generation counters.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoopRef, CFRunLoopSourceRef};
use core_foundation::string::{CFString, CFStringRef};

use crate::ax::{self, AXUIElementRef, Owned, K_AX_ERROR_SUCCESS};
use crate::{now_ms, FocusEvent, FocusSink, PlatformError};
use logic::announce::{self, Deduper};

static TRACKING_ACTIVE: AtomicBool = AtomicBool::new(false);

type AXObserverRef = *const c_void;
type AXError = i32;

type AXObserverCallback = unsafe extern "C" fn(
    observer: AXObserverRef,
    element: AXUIElementRef,
    notification: CFStringRef,
    refcon: *mut c_void,
);

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
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
    fn AXObserverRemoveNotification(
        observer: AXObserverRef,
        element: AXUIElementRef,
        notification: CFStringRef,
    ) -> AXError;
    fn AXObserverGetRunLoopSource(observer: AXObserverRef) -> CFRunLoopSourceRef;
}

extern "C" {
    fn CFRelease(cf: *const c_void);
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRemoveSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source_handled: u8) -> i32;
}

const FOCUS_CHANGED: &str = "AXFocusedUIElementChanged";

/// Owned by the tracking thread for the lifetime of one run and handed to
/// the observer callback through `refcon`.
struct Context {
    sink: FocusSink,
    dedupe: Mutex<Deduper>,
}

fn emit(ctx: &Context, element: AXUIElementRef) {
    if ax::pid(element).map(|p| p as u32) == Some(std::process::id()) {
        return;
    }
    let element = ax::read_element(element, 0);
    let sentence = announce::sentence(&element);
    if sentence.is_empty() {
        return;
    }
    {
        let mut dedupe = ctx.dedupe.lock().unwrap();
        if !dedupe.accept(&sentence) {
            return;
        }
    }
    (ctx.sink)(FocusEvent { element, sentence, timestamp_ms: now_ms() });
}

unsafe extern "C" fn focus_callback(
    _observer: AXObserverRef,
    element: AXUIElementRef,
    _notification: CFStringRef,
    refcon: *mut c_void,
) {
    if !TRACKING_ACTIVE.load(Ordering::SeqCst) || refcon.is_null() {
        return;
    }
    emit(&*(refcon as *const Context), element);
}

/// The observer attached to one application.
struct Attached {
    pid: i32,
    observer: AXObserverRef,
    app: Owned,
    source: CFRunLoopSourceRef,
}

impl Attached {
    fn new(pid: i32, refcon: *mut c_void) -> Option<Attached> {
        let mut observer: AXObserverRef = ptr::null();
        let err = unsafe { AXObserverCreate(pid, focus_callback, &mut observer) };
        if err != K_AX_ERROR_SUCCESS || observer.is_null() {
            return None;
        }
        let app = ax::application(pid);
        let notification = CFString::new(FOCUS_CHANGED);
        let err = unsafe {
            AXObserverAddNotification(
                observer,
                app.element(),
                notification.as_concrete_TypeRef(),
                refcon,
            )
        };
        if err != K_AX_ERROR_SUCCESS {
            unsafe { CFRelease(observer) };
            return None;
        }
        let source = unsafe { AXObserverGetRunLoopSource(observer) };
        unsafe { CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopDefaultMode) };
        Some(Attached { pid, observer, app, source })
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        let notification = CFString::new(FOCUS_CHANGED);
        unsafe {
            CFRunLoopRemoveSource(CFRunLoopGetCurrent(), self.source, kCFRunLoopDefaultMode);
            AXObserverRemoveNotification(
                self.observer,
                self.app.element(),
                notification.as_concrete_TypeRef(),
            );
            CFRelease(self.observer);
        }
    }
}

pub fn start(sink: FocusSink) -> Result<(), PlatformError> {
    if !ax::trusted() {
        return Err(PlatformError::NotPermitted);
    }
    if TRACKING_ACTIVE.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    std::thread::spawn(move || {
        let ctx = Box::new(Context { sink, dedupe: Mutex::new(Deduper::default()) });
        let refcon = Box::into_raw(ctx) as *mut c_void;
        let system = ax::system_wide();
        let mut attached: Option<Attached> = None;

        while TRACKING_ACTIVE.load(Ordering::SeqCst) {
            let front = ax::element_attr(system.element(), "AXFocusedApplication")
                .and_then(|app| ax::pid(app.element()));
            if front != attached.as_ref().map(|a| a.pid) {
                attached = None;
                if let Some(pid) = front.filter(|p| *p as u32 != std::process::id()) {
                    attached = Attached::new(pid, refcon);
                    if let Some(focused) = ax::element_attr(system.element(), "AXFocusedUIElement") {
                        // Safety: refcon points at the Box we own until the loop ends.
                        emit(unsafe { &*(refcon as *const Context) }, focused.element());
                    }
                }
            }
            if attached.is_some() {
                // Handle observer callbacks for up to a quarter second.
                unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, 0) };
            } else {
                std::thread::sleep(Duration::from_millis(250));
            }
        }

        attached = None;
        let _ = attached;
        // Safety: the observer is gone, so nothing can call back into ctx.
        drop(unsafe { Box::from_raw(refcon as *mut Context) });
        TRACKING_ACTIVE.store(false, Ordering::SeqCst);
    });
    Ok(())
}

pub fn stop() {
    TRACKING_ACTIVE.store(false, Ordering::SeqCst);
}

pub fn is_active() -> bool {
    TRACKING_ACTIVE.load(Ordering::SeqCst)
}
