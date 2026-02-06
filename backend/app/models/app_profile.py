"""
Application profiles — app-specific shortcuts and instructions for the screen reader.

Each profile maps a process name to keyboard shortcuts, navigation patterns,
and custom instructions that get injected into the Claude system prompt.
"""

from dataclasses import dataclass, field


@dataclass
class AppProfile:
    """Profile for an application with shortcuts and custom behaviors."""
    process_name: str  # e.g., "chrome.exe", "WINWORD.EXE"
    display_name: str  # e.g., "Google Chrome", "Microsoft Word"
    shortcuts: dict[str, str] = field(default_factory=dict)  # action -> shortcut
    instructions: str = ""  # Custom instructions for Claude
    navigation_hints: str = ""  # Navigation tips


# Built-in profiles
BUILTIN_PROFILES: dict[str, AppProfile] = {
    "chrome.exe": AppProfile(
        process_name="chrome.exe",
        display_name="Google Chrome",
        shortcuts={
            "New tab": "Ctrl+T",
            "Close tab": "Ctrl+W",
            "Reopen closed tab": "Ctrl+Shift+T",
            "Next tab": "Ctrl+Tab",
            "Previous tab": "Ctrl+Shift+Tab",
            "Address bar": "Ctrl+L",
            "Find on page": "Ctrl+F",
            "Refresh": "F5",
            "Back": "Alt+Left",
            "Forward": "Alt+Right",
            "Bookmarks": "Ctrl+Shift+B",
            "Downloads": "Ctrl+J",
            "History": "Ctrl+H",
            "Developer tools": "F12",
            "Zoom in": "Ctrl++",
            "Zoom out": "Ctrl+-",
            "Reset zoom": "Ctrl+0",
        },
        instructions=(
            "This is Google Chrome. Use keyboard shortcuts instead of clicking toolbar buttons. "
            "The address bar can be focused with Ctrl+L. Tab through page content with Tab/Shift+Tab. "
            "For links and buttons on web pages, prefer using Tab to navigate and Enter to activate."
        ),
        navigation_hints="Tab navigates between page elements. Ctrl+L focuses the URL bar.",
    ),
    "msedge.exe": AppProfile(
        process_name="msedge.exe",
        display_name="Microsoft Edge",
        shortcuts={
            "New tab": "Ctrl+T",
            "Close tab": "Ctrl+W",
            "Address bar": "Ctrl+L",
            "Find": "Ctrl+F",
            "Next tab": "Ctrl+Tab",
            "Previous tab": "Ctrl+Shift+Tab",
            "Back": "Alt+Left",
            "Forward": "Alt+Right",
        },
        instructions="This is Microsoft Edge. Same keyboard shortcuts as Chrome apply.",
        navigation_hints="Tab navigates between page elements. Ctrl+L focuses the URL bar.",
    ),
    "firefox.exe": AppProfile(
        process_name="firefox.exe",
        display_name="Mozilla Firefox",
        shortcuts={
            "New tab": "Ctrl+T",
            "Close tab": "Ctrl+W",
            "Address bar": "Ctrl+L",
            "Find": "Ctrl+F",
            "Next tab": "Ctrl+Tab",
            "Previous tab": "Ctrl+Shift+Tab",
        },
        instructions="This is Firefox. Use standard browser keyboard shortcuts.",
    ),
    "winword.exe": AppProfile(
        process_name="WINWORD.EXE",
        display_name="Microsoft Word",
        shortcuts={
            "Save": "Ctrl+S",
            "Bold": "Ctrl+B",
            "Italic": "Ctrl+I",
            "Underline": "Ctrl+U",
            "Undo": "Ctrl+Z",
            "Redo": "Ctrl+Y",
            "Find": "Ctrl+F",
            "Replace": "Ctrl+H",
            "Select all": "Ctrl+A",
            "Copy": "Ctrl+C",
            "Paste": "Ctrl+V",
            "Cut": "Ctrl+X",
            "Print": "Ctrl+P",
            "Open ribbon": "Alt",
            "File menu": "Alt+F",
        },
        instructions=(
            "This is Microsoft Word. Use Ctrl+key shortcuts for formatting. "
            "Access the ribbon with Alt key. Navigate between ribbon tabs with arrow keys."
        ),
        navigation_hints="Alt activates the ribbon. Arrow keys move between tabs. Enter selects.",
    ),
    "excel.exe": AppProfile(
        process_name="EXCEL.EXE",
        display_name="Microsoft Excel",
        shortcuts={
            "Save": "Ctrl+S",
            "Find": "Ctrl+F",
            "Go to cell": "Ctrl+G",
            "Name box": "Ctrl+F5",
            "Insert row": "Ctrl+Shift++",
            "Delete row": "Ctrl+-",
            "Format cells": "Ctrl+1",
            "AutoSum": "Alt+=",
        },
        instructions=(
            "This is Excel. Navigate cells with arrow keys. "
            "Enter edits the selected cell. Tab moves to the next cell. "
            "Use Ctrl+G or the Name Box to jump to a specific cell."
        ),
    ),
    "outlook.exe": AppProfile(
        process_name="OUTLOOK.EXE",
        display_name="Microsoft Outlook",
        shortcuts={
            "New email": "Ctrl+N",
            "Reply": "Ctrl+R",
            "Reply all": "Ctrl+Shift+R",
            "Forward": "Ctrl+F",
            "Send": "Ctrl+Enter",
            "Search": "Ctrl+E",
            "Go to inbox": "Ctrl+Shift+I",
            "Go to calendar": "Ctrl+2",
            "Go to contacts": "Ctrl+3",
        },
        instructions=(
            "This is Outlook. Use Ctrl+N for new email, Ctrl+R to reply. "
            "Tab through email fields (To, Cc, Subject, Body). "
            "Use Ctrl+Enter to send."
        ),
    ),
    "code.exe": AppProfile(
        process_name="Code.exe",
        display_name="Visual Studio Code",
        shortcuts={
            "Command palette": "Ctrl+Shift+P",
            "Quick open": "Ctrl+P",
            "Terminal": "Ctrl+`",
            "Sidebar": "Ctrl+B",
            "Find": "Ctrl+F",
            "Replace": "Ctrl+H",
            "Go to line": "Ctrl+G",
            "Go to definition": "F12",
            "Peek definition": "Alt+F12",
            "Save": "Ctrl+S",
            "Close editor": "Ctrl+W",
            "Split editor": "Ctrl+\\",
            "Toggle comment": "Ctrl+/",
        },
        instructions=(
            "This is VS Code. Use Ctrl+Shift+P for the command palette. "
            "Ctrl+P opens quick file search. Ctrl+` toggles the terminal. "
            "Tab navigates between editor groups. Use F12 for go to definition."
        ),
    ),
    "notepad.exe": AppProfile(
        process_name="notepad.exe",
        display_name="Notepad",
        shortcuts={
            "Save": "Ctrl+S",
            "Open": "Ctrl+O",
            "Find": "Ctrl+F",
            "Replace": "Ctrl+H",
            "Go to line": "Ctrl+G",
            "Select all": "Ctrl+A",
        },
        instructions="This is Notepad. Simple text editor with standard Ctrl+key shortcuts.",
    ),
    "explorer.exe": AppProfile(
        process_name="explorer.exe",
        display_name="File Explorer",
        shortcuts={
            "Address bar": "Alt+D",
            "Search": "Ctrl+F",
            "New folder": "Ctrl+Shift+N",
            "Rename": "F2",
            "Delete": "Delete",
            "Properties": "Alt+Enter",
            "Select all": "Ctrl+A",
            "Back": "Alt+Left",
            "Up": "Alt+Up",
        },
        instructions=(
            "This is File Explorer. Navigate files with arrow keys. "
            "Enter opens files/folders. F2 renames. Alt+D focuses the address bar. "
            "Backspace goes up one folder."
        ),
    ),
}


def get_profile_for_process(process_name: str) -> AppProfile | None:
    """Look up an application profile by process name (case-insensitive)."""
    lower = process_name.lower()
    for key, profile in BUILTIN_PROFILES.items():
        if key.lower() == lower:
            return profile
    return None


def format_profile_for_prompt(profile: AppProfile) -> str:
    """Format an app profile as text to inject into a system prompt."""
    lines = [f"\n=== Application Profile: {profile.display_name} ==="]

    if profile.shortcuts:
        lines.append("Keyboard Shortcuts:")
        for action, shortcut in profile.shortcuts.items():
            lines.append(f"  {action}: {shortcut}")

    if profile.instructions:
        lines.append(f"\n{profile.instructions}")

    if profile.navigation_hints:
        lines.append(f"\nNavigation: {profile.navigation_hints}")

    lines.append("=== End Profile ===\n")
    return "\n".join(lines)
