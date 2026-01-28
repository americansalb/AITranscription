# Scribe — Monetization Analysis

## Product Summary

Scribe is a desktop voice-to-text app (Tauri + React + FastAPI) that records speech, transcribes via Groq Whisper, polishes with Claude Haiku, and auto-pastes into the focused application. It includes a learning system (vector embeddings for user corrections) and a Claude Code MCP integration for spoken code-change explanations.

## Current Pricing Tiers (in codebase)

| Tier | Price | Target |
|------|-------|--------|
| Developer | Free | Testing/development |
| Access | ~$2.50/mo | Verified disabled users |
| Standard | $5/mo | General public |
| Enterprise | Custom | API access, teams |

## Strengths

- **Real pain point**: Dictation → polished text → auto-paste is genuinely useful
- **Learning moat**: Per-user correction embeddings improve quality over time, creating switching costs
- **Accessibility mission**: Strong narrative for grants, partnerships, and PR
- **Claude Code MCP bridge**: Unique developer-facing feature (niche but differentiated)
- **Context presets**: Email/Slack/Code/Document modes add real value vs. generic dictation

## Risks and Weaknesses

### Competitive Pressure
- Apple Dictation, Google Voice Typing, Windows Speech Recognition are free and improving
- Otter.ai, Whisper.cpp (local/free), and dozens of transcription startups exist
- OpenAI's own products are moving into this space

### Unit Economics
- Every request hits Groq (transcription) + Anthropic (polish) + optionally ElevenLabs (TTS)
- At $5/mo with heavy users, API costs could exceed revenue
- No usage caps or cost controls implemented beyond a daily transcription limit DB field

### Missing Infrastructure
- No payment processing (Stripe, Paddle, etc.) — tiers exist in DB only
- Backend runs on Render free tier — no SLA, cold starts, resource limits
- No rate limiting or abuse prevention for API endpoints
- No usage-based billing or metering

### Platform Limitations
- Desktop-only (Tauri doesn't support mobile)
- Voice input arguably more valuable on mobile (meetings, commutes)
- No web version for quick access

### Enterprise Gaps
- No SSO/SAML
- No team management or shared dictionaries
- No audit logs or compliance features
- No on-premise deployment option

## Recommended Monetization Strategy

### Short-Term (Revenue Prerequisites)
1. Integrate Stripe for subscription billing
2. Implement usage metering (per-minute or per-word pricing)
3. Add hard usage caps per tier to control API costs
4. Move backend to production infrastructure (not free tier)
5. Add rate limiting and abuse prevention

### Medium-Term (Market Positioning)
1. **Target professionals** — legal, medical, journalism verticals where willingness to pay is high and custom dictionaries are valued
2. **Usage-based pricing** — base fee + per-minute transcription charges to align costs with revenue
3. **Local Whisper option** — offer on-device transcription to reduce API costs and improve privacy story
4. **Team/org features** — shared dictionaries, admin dashboards, centralized billing

### Long-Term (Scaling)
1. Enterprise tier with SSO, compliance, on-prem
2. API-as-a-service for third-party integrations
3. Mobile app (React Native or similar)
4. Marketplace for context presets and correction rule packs

## Verdict

**Monetizable, but not easily at the current $5/mo consumer price point.** The learning system is a genuine moat, but API cost pressure and free OS-level alternatives make consumer pricing difficult. The strongest revenue path is **professional/enterprise** use cases (legal, medical, accessibility compliance) where willingness to pay is higher and the custom dictionary + learned corrections become truly valuable.

The Claude integration (polishing + MCP bridge) is a differentiator for developer users but too niche to be the primary revenue driver. The broader voice-to-polished-text pipeline with per-user learning is the core product value.
