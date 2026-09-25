//! Turning one element into the words a person hears. Pure, shared by every
//! platform, tested once.

use crate::schema::Element;

/// The longest value read aloud before it is cut, in characters.
pub const MAX_SPOKEN_VALUE_CHARS: usize = 50;

/// The sentence for an element that just received focus, or empty when
/// there is nothing worth saying, which callers treat as "stay silent".
///
/// Order: name, secure, role word, value or emptiness, selected, dimmed.
/// A secure element's value is never spoken, and there is no value to speak
/// because the schema never carries one.
pub fn sentence(element: &Element) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !element.name.is_empty() {
        parts.push(element.name.clone());
    }
    if element.secure {
        parts.push("secure".to_string());
    }
    let role_word = element.role.spoken();
    if !role_word.is_empty() {
        parts.push(role_word.to_string());
    }
    if !element.secure {
        match &element.value {
            Some(value) if !value.is_empty() => parts.push(cut(value)),
            _ if element.role.is_text_entry() => parts.push("empty".to_string()),
            _ => {}
        }
    }
    if element.selected && element.role.is_interactive() {
        parts.push("selected".to_string());
    }
    if !element.enabled {
        parts.push("dimmed".to_string());
    }
    parts.join(", ")
}

/// Cut by characters, never by bytes, so multibyte text cannot panic.
fn cut(value: &str) -> String {
    if value.chars().count() > MAX_SPOKEN_VALUE_CHARS {
        let head: String = value.chars().take(MAX_SPOKEN_VALUE_CHARS).collect();
        format!("{}...", head)
    } else {
        value.to_string()
    }
}

/// Suppresses a sentence identical to the previous one, so a focus tracker
/// that polls does not repeat itself every tick.
#[derive(Debug, Default)]
pub struct Deduper {
    last: String,
}

impl Deduper {
    /// True when `text` is new and should be spoken.
    pub fn accept(&mut self, text: &str) -> bool {
        if self.last == text {
            return false;
        }
        self.last = text.to_string();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Role;

    #[test]
    fn nothing_to_say_is_empty() {
        assert_eq!(sentence(&Element::new(1, Role::Group, "")), "");
        assert_eq!(sentence(&Element::new(1, Role::Unknown, "")), "");
    }

    #[test]
    fn name_and_role_are_joined() {
        assert_eq!(sentence(&Element::new(1, Role::Button, "Save")), "Save, button");
    }

    #[test]
    fn empty_text_entry_says_empty() {
        assert_eq!(sentence(&Element::new(1, Role::TextField, "Search")), "Search, edit field, empty");
        assert_eq!(sentence(&Element::new(1, Role::TextArea, "")), "text area, empty");
    }

    #[test]
    fn a_value_is_read() {
        let e = Element::new(1, Role::TextArea, "Notes").with_value("hello");
        assert_eq!(sentence(&e), "Notes, text area, hello");
    }

    #[test]
    fn a_secure_field_never_says_its_value_and_never_says_empty() {
        let e = Element::new(1, Role::TextField, "Password").secure().with_value("hunter2");
        assert_eq!(sentence(&e), "Password, secure, edit field");
    }

    #[test]
    fn long_values_are_cut_by_character() {
        let e = Element::new(1, Role::TextArea, "").with_value("é".repeat(60));
        assert_eq!(sentence(&e), format!("text area, {}...", "é".repeat(50)));
        let e = Element::new(1, Role::TextArea, "").with_value("b".repeat(50));
        assert_eq!(sentence(&e), format!("text area, {}", "b".repeat(50)));
    }

    #[test]
    fn selected_and_dimmed_are_spoken_last() {
        let e = Element { selected: true, ..Element::new(1, Role::Tab, "Inbox") };
        assert_eq!(sentence(&e), "Inbox, tab, selected");
        let e = Element { enabled: false, ..Element::new(1, Role::Button, "Send") };
        assert_eq!(sentence(&e), "Send, button, dimmed");
    }

    #[test]
    fn a_static_text_reads_its_own_words() {
        let e = Element::new(1, Role::Text, "Three unread messages");
        assert_eq!(sentence(&e), "Three unread messages");
    }

    #[test]
    fn deduper_speaks_new_text_and_suppresses_repeats() {
        let mut d = Deduper::default();
        assert!(d.accept("Save, button"));
        assert!(!d.accept("Save, button"));
        assert!(d.accept("Cancel, button"));
        assert!(d.accept("Save, button"));
    }
}
