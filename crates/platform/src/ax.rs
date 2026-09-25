//! The macOS Accessibility API, wrapped once. Everything unsafe about talking
//! to it lives in this file; the capture and focus modules use these safe
//! functions and nothing else.

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::geometry::{CGPoint, CGSize};
use logic::schema::{Element, Rect, Role};
use std::ffi::c_void;
use std::ptr;

pub type AXUIElementRef = *const c_void;
pub type AXError = i32;
pub const K_AX_ERROR_SUCCESS: AXError = 0;
const K_AX_VALUE_CGPOINT: u32 = 1;
const K_AX_VALUE_CGSIZE: u32 = 2;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> AXError;
    fn AXValueGetValue(value: CFTypeRef, value_type: u32, out: *mut c_void) -> bool;
}

extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFGetTypeID(cf: CFTypeRef) -> u64;
    fn CFStringGetTypeID() -> u64;
    fn CFNumberGetTypeID() -> u64;
    fn CFBooleanGetTypeID() -> u64;
}

/// A Core Foundation or Accessibility object we own and must release.
pub struct Owned(CFTypeRef);

impl Owned {
    pub fn element(&self) -> AXUIElementRef {
        self.0
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

/// True when the user has granted the Accessibility permission.
pub fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn system_wide() -> Owned {
    Owned(unsafe { AXUIElementCreateSystemWide() })
}

pub fn application(pid: i32) -> Owned {
    Owned(unsafe { AXUIElementCreateApplication(pid) })
}

pub fn pid(element: AXUIElementRef) -> Option<i32> {
    let mut pid = 0i32;
    let err = unsafe { AXUIElementGetPid(element, &mut pid) };
    (err == K_AX_ERROR_SUCCESS && pid > 0).then_some(pid)
}

pub fn copy_attr(element: AXUIElementRef, name: &str) -> Option<Owned> {
    let attr = CFString::new(name);
    let mut value: CFTypeRef = ptr::null();
    let err =
        unsafe { AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut value) };
    (err == K_AX_ERROR_SUCCESS && !value.is_null()).then(|| Owned(value))
}

/// An attribute whose value is another element, such as the focused window.
pub fn element_attr(element: AXUIElementRef, name: &str) -> Option<Owned> {
    copy_attr(element, name)
}

fn type_id(value: CFTypeRef) -> u64 {
    unsafe { CFGetTypeID(value) }
}

pub fn string_attr(element: AXUIElementRef, name: &str) -> String {
    match copy_attr(element, name) {
        Some(v) if type_id(v.0) == unsafe { CFStringGetTypeID() } => {
            let s: CFString = unsafe { TCFType::wrap_under_get_rule(v.0 as CFStringRef) };
            s.to_string()
        }
        _ => String::new(),
    }
}

pub fn bool_attr(element: AXUIElementRef, name: &str) -> Option<bool> {
    let v = copy_attr(element, name)?;
    if type_id(v.0) != unsafe { CFBooleanGetTypeID() } {
        return None;
    }
    let b: CFBoolean = unsafe { TCFType::wrap_under_get_rule(v.0 as *const _) };
    Some(b == CFBoolean::true_value())
}

/// The element's value as text: strings as they are, numbers without a
/// trailing ".0", booleans as checked or unchecked. Anything else is skipped.
pub fn value_attr(element: AXUIElementRef) -> Option<String> {
    let v = copy_attr(element, "AXValue")?;
    let id = type_id(v.0);
    unsafe {
        if id == CFStringGetTypeID() {
            let s: CFString = TCFType::wrap_under_get_rule(v.0 as CFStringRef);
            let s = s.to_string();
            (!s.is_empty()).then_some(s)
        } else if id == CFNumberGetTypeID() {
            let n: CFNumber = TCFType::wrap_under_get_rule(v.0 as *const _);
            n.to_f64().map(|f| {
                if f.fract() == 0.0 {
                    format!("{}", f as i64)
                } else {
                    format!("{}", f)
                }
            })
        } else if id == CFBooleanGetTypeID() {
            let b: CFBoolean = TCFType::wrap_under_get_rule(v.0 as *const _);
            Some(if b == CFBoolean::true_value() { "checked" } else { "unchecked" }.to_string())
        } else {
            None
        }
    }
}

fn point_attr(element: AXUIElementRef) -> Option<CGPoint> {
    let v = copy_attr(element, "AXPosition")?;
    let mut point = CGPoint::new(0.0, 0.0);
    let ok = unsafe { AXValueGetValue(v.0, K_AX_VALUE_CGPOINT, &mut point as *mut _ as *mut c_void) };
    ok.then_some(point)
}

fn size_attr(element: AXUIElementRef) -> Option<CGSize> {
    let v = copy_attr(element, "AXSize")?;
    let mut size = CGSize::new(0.0, 0.0);
    let ok = unsafe { AXValueGetValue(v.0, K_AX_VALUE_CGSIZE, &mut size as *mut _ as *mut c_void) };
    ok.then_some(size)
}

/// Screen coordinates in points, top-left origin, as the Accessibility API
/// reports them. To be verified once on real hardware; if it is ever wrong,
/// this is the one function to fix.
pub fn bounds(element: AXUIElementRef) -> Option<Rect> {
    let point = point_attr(element)?;
    let size = size_attr(element)?;
    let rect = Rect { x: point.x, y: point.y, width: size.width, height: size.height };
    rect.is_visible().then_some(rect)
}

/// The element's children, each retained, released when dropped.
pub fn children(element: AXUIElementRef) -> Vec<Owned> {
    let Some(v) = copy_attr(element, "AXChildren") else {
        return Vec::new();
    };
    let array: CFArray<CFType> = unsafe { TCFType::wrap_under_get_rule(v.0 as *const _) };
    let mut out = Vec::with_capacity(array.len() as usize);
    for i in 0..array.len() {
        if let Some(item) = array.get(i) {
            let raw = item.as_CFTypeRef();
            unsafe { CFRetain(raw) };
            out.push(Owned(raw));
        }
    }
    out
}

/// Set a boolean attribute to true, ignoring failure. Used for the flags
/// that make Chromium and Electron expose their web content.
pub fn set_true(element: AXUIElementRef, name: &str) {
    let attr = CFString::new(name);
    unsafe {
        AXUIElementSetAttributeValue(
            element,
            attr.as_concrete_TypeRef(),
            CFBoolean::true_value().as_CFTypeRef(),
        );
    }
}

/// Read one element into the shared schema, without its children.
pub fn read_element(element: AXUIElementRef, id: u32) -> Element {
    let role_name = string_attr(element, "AXRole");
    let subrole = string_attr(element, "AXSubrole");
    let role = Role::from_ax(&role_name, &subrole);
    let secure = role_name == "AXSecureTextField";

    let title = string_attr(element, "AXTitle");
    let name = if !title.is_empty() { title } else { string_attr(element, "AXDescription") };

    let mut e = Element::new(id, role, name);
    if secure {
        e = e.secure();
    } else if let Some(value) = value_attr(element) {
        let value = match (role, value.as_str()) {
            (Role::Checkbox | Role::Radio, "1") => "checked".to_string(),
            (Role::Checkbox | Role::Radio, "0") => "unchecked".to_string(),
            _ => value,
        };
        e = e.with_value(value);
    }
    // Static text and headings carry their words in the value on macOS and
    // in the name on Windows. Normalize to the name so both read the same.
    if matches!(role, Role::Text | Role::Heading) && e.name.is_empty() {
        if let Some(words) = e.value.take() {
            e.name = words;
        }
    }
    e.enabled = bool_attr(element, "AXEnabled").unwrap_or(true);
    e.focused = bool_attr(element, "AXFocused").unwrap_or(false);
    e.selected = bool_attr(element, "AXSelected").unwrap_or(false);
    e.bounds = bounds(element);
    e
}
