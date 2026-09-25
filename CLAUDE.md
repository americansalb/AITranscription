# Claude Code Instructions

## Voice Output

Always use the Scribe speak integration to read responses aloud.

**CRITICAL: Use the MCP `/speak` tool - NOT curl**

The `/speak` tool is available through the MCP server. It automatically manages stable session IDs based on your terminal process. Simply call the speak tool using MCP:

The session ID is handled automatically - all messages from this terminal will be grouped together in the same conversation.

**Session Management:**
- Each terminal window gets a unique session ID automatically (based on process ID)
- All Claude instances in the same terminal share the same session
- You don't need to track or pass session IDs manually
- NEVER use curl to call the speak endpoint directly

**How it works:**
- Session ID format: `{hostname}-{parent_process_id}`
- Same terminal = Same parent PID = Same session
- Different terminal = Different parent PID = Different session


## Detail Level: 5 out of 5

THE FULL SCALE (so you understand the range):
- Level 1 (Minimum): One sentence only. "I updated the login page."
- Level 2: 1-2 sentences. "I fixed the login button - the click handler was missing."
- Level 3 (Middle): Mention file names and explain why. "I modified LoginForm.tsx to fix the submit button by adding the missing onClick handler."
- Level 4: Include line numbers, technical details, and implications.
- Level 5 (Maximum): Full technical breakdown with architecture decisions, edge cases, all files touched, and implementation specifics.

YOU ARE AT LEVEL 5: This is MAXIMUM detail. Give a comprehensive technical breakdown. Mention every file you touched, explain your architecture decisions, cover edge cases, and describe implementation specifics. Developers want the full picture.

## Mode: Screen Reader

The user CANNOT see the screen. You MUST describe all visual information.

### ALWAYS do these things:
- Say the full file path when you modify a file
- Describe where UI elements are positioned (top-right, centered, below the header)
- Mention colors, sizes, and spacing when relevant
- Explain the visual hierarchy and structure of code
- Describe what's above, below, and beside changed elements

### NEVER do these things:
- Read code syntax character by character
- Assume the user can see anything on screen
- Skip describing the location of changes
- Use vague terms like "here" or "this" without context

## Working rules for this repository

- Nothing is ported from the old code. The branches `legacy`, `feature/strict-turn-discipline`, and `dev-local` are reference material only: read them for system call details, never copy from them.
- Version one has no window. The interface is a menu bar menu, one key, and speech. Every setting is changed by voice.
- Every error has a spoken sentence, enforced by a test. Silence is the one failure this app must never have.
- Operating-system calls live only in `crates/platform`. Pure logic lives in `crates/logic` and is tested on Linux against fixtures in `crates/logic/tests/fixtures`.
- Every change that adds a concept, such as a setting, a mode, a window, or a role, must remove or fold one.
- Tests run in CI on every push. A change without a passing test command does not merge.
- The complete state of the app must be describable aloud in under a minute. A feature that cannot be described in one spoken sentence does not ship.
- Ship before planning. No plan document longer than one page until the thing it plans exists.
- Every change to spoken output or screen description includes a recorded transcript of a real session showing its effect.
- The measure is task success: can the user finish a real task, start to end, without looking at the screen.
- The product name is data, not code. It is written only in the `PRODUCT_NAME` file at the repository root and the readme heading. Code reads it from the file. Never put it in a crate name, file name, identifier, string, or message; a test enforces this.
