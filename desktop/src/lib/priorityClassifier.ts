/**
 * Smart Priority Queue - regex-based text classifier.
 * Classifies messages as critical, normal, or low priority.
 * Off by default, toggled in Preferences.
 */

export type PriorityLevel = 'critical' | 'normal' | 'low';

// Critical patterns: errors, failures, exceptions
const CRITICAL_PATTERNS = [
  /\b(error|exception|failed|failure|crash|fatal|panic|broken|unhandled)\b/i,
  /\b(cannot|could not|unable to|refused|denied|timeout|timed out)\b/i,
  /\bERROR\b/,
  /\bFAILED\b/,
  /\bCRITICAL\b/,
  /\bsyntax error\b/i,
  /\bstack trace\b/i,
  /\bsegfault\b/i,
  /\bruntime error\b/i,
  /\btype error\b/i,
  /\breference error\b/i,
  /\bbuild failed\b/i,
  /\bcompilation error\b/i,
  /\btest(s)? failed\b/i,
];

// Low priority patterns: confirmations, simple status
const LOW_PATTERNS = [
  /^(saved|done|success|ok|completed|finished|ready)\b/i,
  /\b(saved successfully|operation complete|task done)\b/i,
  /\bno changes\b/i,
  /\bup to date\b/i,
  /\balready exists\b/i,
  /\bnothing to (do|commit|update)\b/i,
  /^(created|updated|deleted|removed|added) \w+\s*$/i,
];

/**
 * Classify text into a priority level.
 */
export function classifyPriority(text: string): PriorityLevel {
  // Check critical first
  for (const pattern of CRITICAL_PATTERNS) {
    if (pattern.test(text)) {
      return 'critical';
    }
  }

  // Check low priority
  for (const pattern of LOW_PATTERNS) {
    if (pattern.test(text)) {
      return 'low';
    }
  }

  return 'normal';
}

// localStorage key for priority toggle
const PRIORITY_ENABLED_KEY = 'vaak_priority_enabled';

export function getStoredPriorityEnabled(): boolean {
  try {
    return localStorage.getItem(PRIORITY_ENABLED_KEY) === 'true';
  } catch {
    return false; // Off by default
  }
}

export function savePriorityEnabled(enabled: boolean): void {
  try {
    localStorage.setItem(PRIORITY_ENABLED_KEY, enabled ? 'true' : 'false');
  } catch {
    // ignore
  }
}
