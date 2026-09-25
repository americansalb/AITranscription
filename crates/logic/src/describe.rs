//! Turning a snapshot into compact text for the model.
//!
//! One line per element that matters, indented by depth. Nameless
//! containers are collapsed so their children move up a level. Interactive
//! elements carry the point to click. Secure elements never show a value.

use crate::schema::{Element, Snapshot};
use std::fmt::Write;

/// The longest value shown to the model per element, in characters.
pub const MAX_VALUE_CHARS: usize = 120;

pub fn render(snapshot: &Snapshot, max_lines: usize) -> String {
    let mut out = String::new();
    let app = if snapshot.app.name.is_empty() { "unknown" } else { &snapshot.app.name };
    let title = if snapshot.window_title.is_empty() { "untitled" } else { &snapshot.window_title };
    let _ = writeln!(out, "Application: {}. Window: {}.", app, title);
    let _ = writeln!(
        out,
        "{} elements{}.",
        snapshot.element_count,
        if snapshot.truncated { ", cut short at the capture limit" } else { "" }
    );

    let mut lines: Vec<String> = Vec::new();
    for element in &snapshot.elements {
        render_element(element, 0, &mut lines);
    }
    let shown = lines.len().min(max_lines);
    out.push_str(&lines[..shown].join("\n"));
    if lines.len() > shown {
        let _ = write!(out, "\n... {} more lines not shown", lines.len() - shown);
    }
    out
}

fn render_element(element: &Element, depth: usize, lines: &mut Vec<String>) {
    let collapse = element.name.is_empty()
        && element.value.is_none()
        && element.role.is_plain_container()
        && !element.focused;
    if collapse {
        for child in &element.children {
            render_element(child, depth, lines);
        }
        return;
    }

    let mut line = format!("{}[{}] {}", "  ".repeat(depth), element.id, element.role.label());
    if !element.name.is_empty() {
        let _ = write!(line, " \"{}\"", element.name);
    }
    if element.secure {
        line.push_str(" (secure)");
    } else if let Some(value) = &element.value {
        let _ = write!(line, " = \"{}\"", cut(value));
    }
    if element.focused {
        line.push_str(" focused");
    }
    if element.selected {
        line.push_str(" selected");
    }
    if !element.enabled {
        line.push_str(" dimmed");
    }
    if element.role.is_interactive() {
        if let Some(bounds) = element.bounds.filter(|b| b.is_visible()) {
            let (x, y) = bounds.center();
            let _ = write!(line, " at {},{}", x.round() as i64, y.round() as i64);
        }
    }
    lines.push(line);
    for child in &element.children {
        render_element(child, depth + 1, lines);
    }
}

fn cut(value: &str) -> String {
    if value.chars().count() > MAX_VALUE_CHARS {
        let head: String = value.chars().take(MAX_VALUE_CHARS).collect();
        format!("{}...", head)
    } else {
        value.to_string()
    }
}

/// True when the rendered text never shows a secure element's value. Used by
/// tests and by callers that want to assert the rule before sending text
/// anywhere.
pub fn shows_no_secure_values(snapshot: &Snapshot) -> bool {
    let mut clean = true;
    snapshot.walk(&mut |e, _| {
        if e.secure && e.value.is_some() {
            clean = false;
        }
    });
    clean
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{App, Rect, Role};

    fn snapshot(elements: Vec<Element>) -> Snapshot {
        Snapshot {
            platform: "test".into(),
            app: App { name: "Mail".into(), pid: 42 },
            window_title: "Inbox".into(),
            scale: 2.0,
            captured_at_ms: 0,
            elapsed_ms: 3,
            truncated: false,
            element_count: Snapshot::count_elements(&elements),
            elements,
        }
    }

    #[test]
    fn header_names_the_app_and_window() {
        let text = render(&snapshot(vec![]), 100);
        assert!(text.starts_with("Application: Mail. Window: Inbox.\n0 elements.\n"));
    }

    #[test]
    fn nameless_containers_collapse_and_children_move_up() {
        let tree = vec![Element::new(1, Role::Group, "").with_children(vec![
            Element::new(2, Role::Group, "").with_children(vec![Element::new(3, Role::Button, "Save")]),
        ])];
        let text = render(&snapshot(tree), 100);
        assert!(text.ends_with("[3] button \"Save\""), "{text}");
        assert!(!text.contains("group"));
    }

    #[test]
    fn named_containers_stay_and_indent_their_children() {
        let tree = vec![Element::new(1, Role::Group, "Toolbar").with_children(vec![
            Element::new(2, Role::Button, "Reply"),
        ])];
        let text = render(&snapshot(tree), 100);
        assert!(text.contains("[1] group \"Toolbar\"\n  [2] button \"Reply\""), "{text}");
    }

    #[test]
    fn interactive_elements_carry_a_click_point_and_text_does_not() {
        let bounds = Some(Rect { x: 10.0, y: 20.0, width: 100.0, height: 40.0 });
        let tree = vec![
            Element { bounds, ..Element::new(1, Role::Button, "Send") },
            Element { bounds, ..Element::new(2, Role::Text, "Draft saved") },
        ];
        let text = render(&snapshot(tree), 100);
        assert!(text.contains("[1] button \"Send\" at 60,40"), "{text}");
        assert!(text.contains("[2] text \"Draft saved\"\n") || text.ends_with("[2] text \"Draft saved\""));
        assert!(!text.contains("\"Draft saved\" at"));
    }

    #[test]
    fn a_secure_field_shows_no_value_ever() {
        let tree = vec![Element::new(1, Role::TextField, "Password").secure().with_value("hunter2")];
        let snap = snapshot(tree);
        let text = render(&snap, 100);
        assert!(text.contains("[1] edit field \"Password\" (secure)"), "{text}");
        assert!(!text.contains("hunter2"));
        assert!(shows_no_secure_values(&snap));
    }

    #[test]
    fn flags_are_appended_in_a_fixed_order() {
        let e = Element {
            focused: true,
            selected: true,
            enabled: false,
            ..Element::new(1, Role::Tab, "Sent")
        };
        let text = render(&snapshot(vec![e]), 100);
        assert!(text.contains("[1] tab \"Sent\" focused selected dimmed"), "{text}");
    }

    #[test]
    fn long_values_are_cut_and_the_line_count_is_capped() {
        let long = "x".repeat(200);
        let tree: Vec<Element> = (1..=5)
            .map(|i| Element::new(i, Role::TextField, format!("Field {i}")).with_value(long.clone()))
            .collect();
        let text = render(&snapshot(tree), 2);
        assert!(text.contains(&format!("= \"{}...\"", "x".repeat(120))));
        assert!(text.ends_with("... 3 more lines not shown"), "{text}");
        assert!(!text.contains("Field 3"));
    }

    #[test]
    fn a_focused_nameless_container_is_kept_so_focus_is_never_lost() {
        let e = Element { focused: true, ..Element::new(1, Role::Group, "") };
        let text = render(&snapshot(vec![e]), 100);
        assert!(text.contains("[1] group focused"), "{text}");
    }
}
