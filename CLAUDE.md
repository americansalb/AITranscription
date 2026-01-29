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
