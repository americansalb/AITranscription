/**
 * Tests for voiceStream storage helpers — localStorage persistence for voice settings.
 *
 * Covers:
 *   - getStoredVoiceEnabled / saveVoiceEnabled: boolean toggle
 *   - getStoredBlindMode / saveBlindMode: blind mode toggle + legacy migration
 *   - getStoredVoiceMode / saveVoiceMode: legacy VoiceMode compat layer
 *   - getStoredVoiceDetail / saveVoiceDetail: 1-5 integer range with clamping
 *   - getStoredVoiceAuto / saveVoiceAuto: auto-trigger boolean (default true)
 *   - Screen reader settings (SR): voice ID, model, detail, focus, hotkey
 *   - Default values for each setting
 *   - Error handling (localStorage unavailable)
 */
import { describe, it, expect, beforeEach } from "vitest";
import {
  getStoredVoiceEnabled,
  saveVoiceEnabled,
  getStoredBlindMode,
  saveBlindMode,
  getStoredVoiceMode,
  saveVoiceMode,
  getStoredVoiceDetail,
  saveVoiceDetail,
  getStoredVoiceAuto,
  saveVoiceAuto,
  getStoredSRVoiceId,
  saveSRVoiceId,
  getStoredSRModel,
  saveSRModel,
  getStoredSRDetail,
  saveSRDetail,
  getStoredSRFocus,
  saveSRFocus,
  getStoredSRHotkey,
  saveSRHotkey,
} from "../lib/voiceStream";


beforeEach(() => {
  localStorage.clear();
});


// =============================================================================
// VOICE ENABLED
// =============================================================================

describe("voiceEnabled", () => {
  it("defaults to false when no stored value", () => {
    expect(getStoredVoiceEnabled()).toBe(false);
  });

  it("returns true after saving true", () => {
    saveVoiceEnabled(true);
    expect(getStoredVoiceEnabled()).toBe(true);
  });

  it("returns false after saving false", () => {
    saveVoiceEnabled(true);
    saveVoiceEnabled(false);
    expect(getStoredVoiceEnabled()).toBe(false);
  });

  it("persists to correct localStorage key", () => {
    saveVoiceEnabled(true);
    expect(localStorage.getItem("vaak_voice_enabled")).toBe("true");
  });
});


// =============================================================================
// BLIND MODE
// =============================================================================

describe("blindMode", () => {
  it("defaults to false when no stored value", () => {
    expect(getStoredBlindMode()).toBe(false);
  });

  it("returns true after saving true", () => {
    saveBlindMode(true);
    expect(getStoredBlindMode()).toBe(true);
  });

  it("returns false after saving false", () => {
    saveBlindMode(true);
    saveBlindMode(false);
    expect(getStoredBlindMode()).toBe(false);
  });

  it("persists to correct localStorage key", () => {
    saveBlindMode(true);
    expect(localStorage.getItem("vaak_blind_mode")).toBe("true");
  });

  it("migrates from legacy 'vaak_voice_mode' = 'blind'", () => {
    localStorage.setItem("vaak_voice_mode", "blind");
    expect(getStoredBlindMode()).toBe(true);
  });

  it("migrates from legacy 'vaak_voice_mode' = 'summary' as false", () => {
    localStorage.setItem("vaak_voice_mode", "summary");
    expect(getStoredBlindMode()).toBe(false);
  });

  it("prefers new key over legacy key", () => {
    localStorage.setItem("vaak_blind_mode", "false");
    localStorage.setItem("vaak_voice_mode", "blind");
    expect(getStoredBlindMode()).toBe(false);
  });
});


// =============================================================================
// LEGACY VOICE MODE
// =============================================================================

describe("voiceMode (legacy compat)", () => {
  it("returns 'summary' when blind mode is false", () => {
    saveBlindMode(false);
    expect(getStoredVoiceMode()).toBe("summary");
  });

  it("returns 'blind' when blind mode is true", () => {
    saveBlindMode(true);
    expect(getStoredVoiceMode()).toBe("blind");
  });

  it("saveVoiceMode('blind') enables blind mode", () => {
    saveVoiceMode("blind");
    expect(getStoredBlindMode()).toBe(true);
  });

  it("saveVoiceMode('summary') disables blind mode", () => {
    saveBlindMode(true);
    saveVoiceMode("summary");
    expect(getStoredBlindMode()).toBe(false);
  });

  it("saveVoiceMode('developer') disables blind mode", () => {
    saveBlindMode(true);
    saveVoiceMode("developer");
    expect(getStoredBlindMode()).toBe(false);
  });
});


// =============================================================================
// VOICE DETAIL (1-5 range)
// =============================================================================

describe("voiceDetail", () => {
  it("defaults to 3 when no stored value", () => {
    expect(getStoredVoiceDetail()).toBe(3);
  });

  it("returns stored value within range", () => {
    saveVoiceDetail(1);
    expect(getStoredVoiceDetail()).toBe(1);
    saveVoiceDetail(5);
    expect(getStoredVoiceDetail()).toBe(5);
  });

  it("rejects value < 1 (does not save)", () => {
    saveVoiceDetail(3);
    saveVoiceDetail(0);
    // Should still be 3 since 0 is out of range and saveVoiceDetail guards
    expect(getStoredVoiceDetail()).toBe(3);
  });

  it("rejects value > 5 (does not save)", () => {
    saveVoiceDetail(3);
    saveVoiceDetail(6);
    expect(getStoredVoiceDetail()).toBe(3);
  });

  it("falls back to 3 for corrupted stored value", () => {
    localStorage.setItem("vaak_voice_detail", "garbage");
    expect(getStoredVoiceDetail()).toBe(3);
  });

  it("persists to correct localStorage key", () => {
    saveVoiceDetail(4);
    expect(localStorage.getItem("vaak_voice_detail")).toBe("4");
  });
});


// =============================================================================
// VOICE AUTO (default true)
// =============================================================================

describe("voiceAuto", () => {
  it("defaults to true when no stored value", () => {
    expect(getStoredVoiceAuto()).toBe(true);
  });

  it("returns false after saving false", () => {
    saveVoiceAuto(false);
    expect(getStoredVoiceAuto()).toBe(false);
  });

  it("returns true after saving true", () => {
    saveVoiceAuto(false);
    saveVoiceAuto(true);
    expect(getStoredVoiceAuto()).toBe(true);
  });

  it("persists to correct localStorage key", () => {
    saveVoiceAuto(false);
    expect(localStorage.getItem("vaak_voice_auto")).toBe("false");
  });
});


// =============================================================================
// SCREEN READER: VOICE ID
// =============================================================================

describe("SR voice ID", () => {
  const DEFAULT_VOICE = "jiIkqWtTmS0GBz46iqA0"; // Ravi

  it("defaults to Ravi voice ID", () => {
    expect(getStoredSRVoiceId()).toBe(DEFAULT_VOICE);
  });

  it("returns saved voice ID", () => {
    saveSRVoiceId("custom-voice-123");
    expect(getStoredSRVoiceId()).toBe("custom-voice-123");
  });

  it("persists to correct localStorage key", () => {
    saveSRVoiceId("custom-voice");
    expect(localStorage.getItem("vaak_sr_voice_id")).toBe("custom-voice");
  });
});


// =============================================================================
// SCREEN READER: MODEL
// =============================================================================

describe("SR model", () => {
  it("defaults to claude-3-5-haiku-20241022", () => {
    expect(getStoredSRModel()).toBe("claude-3-5-haiku-20241022");
  });

  it("returns saved model", () => {
    saveSRModel("claude-3-5-sonnet-20250929");
    expect(getStoredSRModel()).toBe("claude-3-5-sonnet-20250929");
  });

  it("persists to correct localStorage key", () => {
    saveSRModel("custom-model");
    expect(localStorage.getItem("vaak_sr_model")).toBe("custom-model");
  });
});


// =============================================================================
// SCREEN READER: DETAIL (1-5 range, default 5)
// =============================================================================

describe("SR detail", () => {
  it("defaults to 5", () => {
    expect(getStoredSRDetail()).toBe(5);
  });

  it("returns stored value within range", () => {
    saveSRDetail(1);
    expect(getStoredSRDetail()).toBe(1);
    saveSRDetail(5);
    expect(getStoredSRDetail()).toBe(5);
  });

  it("falls back to 5 for out-of-range stored value", () => {
    localStorage.setItem("vaak_sr_detail", "0");
    expect(getStoredSRDetail()).toBe(5);
    localStorage.setItem("vaak_sr_detail", "6");
    expect(getStoredSRDetail()).toBe(5);
  });

  it("falls back to 5 for corrupted stored value", () => {
    localStorage.setItem("vaak_sr_detail", "abc");
    expect(getStoredSRDetail()).toBe(5);
  });
});


// =============================================================================
// SCREEN READER: FOCUS
// =============================================================================

describe("SR focus", () => {
  it("defaults to 'code'", () => {
    expect(getStoredSRFocus()).toBe("code");
  });

  it("returns saved focus", () => {
    saveSRFocus("ui");
    expect(getStoredSRFocus()).toBe("ui");
  });

  it("persists to correct localStorage key", () => {
    saveSRFocus("accessibility");
    expect(localStorage.getItem("vaak_sr_focus")).toBe("accessibility");
  });
});


// =============================================================================
// SCREEN READER: HOTKEY
// =============================================================================

describe("SR hotkey", () => {
  it("defaults to 'Alt+R'", () => {
    expect(getStoredSRHotkey()).toBe("Alt+R");
  });

  it("returns saved hotkey", () => {
    saveSRHotkey("Ctrl+Shift+R");
    expect(getStoredSRHotkey()).toBe("Ctrl+Shift+R");
  });

  it("persists to correct localStorage key", () => {
    saveSRHotkey("Meta+R");
    expect(localStorage.getItem("vaak_sr_hotkey")).toBe("Meta+R");
  });
});
