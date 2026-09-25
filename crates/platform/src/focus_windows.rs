//! Windows focus tracking: a background thread asks UI Automation for the
//! focused element every 100 milliseconds and announces changes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::capture_windows::{automation, is_ours, read_element, ComGuard};
use crate::{now_ms, FocusEvent, FocusSink, PlatformError};
use logic::announce::{self, Deduper};

static TRACKING_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn start(sink: FocusSink) -> Result<(), PlatformError> {
    if TRACKING_ACTIVE.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    std::thread::spawn(move || {
        let _com = ComGuard::new();
        let uia = match automation() {
            Ok(uia) => uia,
            Err(_) => {
                TRACKING_ACTIVE.store(false, Ordering::SeqCst);
                return;
            }
        };
        let mut dedupe = Deduper::default();

        while TRACKING_ACTIVE.load(Ordering::SeqCst) {
            if let Ok(focused) = unsafe { uia.GetFocusedElement() } {
                if !is_ours(&focused) {
                    let element = read_element(&focused, 0);
                    let sentence = announce::sentence(&element);
                    if !sentence.is_empty() && dedupe.accept(&sentence) {
                        (sink)(FocusEvent { element, sentence, timestamp_ms: now_ms() });
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }

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
