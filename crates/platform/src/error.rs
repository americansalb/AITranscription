//! Everything that can go wrong at the platform layer, each with a sentence
//! a person can hear. Silence is the one failure this app must never have.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformError {
    /// This operating system cannot do this at all.
    NotSupported,
    /// The permission to read the screen has not been granted.
    NotPermitted,
    /// No application has a window in front.
    NoForegroundWindow,
    /// The application in front has no window, for example a menu bar app.
    NoWindow { app: String },
    /// The window in front is our own.
    OwnWindow,
    /// The operating system reported a failure.
    System(String),
}

impl PlatformError {
    /// The sentence to speak. Plain words, what happened, and when possible
    /// what to do about it.
    pub fn spoken(&self) -> String {
        match self {
            PlatformError::NotSupported => {
                "This computer's system cannot share what is on the screen.".to_string()
            }
            PlatformError::NotPermitted => {
                "I do not have permission to read the screen yet.".to_string()
            }
            PlatformError::NoForegroundWindow => "There is no window in front.".to_string(),
            PlatformError::NoWindow { app } => {
                let app = if app.is_empty() { "The application in front" } else { app.as_str() };
                format!("{app} has no window open.")
            }
            PlatformError::OwnWindow => {
                "The window in front is mine, so there is nothing to read.".to_string()
            }
            PlatformError::System(detail) => {
                format!("Something went wrong reading the screen: {}.", detail.trim_end_matches('.'))
            }
        }
    }

    /// One of each variant, so a test can prove every error speaks.
    pub fn samples() -> Vec<PlatformError> {
        vec![
            PlatformError::NotSupported,
            PlatformError::NotPermitted,
            PlatformError::NoForegroundWindow,
            PlatformError::NoWindow { app: "Spotify".to_string() },
            PlatformError::NoWindow { app: String::new() },
            PlatformError::OwnWindow,
            PlatformError::System("the window could not be read".to_string()),
        ]
    }
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.spoken())
    }
}

impl std::error::Error for PlatformError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_speaks_a_full_sentence() {
        for error in PlatformError::samples() {
            let text = error.spoken();
            assert!(!text.is_empty(), "{error:?} has no sentence");
            assert!(text.ends_with('.'), "{error:?} does not end a sentence: {text}");
            assert!(text.chars().next().unwrap().is_uppercase(), "{error:?}: {text}");
            assert!(!text.contains("Error"), "{error:?} speaks jargon: {text}");
        }
    }

    #[test]
    fn a_nameless_app_still_gets_a_sentence() {
        assert_eq!(
            PlatformError::NoWindow { app: String::new() }.spoken(),
            "The application in front has no window open."
        );
        assert_eq!(PlatformError::NoWindow { app: "Spotify".into() }.spoken(), "Spotify has no window open.");
    }

    #[test]
    fn system_details_do_not_double_the_period() {
        assert_eq!(
            PlatformError::System("it broke.".into()).spoken(),
            "Something went wrong reading the screen: it broke."
        );
    }
}
