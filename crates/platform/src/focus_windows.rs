//! Windows focus tracking via UI Automation polling.
//!
//! A dedicated thread asks UIA for the focused element every 100 ms, builds
//! one announcement through the shared `announce` module, suppresses repeats,
//! and hands each new announcement to the caller's sink.

use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::System::Com::*;
use windows::Win32::UI::Accessibility::*;

use crate::announce::{build_announcement, now_ms, Deduper, FocusEvent, FocusSink};
use crate::types::uia_control_type_to_role;

static TRACKING_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn start(sink: FocusSink) {
    if TRACKING_ACTIVE.swap(true, Ordering::SeqCst) {
        return;
    }

    std::thread::spawn(move || {
        unsafe {
            if CoInitializeEx(Some(std::ptr::null()), COINIT_APARTMENTTHREADED).is_err() {
                TRACKING_ACTIVE.store(false, Ordering::SeqCst);
                return;
            }
        }

        let uia: IUIAutomation =
            match unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("[platform/focus_windows] UIA create failed: {}", e);
                    TRACKING_ACTIVE.store(false, Ordering::SeqCst);
                    unsafe { CoUninitialize() };
                    return;
                }
            };

        let mut dedupe = Deduper::default();

        while TRACKING_ACTIVE.load(Ordering::SeqCst) {
            if let Ok(focused) = unsafe { uia.GetFocusedElement() } {
                let name =
                    unsafe { focused.CurrentName().map(|s| s.to_string()).unwrap_or_default() };
                let control_type_id =
                    unsafe { focused.CurrentControlType().unwrap_or(UIA_CONTROLTYPE_ID(0)) };
                let role = uia_control_type_to_role(control_type_id.0).as_str().to_string();
                let value = unsafe {
                    focused
                        .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                        .ok()
                        .and_then(|p| p.CurrentValue().ok())
                        .map(|v| v.to_string())
                        .unwrap_or_default()
                };

                let text = build_announcement(&name, &role, &value);
                if !text.is_empty() && dedupe.accept(&text) {
                    (sink)(FocusEvent { text, name, role, value, timestamp_ms: now_ms() });
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        unsafe { CoUninitialize() };
        TRACKING_ACTIVE.store(false, Ordering::SeqCst);
    });
}

pub fn stop() {
    TRACKING_ACTIVE.store(false, Ordering::SeqCst);
}

pub fn is_active() -> bool {
    TRACKING_ACTIVE.load(Ordering::SeqCst)
}
