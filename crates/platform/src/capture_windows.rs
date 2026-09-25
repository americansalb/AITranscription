//! Windows capture: the window in front, read through UI Automation into the
//! shared schema.

use crate::{now_ms, PlatformError};
use logic::schema::{App, Element, Limits, Rect, Role, Snapshot};
use std::time::{Duration, Instant};
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Initializes COM for this thread and uninitializes it on drop, unless COM
/// was already initialized in another mode, in which case it is left alone.
pub struct ComGuard {
    owned: bool,
}

impl ComGuard {
    pub fn new() -> ComGuard {
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        ComGuard { owned: hr.is_ok() }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.owned {
            unsafe { CoUninitialize() };
        }
    }
}

pub fn automation() -> Result<IUIAutomation, PlatformError> {
    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
        .map_err(|e| PlatformError::System(format!("UI Automation is unavailable: {e}")))
}

pub fn snapshot(limits: &Limits) -> Result<Snapshot, PlatformError> {
    let _com = ComGuard::new();
    let started = Instant::now();
    let uia = automation()?;

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == HWND::default() {
        return Err(PlatformError::NoForegroundWindow);
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid != 0 && pid == unsafe { GetCurrentProcessId() } {
        return Err(PlatformError::OwnWindow);
    }

    let root = unsafe { uia.ElementFromHandle(hwnd) }
        .map_err(|e| PlatformError::System(format!("the window could not be read: {e}")))?;
    let window_title = unsafe { root.CurrentName() }.map(|s| s.to_string()).unwrap_or_default();
    let tree = unsafe { uia.ControlViewWalker() }
        .map_err(|e| PlatformError::System(format!("the window could not be walked: {e}")))?;

    let mut walker = Walker::new(limits, started);
    let elements = walker.children_of(&tree, &root, 0);

    Ok(Snapshot {
        platform: "windows".to_string(),
        app: App { name: process_name(pid), pid },
        window_title,
        scale: 1.0,
        captured_at_ms: now_ms(),
        elapsed_ms: started.elapsed().as_millis() as u64,
        truncated: walker.truncated,
        element_count: Snapshot::count_elements(&elements),
        elements,
    })
}

pub fn focused_element() -> Result<Element, PlatformError> {
    let _com = ComGuard::new();
    let uia = automation()?;
    let focused = unsafe { uia.GetFocusedElement() }.map_err(|_| PlatformError::NoForegroundWindow)?;
    if is_ours(&focused) {
        return Err(PlatformError::OwnWindow);
    }
    Ok(read_element(&focused, 1))
}

/// True when the element belongs to this process.
pub fn is_ours(element: &IUIAutomationElement) -> bool {
    unsafe { element.CurrentProcessId() }
        .map(|p| p as u32 == unsafe { GetCurrentProcessId() })
        .unwrap_or(false)
}

/// Read one element into the shared schema, without its children.
pub fn read_element(el: &IUIAutomationElement, id: u32) -> Element {
    unsafe {
        let name = el.CurrentName().map(|s| s.to_string()).unwrap_or_default();
        let control = el.CurrentControlType().map(|c| c.0).unwrap_or(0);
        let role = Role::from_uia(control);
        let secure = el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false);

        let mut e = Element::new(id, role, name);
        if secure {
            e = e.secure();
        } else if let Some(value) = value_of(el) {
            e = e.with_value(value);
        }
        e.enabled = el.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(true);
        e.focused = el.CurrentHasKeyboardFocus().map(|b| b.as_bool()).unwrap_or(false);
        e.selected = el
            .GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)
            .ok()
            .and_then(|p| p.CurrentIsSelected().ok())
            .map(|b| b.as_bool())
            .unwrap_or(false);
        e.bounds = el.CurrentBoundingRectangle().ok().map(rect_from).filter(Rect::is_visible);
        e
    }
}

#[allow(non_upper_case_globals)]
fn value_of(el: &IUIAutomationElement) -> Option<String> {
    unsafe {
        if let Ok(pattern) = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
            if let Ok(value) = pattern.CurrentValue() {
                let text = value.to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
        if let Ok(pattern) = el.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) {
            if let Ok(state) = pattern.CurrentToggleState() {
                return Some(
                    match state {
                        ToggleState_On => "checked",
                        ToggleState_Off => "unchecked",
                        _ => "mixed",
                    }
                    .to_string(),
                );
            }
        }
        None
    }
}

fn rect_from(r: RECT) -> Rect {
    Rect {
        x: r.left as f64,
        y: r.top as f64,
        width: (r.right - r.left) as f64,
        height: (r.bottom - r.top) as f64,
    }
}

/// The executable name of a process without its extension, or empty.
fn process_name(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let result =
            QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut size);
        let _ = CloseHandle(handle);
        if result.is_err() {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        let file = path.rsplit('\\').next().unwrap_or("");
        file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file).to_string()
    }
}

struct Walker<'a> {
    limits: &'a Limits,
    started: Instant,
    budget: Duration,
    count: u32,
    next_id: u32,
    truncated: bool,
}

impl<'a> Walker<'a> {
    fn new(limits: &'a Limits, started: Instant) -> Walker<'a> {
        Walker {
            limits,
            started,
            budget: Duration::from_millis(limits.budget_ms),
            count: 0,
            next_id: 0,
            truncated: false,
        }
    }

    fn children_of(
        &mut self,
        tree: &IUIAutomationTreeWalker,
        parent: &IUIAutomationElement,
        depth: u32,
    ) -> Vec<Element> {
        if depth >= self.limits.max_depth {
            self.truncated = true;
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut current = match unsafe { tree.GetFirstChildElement(parent) } {
            Ok(el) => el,
            Err(_) => return out,
        };
        loop {
            if self.count >= self.limits.max_elements || self.started.elapsed() > self.budget {
                self.truncated = true;
                break;
            }
            self.count += 1;
            self.next_id += 1;
            let mut element = read_element(&current, self.next_id);
            element.children = self.children_of(tree, &current, depth + 1);
            out.push(element);
            match unsafe { tree.GetNextSiblingElement(&current) } {
                Ok(next) => current = next,
                Err(_) => break,
            }
        }
        out
    }
}
