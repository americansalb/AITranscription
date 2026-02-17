/**
 * Tests for statsEquivalents — real-world stat equivalents calculator.
 *
 * Covers:
 *   - getWordEquivalent: tweets → emails → blog posts → short stories → novels
 *   - getWordEquivalentBrief: compact string format
 *   - getTimeSavedEquivalent: coffee → lunch → workout → Netflix → work day
 *   - getTimeSavedDescription: full sentence format
 *   - getAudioEquivalent: songs → podcasts → feature film → audiobook hours
 *   - getPercentileDescription: ranking descriptions
 *   - formatLargeNumber: K/M abbreviations
 *   - formatDurationCompact: seconds → minutes → hours
 *   - formatDurationVerbose: human-readable verbose
 *   - getStreakMilestone: next milestone calculation
 *   - calculateWoWChange: week-over-week percentage
 *   - getMilestoneMessage: celebration messages
 */
import { describe, it, expect } from "vitest";
import {
  getWordEquivalent,
  getWordEquivalentBrief,
  getTimeSavedEquivalent,
  getTimeSavedDescription,
  getAudioEquivalent,
  getPercentileDescription,
  formatLargeNumber,
  formatDurationCompact,
  formatDurationVerbose,
  getStreakMilestone,
  calculateWoWChange,
  getMilestoneMessage,
} from "../lib/statsEquivalents";


// =============================================================================
// WORD EQUIVALENTS
// =============================================================================

describe("getWordEquivalent", () => {
  it("returns tweets for < 500 words", () => {
    const result = getWordEquivalent(200);
    expect(result.unit).toContain("tweet");
    expect(result.count).toBe(4); // 200/50 = 4
  });

  it("returns at least 1 tweet for very small counts", () => {
    const result = getWordEquivalent(10);
    expect(result.count).toBeGreaterThanOrEqual(1);
    expect(result.unit).toContain("tweet");
  });

  it("returns emails for 500-1999 words", () => {
    const result = getWordEquivalent(1000);
    expect(result.unit).toContain("email");
    expect(result.count).toBe(5); // 1000/200 = 5
  });

  it("returns blog posts for 2000-9999 words", () => {
    const result = getWordEquivalent(3000);
    expect(result.unit).toContain("blog post");
    expect(result.count).toBe(2); // 3000/1500 = 2
  });

  it("returns short stories for 10000-49999 words", () => {
    const result = getWordEquivalent(15000);
    expect(result.unit).toContain("short stor");
    expect(result.count).toBe(2); // 15000/7500 = 2
  });

  it("returns novels for >= 50000 words", () => {
    const result = getWordEquivalent(80000);
    expect(result.unit).toContain("novel");
    expect(result.count).toBe(1); // 80000/80000 = 1
  });

  it("returns plural for multiple novels", () => {
    const result = getWordEquivalent(240000);
    expect(result.unit).toBe("novels");
    expect(result.count).toBe(3);
  });

  it("returns singular for exactly 1 tweet", () => {
    const result = getWordEquivalent(50);
    expect(result.unit).toBe("tweet");
  });
});

describe("getWordEquivalentBrief", () => {
  it("returns compact format", () => {
    const result = getWordEquivalentBrief(200);
    expect(result).toBe("4 tweets");
  });

  it("returns singular for 1 unit", () => {
    const result = getWordEquivalentBrief(50);
    expect(result).toBe("1 tweet");
  });
});


// =============================================================================
// TIME SAVED EQUIVALENTS
// =============================================================================

describe("getTimeSavedEquivalent", () => {
  it("returns coffee break for < 5 minutes", () => {
    const result = getTimeSavedEquivalent(120); // 2 min
    expect(result.description).toBe("a coffee break");
  });

  it("returns lunch break for 5-29 minutes", () => {
    const result = getTimeSavedEquivalent(900); // 15 min
    expect(result.description).toBe("a lunch break");
  });

  it("returns workout for 30-59 minutes", () => {
    const result = getTimeSavedEquivalent(2400); // 40 min
    expect(result.description).toBe("a workout session");
  });

  it("returns Netflix for 1-3 hours", () => {
    const result = getTimeSavedEquivalent(7200); // 2 hours
    expect(result.description).toBe("a Netflix binge");
  });

  it("returns work day for 4-7 hours", () => {
    const result = getTimeSavedEquivalent(21600); // 6 hours
    expect(result.description).toBe("a full work day");
  });

  it("returns N work days for >= 8 hours", () => {
    const result = getTimeSavedEquivalent(57600); // 16 hours
    expect(result.description).toBe("2 work days");
  });

  it("returns singular for exactly 1 work day", () => {
    const result = getTimeSavedEquivalent(28800); // 8 hours
    expect(result.description).toBe("1 work day");
  });
});

describe("getTimeSavedDescription", () => {
  it("returns full sentence format", () => {
    const result = getTimeSavedDescription(120);
    expect(result).toBe("That's enough time for a coffee break");
  });
});


// =============================================================================
// AUDIO EQUIVALENTS
// =============================================================================

describe("getAudioEquivalent", () => {
  it("returns songs for < 5 minutes", () => {
    const result = getAudioEquivalent(3);
    expect(result.unit).toContain("song");
    expect(result.count).toBeGreaterThanOrEqual(1);
  });

  it("returns songs for 5-44 minutes", () => {
    const result = getAudioEquivalent(14); // ~4 songs
    expect(result.unit).toBe("songs");
    expect(result.count).toBe(4);
  });

  it("returns podcast episodes for 45-119 minutes", () => {
    const result = getAudioEquivalent(90); // 2 podcasts
    expect(result.unit).toContain("podcast");
    expect(result.count).toBe(2);
  });

  it("returns feature film for 120-179 minutes", () => {
    const result = getAudioEquivalent(150);
    expect(result.unit).toBe("feature film");
    expect(result.count).toBe(1);
  });

  it("returns audiobook hours for >= 180 minutes", () => {
    const result = getAudioEquivalent(300); // 5 hours
    expect(result.unit).toBe("audiobook hours");
    expect(result.count).toBe(5);
  });
});


// =============================================================================
// PERCENTILE DESCRIPTIONS
// =============================================================================

describe("getPercentileDescription", () => {
  it("returns 'Top 1%' for >= 99", () => {
    expect(getPercentileDescription(99)).toBe("Top 1% of users");
    expect(getPercentileDescription(100)).toBe("Top 1% of users");
  });

  it("returns 'Top 5%' for 95-98", () => {
    expect(getPercentileDescription(95)).toBe("Top 5% of users");
    expect(getPercentileDescription(98)).toBe("Top 5% of users");
  });

  it("returns 'Top 10%' for 90-94", () => {
    expect(getPercentileDescription(90)).toBe("Top 10% of users");
  });

  it("returns 'Top 25%' for 75-84", () => {
    expect(getPercentileDescription(75)).toBe("Top 25% of users");
  });

  it("returns 'Above average' for 50-74", () => {
    expect(getPercentileDescription(50)).toBe("Above average");
    expect(getPercentileDescription(74)).toBe("Above average");
  });

  it("returns 'Getting started' for < 50", () => {
    expect(getPercentileDescription(49)).toBe("Getting started");
    expect(getPercentileDescription(0)).toBe("Getting started");
  });
});


// =============================================================================
// FORMAT LARGE NUMBER
// =============================================================================

describe("formatLargeNumber", () => {
  it("returns plain number for < 1000", () => {
    expect(formatLargeNumber(42)).toBe("42");
    expect(formatLargeNumber(999)).toBe("999");
  });

  it("returns locale string for 1000-9999", () => {
    const result = formatLargeNumber(5000);
    // toLocaleString varies by environment, just check it's not K-abbreviated
    expect(result).not.toContain("K");
  });

  it("returns K format for 10000-999999", () => {
    expect(formatLargeNumber(10000)).toBe("10.0K");
    expect(formatLargeNumber(50000)).toBe("50.0K");
    expect(formatLargeNumber(999999)).toBe("1000.0K");
  });

  it("returns M format for >= 1000000", () => {
    expect(formatLargeNumber(1000000)).toBe("1.0M");
    expect(formatLargeNumber(2500000)).toBe("2.5M");
  });

  it("handles zero", () => {
    expect(formatLargeNumber(0)).toBe("0");
  });
});


// =============================================================================
// FORMAT DURATION COMPACT
// =============================================================================

describe("formatDurationCompact", () => {
  it("returns seconds for < 60s", () => {
    expect(formatDurationCompact(30)).toBe("30s");
    expect(formatDurationCompact(0)).toBe("0s");
  });

  it("returns minutes only when no hours", () => {
    expect(formatDurationCompact(300)).toBe("5m");
    expect(formatDurationCompact(60)).toBe("1m");
  });

  it("returns hours only when no leftover minutes", () => {
    expect(formatDurationCompact(3600)).toBe("1h");
    expect(formatDurationCompact(7200)).toBe("2h");
  });

  it("returns hours and minutes together", () => {
    expect(formatDurationCompact(3900)).toBe("1h 5m");
    expect(formatDurationCompact(5400)).toBe("1h 30m");
  });

  it("rounds fractional seconds", () => {
    expect(formatDurationCompact(30.7)).toBe("31s");
  });
});


// =============================================================================
// FORMAT DURATION VERBOSE
// =============================================================================

describe("formatDurationVerbose", () => {
  it("returns seconds for < 60s", () => {
    expect(formatDurationVerbose(30)).toBe("30 seconds");
  });

  it("returns singular minute", () => {
    expect(formatDurationVerbose(60)).toBe("1 minute");
  });

  it("returns plural minutes", () => {
    expect(formatDurationVerbose(300)).toBe("5 minutes");
  });

  it("returns singular hour", () => {
    expect(formatDurationVerbose(3600)).toBe("1 hour");
  });

  it("returns plural hours", () => {
    expect(formatDurationVerbose(7200)).toBe("2 hours");
  });

  it("returns hours and minutes together", () => {
    expect(formatDurationVerbose(3900)).toBe("1 hour 5 minutes");
  });

  it("handles plural hours with singular minute", () => {
    expect(formatDurationVerbose(7260)).toBe("2 hours 1 minute");
  });
});


// =============================================================================
// STREAK MILESTONES
// =============================================================================

describe("getStreakMilestone", () => {
  it("returns 7-day milestone for day 1", () => {
    const result = getStreakMilestone(1);
    expect(result).not.toBeNull();
    expect(result!.milestone).toBe(7);
    expect(result!.remaining).toBe(6);
    expect(result!.description).toBe("1 week streak");
  });

  it("returns 14-day milestone for day 8", () => {
    const result = getStreakMilestone(8);
    expect(result).not.toBeNull();
    expect(result!.milestone).toBe(14);
    expect(result!.remaining).toBe(6);
  });

  it("returns 30-day milestone for day 15", () => {
    const result = getStreakMilestone(15);
    expect(result).not.toBeNull();
    expect(result!.milestone).toBe(30);
    expect(result!.remaining).toBe(15);
  });

  it("returns 365-day milestone for day 101", () => {
    const result = getStreakMilestone(101);
    expect(result).not.toBeNull();
    expect(result!.milestone).toBe(365);
    expect(result!.remaining).toBe(264);
  });

  it("returns null when past all milestones", () => {
    expect(getStreakMilestone(400)).toBeNull();
  });

  it("returns correct milestone for exact boundary", () => {
    // Day 7 should target 14
    const result = getStreakMilestone(7);
    expect(result!.milestone).toBe(14);
  });
});


// =============================================================================
// WEEK-OVER-WEEK CHANGE
// =============================================================================

describe("calculateWoWChange", () => {
  it("returns 'up' for significant increase", () => {
    const result = calculateWoWChange(120, 100);
    expect(result.direction).toBe("up");
    expect(result.percentage).toBe(20);
    expect(result.description).toContain("more than last week");
  });

  it("returns 'down' for significant decrease", () => {
    const result = calculateWoWChange(80, 100);
    expect(result.direction).toBe("down");
    expect(result.percentage).toBe(20);
    expect(result.description).toContain("less than last week");
  });

  it("returns 'same' for small changes (< 5%)", () => {
    const result = calculateWoWChange(102, 100);
    expect(result.direction).toBe("same");
    expect(result.description).toContain("About the same");
  });

  it("returns 'New this week!' when lastWeek is 0 and thisWeek > 0", () => {
    const result = calculateWoWChange(50, 0);
    expect(result.direction).toBe("up");
    expect(result.percentage).toBe(100);
    expect(result.description).toBe("New this week!");
  });

  it("returns 'No change' when both are 0", () => {
    const result = calculateWoWChange(0, 0);
    expect(result.direction).toBe("same");
    expect(result.description).toBe("No change");
  });

  it("returns 'same' for exactly equal values", () => {
    const result = calculateWoWChange(100, 100);
    expect(result.direction).toBe("same");
  });
});


// =============================================================================
// MILESTONE MESSAGES
// =============================================================================

describe("getMilestoneMessage", () => {
  it("returns message for 100 words", () => {
    expect(getMilestoneMessage(100)).toBe("First 100 words!");
  });

  it("returns message for 1000 words", () => {
    expect(getMilestoneMessage(1000)).toBe("1K words milestone!");
  });

  it("returns message within 10% overage", () => {
    expect(getMilestoneMessage(105)).toBe("First 100 words!"); // 5% over
  });

  it("returns null when well past a milestone", () => {
    expect(getMilestoneMessage(200)).toBeNull(); // Past 100, not at 500
  });

  it("returns null for 0 words", () => {
    expect(getMilestoneMessage(0)).toBeNull();
  });

  it("returns message for 1 million words", () => {
    expect(getMilestoneMessage(1000000)).toBe("1 MILLION WORDS!");
  });

  it("returns null when past all milestones", () => {
    expect(getMilestoneMessage(2000000)).toBeNull();
  });

  it("returns 50K milestone message", () => {
    expect(getMilestoneMessage(50000)).toBe("50K words - half a novel!");
  });
});
