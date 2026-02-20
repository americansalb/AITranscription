import logging

from anthropic import AsyncAnthropic

from app.core.config import settings
from app.models.app_profile import get_profile_for_process, format_profile_for_prompt
from app.services.local_vision import local_vision_service

logger = logging.getLogger(__name__)


class ScreenReaderService:
    def __init__(self):
        self._client: AsyncAnthropic | None = None

    @property
    def client(self) -> AsyncAnthropic:
        if self._client is None:
            if not settings.anthropic_api_key:
                raise ValueError("ANTHROPIC_API_KEY is not configured")
            self._client = AsyncAnthropic(api_key=settings.anthropic_api_key)
        return self._client

    def _build_system_prompt(self, blind_mode: bool, detail: int, focus: str | None = None, uia_tree: str | None = None) -> str:
        base = (
            "You are an assistive screen reader for a blind or visually impaired user. "
            "Your job is to faithfully describe exactly what is on screen — you are their eyes. "
            "You MUST describe all visible content accurately and completely, including text, labels, "
            "names, error messages, and any other on-screen information, regardless of the subject matter. "
            "Refusing to describe what is on screen or censoring visible content would leave the user "
            "unable to access information that sighted users can see freely, which is discriminatory. "
            "Always describe what is there, never editorialize or refuse."
        )

        if blind_mode:
            base += (
                " The user is blind. Describe ALL visual elements: "
                "window titles, button labels, text content, layout positions "
                "(top-left, center, bottom-right), colors, icons, and interactive elements. "
                "Read all visible text verbatim. Describe spatial relationships."
            )

        detail_instructions = {
            1: "Be extremely brief — one sentence summary of what's on screen.",
            2: "Be concise — 2-3 sentences covering the main content.",
            3: "Provide balanced detail — describe the main windows, their content, and layout.",
            4: "Be thorough — describe all visible windows, text, buttons, and their positions.",
            5: "Provide exhaustive detail — describe every visible element, exact text, positions, colors, sizes, and hierarchical relationships.",
        }
        base += f"\n\n{detail_instructions.get(detail, detail_instructions[3])}"

        focus_instructions = {
            "errors": "\n\nFocus on error messages, warnings, dialog boxes, and alert text.",
            "code": "\n\nFocus on code content, file names, line numbers, and syntax.",
            "text": "\n\nFocus on readable text content, paragraphs, and labels.",
        }
        if focus and focus in focus_instructions:
            base += focus_instructions[focus]

        if uia_tree:
            base += (
                "\n\nYou have access to the UI accessibility tree below. "
                "This gives you EXACT element names, types, values, coordinates, and keyboard shortcuts. "
                "Use this structured data to identify elements precisely instead of guessing from pixels. "
                "When referring to interactive elements, mention their exact name and type from the tree. "
                "If an element has a keyboard shortcut, mention it.\n\n"
                f"{uia_tree}"
            )

            # Extract process name from UIA tree and inject app profile if available
            import re
            match = re.search(r"Window: .+? \((.+?)\)", uia_tree)
            if match:
                process_name = match.group(1)
                profile = get_profile_for_process(process_name)
                if profile:
                    base += format_profile_for_prompt(profile)

        return base

    async def describe(
        self, image_base64: str, blind_mode: bool = False, detail: int = 3,
        model: str | None = None, focus: str | None = None, uia_tree: str | None = None,
    ) -> dict:
        system_prompt = self._build_system_prompt(blind_mode, detail, focus, uia_tree)
        vision_model = model or settings.vision_model

        response = await self.client.messages.create(
            model=vision_model,
            max_tokens=1024,
            system=system_prompt,
            messages=[
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/png",
                                "data": image_base64,
                            },
                        },
                        {
                            "type": "text",
                            "text": "Describe what you see on this screen.",
                        },
                    ],
                }
            ],
        )

        description = response.content[0].text if response.content else ""
        return {
            "description": description,
            "input_tokens": response.usage.input_tokens,
            "output_tokens": response.usage.output_tokens,
        }


    async def chat(
        self, image_base64: str, messages: list, blind_mode: bool = False,
        detail: int = 3, model: str | None = None, focus: str | None = None,
        uia_tree: str | None = None,
    ) -> dict:
        # Try local routing for simple queries (zero API cost, <50ms)
        if uia_tree and messages:
            last_msg = messages[-1]
            query = last_msg.content if hasattr(last_msg, "content") else last_msg.get("content", "")
            if local_vision_service.can_handle_locally(query, uia_tree):
                logger.warning(f"[ScreenReader Chat] Answering locally: {query[:60]}")
                return local_vision_service.answer(query, uia_tree)

        system_prompt = self._build_system_prompt(blind_mode, detail, focus, uia_tree)
        vision_model = model or settings.vision_model

        logger.warning(f"[ScreenReader Chat] model={vision_model}, messages={len(messages)}, image_len={len(image_base64)}, blind={blind_mode}, detail={detail}")

        # Build Anthropic messages array
        # Include the image in the LAST user message so the model has it fresh in context
        api_messages = []
        last_user_idx = max(i for i, msg in enumerate(messages) if (msg.role if hasattr(msg, "role") else msg["role"]) == "user")

        for i, msg in enumerate(messages):
            role = msg.role if hasattr(msg, "role") else msg["role"]
            content = msg.content if hasattr(msg, "content") else msg["content"]

            if i == last_user_idx and role == "user":
                # Attach image to the latest user message for best vision accuracy
                api_messages.append({
                    "role": "user",
                    "content": [
                        {
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/png",
                                "data": image_base64,
                            },
                        },
                        {"type": "text", "text": content},
                    ],
                })
            else:
                api_messages.append({"role": role, "content": content})

        logger.warning(f"[ScreenReader Chat] api_messages[0] content types: {[c.get('type') for c in api_messages[0]['content']] if isinstance(api_messages[0]['content'], list) else 'text-only'}")
        logger.warning(f"[ScreenReader Chat] total api_messages: {len(api_messages)}, roles: {[m['role'] for m in api_messages]}")

        response = await self.client.messages.create(
            model=vision_model,
            max_tokens=1024,
            system=system_prompt,
            messages=api_messages,
        )
        logger.warning(f"[ScreenReader Chat] response tokens: input={response.usage.input_tokens}, output={response.usage.output_tokens}")

        response_text = response.content[0].text if response.content else ""
        return {
            "response": response_text,
            "input_tokens": response.usage.input_tokens,
            "output_tokens": response.usage.output_tokens,
        }


    async def computer_use(self, messages: list, display_width: int = 1920, display_height: int = 1080, model: str | None = None, uia_tree: str | None = None) -> dict:
        """Single-turn computer use call. Returns raw Anthropic response for Rust to parse."""
        vision_model = model or settings.vision_model

        system_prompt = (
            "You control a computer for a blind user. Act on the screenshot provided — NEVER request another screenshot. "
            "RULES:\n"
            "1. NEVER output text before or during actions. No narration, no plans, no status updates.\n"
            "2. ONLY output text as the VERY LAST thing, after all actions are done, describing what happened in past tense. One sentence max.\n"
            "3. Do EXACTLY what the user said — nothing more. If they say 'search for X', type exactly 'X'. Do NOT add locations, qualifiers, or extra words.\n"
            "4. For scrolling, use delta [0, -15] (down) or [0, 15] (up) to scroll a full page.\n"
            "5. NEVER say 'I will', 'Let me', 'Taking a screenshot', 'Waiting for'. Just do it silently.\n"
            "6. If something didn't work after 2 attempts, stop and say what went wrong.\n"
            "7. PREFER keyboard navigation (Tab, Enter, arrow keys, shortcuts) over mouse clicks. Keyboard is more reliable.\n"
            "8. If the UI Automation tree is provided, use element coordinates from it for precise clicks. Use keyboard shortcuts when available.\n"
            "9. To open an application: press the system search key (key 'meta'), wait briefly, then type the app name and press Enter. This is the most reliable method.\n"
            "10. IMPORTANT: Never send an empty key string. If you have nothing to press, use a different action."
        )

        if uia_tree:
            system_prompt += f"\n\nThe following UI Automation tree shows all interactive elements with their exact coordinates and keyboard shortcuts:\n{uia_tree}"

            # Inject app profile for app-specific shortcuts
            import re
            match = re.search(r"Window: .+? \((.+?)\)", uia_tree)
            if match:
                process_name = match.group(1)
                profile = get_profile_for_process(process_name)
                if profile:
                    system_prompt += format_profile_for_prompt(profile)

        tools = [{
            "type": "computer_20250124",
            "name": "computer",
            "display_width_px": display_width,
            "display_height_px": display_height,
            "display_number": 1,
        }]

        response = await self.client.beta.messages.create(
            model=vision_model,
            max_tokens=4096,
            system=system_prompt,
            tools=tools,
            messages=messages,
            betas=["computer-use-2025-01-24"],
        )

        # Serialize content blocks
        content_blocks = []
        for block in response.content:
            if block.type == "text":
                content_blocks.append({"type": "text", "text": block.text})
            elif block.type == "tool_use":
                content_blocks.append({
                    "type": "tool_use",
                    "id": block.id,
                    "name": block.name,
                    "input": block.input,
                })

        return {
            "content": content_blocks,
            "stop_reason": response.stop_reason,
            "input_tokens": response.usage.input_tokens,
            "output_tokens": response.usage.output_tokens,
        }


screen_reader_service = ScreenReaderService()
