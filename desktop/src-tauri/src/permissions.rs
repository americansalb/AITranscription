/// Permission checking and requesting for macOS accessibility features
///
/// This module provides platform-specific permission handling for features
/// that require system-level access (global hotkeys, keyboard simulation).

#[cfg(target_os = "macos")]
use std::process::Command;

/// Check if the application has accessibility permissions on macOS.
///
/// On macOS, accessibility permissions are required for:
/// - Global hotkey registration (system-wide keyboard events)
/// - Keyboard simulation (auto-paste functionality)
///
/// This function uses AppleScript to test if we can access UI elements,
/// which will fail if accessibility permissions aren't granted.
///
/// # Returns
/// - `true` if permissions are granted
/// - `false` if permissions are denied or check failed
///
/// # Platform Support
/// - macOS: Checks actual permission status via AppleScript
/// - Windows/Linux: Always returns `true` (no special permissions needed)
#[cfg(target_os = "macos")]
pub fn check_accessibility_permission() -> bool {
    // Use AppleScript to check if we can access UI elements
    // This will fail if accessibility permission is not granted
    let output = Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to get UI elements"#,
        ])
        .output();

    match output {
        Ok(result) => {
            let success = result.status.success();
            if !success {
                eprintln!("Accessibility permission check failed. macOS may be blocking access.");
                eprintln!("User needs to enable Scribe in System Settings > Privacy & Security > Accessibility");
            }
            success
        },
        Err(e) => {
            eprintln!("Failed to check accessibility permission: {}", e);
            false
        }
    }
}

/// Request accessibility permission by opening System Settings.
///
/// Opens the macOS System Settings app directly to the Accessibility panel
/// in Privacy & Security, where the user can grant permission to Scribe.
///
/// # Returns
/// - `Ok(())` if System Settings opened successfully
/// - `Err(String)` if failed to open System Settings
///
/// # Platform Support
/// - macOS: Opens System Settings to Accessibility panel
/// - Windows/Linux: No-op (returns Ok immediately)
///
/// # Note
/// This doesn't automatically grant permission - it just guides the user
/// to the right place in System Settings. The user must manually toggle
/// the permission and restart the app.
#[cfg(target_os = "macos")]
pub fn request_accessibility_permission() -> Result<(), String> {
    println!("Opening System Settings to Accessibility panel...");

    Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn()
        .map_err(|e| format!("Failed to open System Settings: {}", e))?;

    Ok(())
}

// Non-macOS platforms: Permissions not required, always return success
#[cfg(not(target_os = "macos"))]
pub fn check_accessibility_permission() -> bool {
    // Windows and Linux don't require special permissions for global hotkeys
    true
}

#[cfg(not(target_os = "macos"))]
pub fn request_accessibility_permission() -> Result<(), String> {
    // No-op on non-macOS platforms
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_permission_doesnt_panic() {
        // Should not panic regardless of permission status
        let _result = check_accessibility_permission();
    }

    #[test]
    fn test_request_permission_doesnt_panic() {
        // Should not panic when requesting permission
        #[cfg(not(target_os = "macos"))]
        {
            // On non-macOS, should always succeed
            assert!(request_accessibility_permission().is_ok());
        }
    }
}
