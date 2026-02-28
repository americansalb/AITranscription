import { LANGUAGES, TARGET_LANGUAGES } from "../lib/languages";

export type AppMode = "interpret" | "transcribe";
export type TranslationDirection = "unidirectional" | "bidirectional";
export type TranslationTiming = "consecutive" | "simultaneous";
export type ConsecutiveTrigger = "auto" | "manual";

export interface InterpretationSettings {
  mode: AppMode;
  sourceLang: string;
  targetLang: string;
  direction: TranslationDirection;
  timing: TranslationTiming;
  trigger: ConsecutiveTrigger;
  /** Silence threshold in seconds for auto-consecutive mode. */
  silenceThreshold: number;
  /** Selected LLM provider for translation. */
  provider: string;
  /** Whether to read translations aloud. */
  ttsEnabled: boolean;
  /** Voice URI for TTS (empty = system default). */
  ttsVoice: string;
  /** Seconds of no new text before TTS triggers. */
  ttsSilenceDelay: number;
  /** Speech rate (0.5–2.0). */
  ttsRate: number;
}

export const DEFAULT_SETTINGS: InterpretationSettings = {
  mode: "interpret",
  sourceLang: "auto",
  targetLang: "en",
  direction: "unidirectional",
  timing: "consecutive",
  trigger: "auto",
  silenceThreshold: 2.0,
  provider: "claude-opus",
  ttsEnabled: false,
  ttsVoice: "",
  ttsSilenceDelay: 2.0,
  ttsRate: 1.0,
};

interface SettingsPanelProps {
  settings: InterpretationSettings;
  onChange: (settings: InterpretationSettings) => void;
  availableProviders: { id: string; model: string }[];
  /** Voices available for the current target language. */
  ttsVoices?: SpeechSynthesisVoice[];
  disabled?: boolean;
}

export function SettingsPanel({ settings, onChange, availableProviders, ttsVoices = [], disabled }: SettingsPanelProps) {
  const update = (patch: Partial<InterpretationSettings>) => {
    onChange({ ...settings, ...patch });
  };

  const isTranslation = settings.mode === "interpret";

  return (
    <div className="settings-panel">
      {/* Mode: Interpret vs Transcribe-only */}
      <div className="settings-row">
        <div className="setting-group">
          <span className="group-label">Mode</span>
          <div className="toggle-group">
            <button
              className={settings.mode === "interpret" ? "active" : ""}
              onClick={() => update({ mode: "interpret" })}
              disabled={disabled}
            >
              Interpret
            </button>
            <button
              className={settings.mode === "transcribe" ? "active" : ""}
              onClick={() => update({ mode: "transcribe" })}
              disabled={disabled}
            >
              Transcribe Only
            </button>
          </div>
        </div>
      </div>

      {/* Languages */}
      <div className="settings-row lang-row">
        <div className="setting-field">
          <label htmlFor="source-lang">{isTranslation ? "Source" : "Language"}</label>
          <select
            id="source-lang"
            value={settings.sourceLang}
            onChange={(e) => update({ sourceLang: e.target.value })}
            disabled={disabled}
          >
            {LANGUAGES.map((l) => (
              <option key={l.code} value={l.code}>{l.name}</option>
            ))}
          </select>
        </div>

        {isTranslation && (
          <>
            <button
              className="swap-btn"
              onClick={() => {
                if (settings.sourceLang !== "auto") {
                  update({ sourceLang: settings.targetLang, targetLang: settings.sourceLang });
                }
              }}
              disabled={disabled || settings.sourceLang === "auto"}
              title="Swap languages"
              aria-label="Swap source and target languages"
            >
              &#8646;
            </button>

            <div className="setting-field">
              <label htmlFor="target-lang">Target</label>
              <select
                id="target-lang"
                value={settings.targetLang}
                onChange={(e) => update({ targetLang: e.target.value })}
                disabled={disabled}
              >
                {TARGET_LANGUAGES.map((l) => (
                  <option key={l.code} value={l.code}>{l.name}</option>
                ))}
              </select>
            </div>
          </>
        )}
      </div>

      {/* Translation-specific settings */}
      {isTranslation && (
        <>
          {/* Direction */}
          <div className="settings-row">
            <div className="setting-group">
              <span className="group-label">Direction</span>
              <div className="toggle-group">
                <button
                  className={settings.direction === "unidirectional" ? "active" : ""}
                  onClick={() => update({ direction: "unidirectional" })}
                  disabled={disabled}
                >
                  Unidirectional
                </button>
                <button
                  className={settings.direction === "bidirectional" ? "active" : ""}
                  onClick={() => update({ direction: "bidirectional" })}
                  disabled={disabled}
                >
                  Bidirectional
                </button>
              </div>
            </div>
          </div>

          {/* LLM Provider */}
          <div className="settings-row">
            <div className="setting-field">
              <label htmlFor="provider">Translation LLM</label>
              <select
                id="provider"
                value={settings.provider}
                onChange={(e) => update({ provider: e.target.value })}
                disabled={disabled}
              >
                {availableProviders.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.id === "claude-opus" ? "Claude Opus" : p.id === "claude-sonnet" ? "Claude Sonnet" : p.id === "gpt" ? "GPT" : p.id === "groq" ? "Groq (Llama)" : p.id === "gemini" ? "Gemini" : p.id}
                  </option>
                ))}
                {availableProviders.length === 0 && (
                  <option disabled>No providers configured</option>
                )}
              </select>
            </div>
          </div>

          {/* Read Aloud (TTS) */}
          <div className="settings-row">
            <div className="setting-group">
              <span className="group-label">Read Aloud</span>
              <div className="toggle-group">
                <button
                  className={settings.ttsEnabled ? "active" : ""}
                  onClick={() => update({ ttsEnabled: true })}
                  disabled={disabled}
                >
                  On
                </button>
                <button
                  className={!settings.ttsEnabled ? "active" : ""}
                  onClick={() => update({ ttsEnabled: false })}
                  disabled={disabled}
                >
                  Off
                </button>
              </div>
            </div>
          </div>

          {settings.ttsEnabled && (
            <>
              <div className="settings-row">
                <div className="setting-field">
                  <label htmlFor="tts-voice">Voice</label>
                  <select
                    id="tts-voice"
                    value={settings.ttsVoice}
                    onChange={(e) => update({ ttsVoice: e.target.value })}
                    disabled={disabled}
                  >
                    <option value="">System Default</option>
                    {ttsVoices.map((v) => (
                      <option key={v.voiceURI} value={v.voiceURI}>
                        {v.name} ({v.lang})
                      </option>
                    ))}
                  </select>
                </div>
              </div>

              <div className="settings-row">
                <div className="setting-field silence-field">
                  <label htmlFor="tts-delay">Read after: {settings.ttsSilenceDelay.toFixed(1)}s</label>
                  <input
                    id="tts-delay"
                    type="range"
                    min="0.5"
                    max="5.0"
                    step="0.5"
                    value={settings.ttsSilenceDelay}
                    onChange={(e) => update({ ttsSilenceDelay: parseFloat(e.target.value) })}
                    disabled={disabled}
                  />
                </div>

                <div className="setting-field silence-field">
                  <label htmlFor="tts-rate">Speed: {settings.ttsRate.toFixed(1)}x</label>
                  <input
                    id="tts-rate"
                    type="range"
                    min="0.5"
                    max="2.0"
                    step="0.1"
                    value={settings.ttsRate}
                    onChange={(e) => update({ ttsRate: parseFloat(e.target.value) })}
                    disabled={disabled}
                  />
                </div>
              </div>
            </>
          )}
        </>
      )}

      {/* Timing — applies to both modes */}
      <div className="settings-row">
        <div className="setting-group">
          <span className="group-label">Timing</span>
          <div className="toggle-group">
            <button
              className={settings.timing === "consecutive" ? "active" : ""}
              onClick={() => update({ timing: "consecutive" })}
              disabled={disabled}
            >
              Consecutive
            </button>
            <button
              className={settings.timing === "simultaneous" ? "active" : ""}
              onClick={() => update({ timing: "simultaneous" })}
              disabled={disabled}
            >
              Simultaneous
            </button>
          </div>
        </div>
      </div>

      {/* Consecutive trigger options */}
      {settings.timing === "consecutive" && (
        <div className="settings-row">
          <div className="setting-group">
            <span className="group-label">Trigger</span>
            <div className="toggle-group">
              <button
                className={settings.trigger === "auto" ? "active" : ""}
                onClick={() => update({ trigger: "auto" })}
                disabled={disabled}
              >
                Auto
              </button>
              <button
                className={settings.trigger === "manual" ? "active" : ""}
                onClick={() => update({ trigger: "manual" })}
                disabled={disabled}
              >
                Manual
              </button>
            </div>
          </div>

          {settings.trigger === "auto" && (
            <div className="setting-field silence-field">
              <label htmlFor="silence-threshold">Pause: {settings.silenceThreshold.toFixed(1)}s</label>
              <input
                id="silence-threshold"
                type="range"
                min="0.5"
                max="5.0"
                step="0.5"
                value={settings.silenceThreshold}
                onChange={(e) => update({ silenceThreshold: parseFloat(e.target.value) })}
                disabled={disabled}
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}
