//! Turning a focused element into words, the same way on every platform.
//!
//! Each platform's focus tracker reads the raw element and hands the parts
//! here. Building the sentence and suppressing repeats are pure and shared,
//! so they are tested once and behave identically on macOS and Windows.

use serde::{Deserialize, Serialize};

/// One announcement about the element that just received keyboard focus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusEvent {
    /// The sentence to speak, already assembled.
    pub text: String,
    pub name: String,
    pub role: String,
    pub value: String,
    pub timestamp_ms: u64,
}

/// Where focus events go. Called on a background thread owned by the
/// platform crate, so it must be safe to send and share.
pub type FocusSink = Box<dyn Fn(FocusEvent) + Send + Sync + 'static>;

/// The longest value read aloud before it is cut, in characters.
pub const MAX_SPOKEN_VALUE_CHARS: usize = 50;

/// The plain-language name of a normalized role, or empty when the role adds
/// nothing worth saying.
pub fn friendly_role(role: &str) -> &'static str {
    match role {
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
    }
}

/// Assemble what to say about a focused element. Returns an empty string when
/// there is nothing worth saying, which callers treat as "stay silent".
pub fn build_announcement(name: &str, role: &str, value: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !name.is_empty() {
        parts.push(name.to_string());
    }
    let friendly = friendly_role(role);
    if !friendly.is_empty() {
        parts.push(friendly.to_string());
    }
    if !value.is_empty() {
        // Cut by characters, never by bytes, so multibyte text cannot panic.
        let display = if value.chars().count() > MAX_SPOKEN_VALUE_CHARS {
            let head: String = value.chars().take(MAX_SPOKEN_VALUE_CHARS).collect();
            format!("{}...", head)
        } else {
            value.to_string()
        };
        parts.push(display);
    } else if role == "TextInput" {
        parts.push("empty".to_string());
    }
    parts.join(", ")
}

/// Suppresses an announcement identical to the previous one, so a focus
/// tracker that polls does not repeat itself every tick.
#[derive(Debug, Default)]
pub struct Deduper {
    last: String,
}

impl Deduper {
    /// Returns true when `text` is new and should be spoken.
    pub fn accept(&mut self, text: &str) -> bool {
        if self.last == text {
            return false;
        }
        self.last = text.to_string();
        true
    }
}

/// Milliseconds since the Unix epoch, or zero if the clock is unavailable.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_to_say_is_empty() {
        assert_eq!(build_announcement("", "Unknown", ""), "");
    }

    #[test]
    fn name_and_role_are_joined() {
        assert_eq!(build_announcement("Save", "Button", ""), "Save, button");
    }

    #[test]
    fn empty_text_input_says_empty() {
        assert_eq!(build_announcement("Search", "TextInput", ""), "Search, edit field, empty");
    }

    #[test]
    fn text_area_with_value_reads_the_value() {
        assert_eq!(
            build_announcement("Notes", "TextArea", "hello"),
            "Notes, text area, hello"
        );
    }

    #[test]
    fn unknown_role_is_omitted_but_value_is_kept() {
        assert_eq!(build_announcement("Cell", "Unknown", "42"), "Cell, 42");
    }

    #[test]
    fn long_values_are_cut_at_fifty_characters() {
        let value = "a".repeat(60);
        let text = build_announcement("", "TextArea", &value);
        assert_eq!(text, format!("text area, {}...", "a".repeat(50)));
    }

    #[test]
    fn cutting_is_by_character_not_by_byte() {
        // Each of these is two bytes in UTF-8; slicing at byte 50 would split one.
        let value = "é".repeat(60);
        let text = build_announcement("", "TextArea", &value);
        assert_eq!(text, format!("text area, {}...", "é".repeat(50)));
    }

    #[test]
    fn exactly_fifty_characters_is_not_cut() {
        let value = "b".repeat(50);
        let text = build_announcement("", "TextArea", &value);
        assert_eq!(text, format!("text area, {}", value));
    }

    #[test]
    fn deduper_speaks_new_text_and_suppresses_repeats() {
        let mut d = Deduper::default();
        assert!(d.accept("Save, button"));
        assert!(!d.accept("Save, button"));
        assert!(d.accept("Cancel, button"));
        assert!(d.accept("Save, button"));
    }

    #[test]
    fn every_friendly_role_is_lowercase_words() {
        for role in [
            "TextInput", "TextArea", "Button", "Checkbox", "RadioButton", "ComboBox", "Tab",
            "TabItem", "MenuItem", "Link", "ListItem", "TreeItem", "Slider", "Spinner",
        ] {
            let f = friendly_role(role);
            assert!(!f.is_empty(), "{role} has no friendly name");
            assert_eq!(f, f.to_lowercase());
        }
        assert_eq!(friendly_role("Window"), "");
    }

    #[test]
    fn focus_event_round_trips_through_json() {
        let event = FocusEvent {
            text: "Save, button".into(),
            name: "Save".into(),
            role: "Button".into(),
            value: String::new(),
            timestamp_ms: 12345,
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: FocusEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }
}
