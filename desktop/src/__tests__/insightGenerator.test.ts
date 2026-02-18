/**
 * Tests for insightGenerator — personalized narrative insights from user stats.
 *
 * Covers:
 *   - generateInsights: all 12 insight types with threshold conditions
 *   - Priority sorting (highest first) and top-8 cap
 *   - formatHour: 12h format with AM/PM
 *   - calculatePeakPercentage: hourly activity fraction
 *   - getStyleMessage: context-based descriptions
 *   - getSpeedDescription: WPM tier descriptions
 *   - getTopInsight: single highest-priority insight
 *   - getStoryInsights: top-5 slice
 *   - Edge cases: null/empty stats, missing fields
 */
import { describe, it, expect } from "vitest";
import {
  generateInsights,
  getTopInsight,
  getStoryInsights,
} from "../lib/insightGenerator";
import type {
  UserStats,
  DetailedStatsResponse,
  AchievementItem,
} from "../lib/api";


// =============================================================================
// TEST FIXTURES
// =============================================================================

function makeUserStats(overrides: Partial<UserStats> = {}): UserStats {
  return {
    total_transcriptions: 50,
    total_words: 5000,
    total_audio_seconds: 1200,
    transcriptions_today: 3,
    words_today: 500,
    average_words_per_transcription: 100,
    average_words_per_minute: 130,
    time_saved_seconds: 600,
    time_saved_today_seconds: 60,
    typing_wpm: 40,
    ...overrides,
  };
}

function makeDetailed(overrides: Partial<DetailedStatsResponse> = {}): DetailedStatsResponse {
  return {
    total_transcriptions: 50,
    total_words: 5000,
    total_audio_seconds: 1200,
    total_characters: 25000,
    transcriptions_today: 3,
    words_today: 500,
    transcriptions_this_week: 20,
    words_this_week: 2000,
    transcriptions_this_month: 50,
    words_this_month: 5000,
    average_words_per_transcription: 100,
    average_words_per_minute: 130,
    average_transcriptions_per_day: 2,
    average_audio_duration_seconds: 24,
    estimated_time_saved_minutes: 10,
    context_breakdown: [],
    formality_breakdown: [],
    daily_activity: [],
    hourly_activity: [],
    day_of_week_breakdown: [],
    monthly_trends: [],
    word_length_distribution: [],
    current_streak_days: 0,
    longest_streak_days: 0,
    most_productive_day: null,
    most_productive_day_words: 0,
    longest_transcription_words: 200,
    shortest_transcription_words: 10,
    fastest_wpm: 180,
    slowest_wpm: 80,
    growth: {
      words_wow_change: 0,
      words_mom_change: 0,
      transcriptions_wow_change: 0,
      transcriptions_mom_change: 0,
      last_week_words: 1000,
      prev_week_words: 1000,
      last_month_words: 4000,
      prev_month_words: 4000,
    },
    productivity: {
      peak_hour: 14,
      peak_hour_label: "2 PM",
      peak_day: "Monday",
      avg_session_words: 100,
      avg_session_duration_seconds: 60,
      busiest_week_ever: null,
      busiest_week_words: 0,
      efficiency_score: 80,
    },
    achievements: [],
    member_since: "2025-01-01",
    days_as_member: 100,
    total_active_days: 50,
    ...overrides,
  };
}

function makeAchievement(overrides: Partial<AchievementItem> = {}): AchievementItem {
  return {
    id: "test-achievement",
    name: "Word Master",
    description: "Transcribe 10000 words",
    icon: "trophy",
    unlocked: false,
    unlocked_at: null,
    progress: 0.5,
    category: "words",
    threshold: 10000,
    current_value: 5000,
    ...overrides,
  };
}


// =============================================================================
// INSIGHT 1: POWER HOUR (peak_hour)
// =============================================================================

describe("generateInsights — power hour", () => {
  it("generates power hour insight when peak_hour exists", () => {
    const stats = makeUserStats();
    const detailed = makeDetailed();
    const insights = generateInsights(stats, detailed);
    const powerHour = insights.find(i => i.id === "power_hour");
    expect(powerHour).toBeDefined();
    expect(powerHour!.type).toBe("power_hour");
    expect(powerHour!.title).toBe("YOUR POWER HOUR");
    expect(powerHour!.subtitle).toContain("2 PM");
    expect(powerHour!.priority).toBe(8);
  });

  it("uses peak_hour_label when available", () => {
    const detailed = makeDetailed({
      productivity: {
        peak_hour: 9,
        peak_hour_label: "9 AM",
        peak_day: "Tuesday",
        avg_session_words: 100,
        avg_session_duration_seconds: 60,
        busiest_week_ever: null,
        busiest_week_words: 0,
        efficiency_score: 80,
      },
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const powerHour = insights.find(i => i.id === "power_hour");
    expect(powerHour!.subtitle).toContain("9 AM");
  });

  it("includes peak percentage when hourly data available", () => {
    const detailed = makeDetailed({
      hourly_activity: [
        { hour: 14, transcriptions: 10, words: 3000 },
        { hour: 15, transcriptions: 5, words: 1000 },
        { hour: 16, transcriptions: 3, words: 1000 },
      ],
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const powerHour = insights.find(i => i.id === "power_hour");
    // 3000/(3000+1000+1000) = 60%
    expect(powerHour!.description).toContain("60%");
  });

  it("skips power hour when peak_hour is null", () => {
    const detailed = makeDetailed({
      productivity: {
        peak_hour: null as unknown as number,
        peak_hour_label: "",
        peak_day: "Monday",
        avg_session_words: 100,
        avg_session_duration_seconds: 60,
        busiest_week_ever: null,
        busiest_week_words: 0,
        efficiency_score: 80,
      },
    });
    const insights = generateInsights(makeUserStats(), detailed);
    expect(insights.find(i => i.id === "power_hour")).toBeUndefined();
  });

  it("handles hour 0 (midnight) via formatHour", () => {
    const detailed = makeDetailed({
      productivity: {
        peak_hour: 0,
        peak_hour_label: "",
        peak_day: "Monday",
        avg_session_words: 100,
        avg_session_duration_seconds: 60,
        busiest_week_ever: null,
        busiest_week_words: 0,
        efficiency_score: 80,
      },
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const powerHour = insights.find(i => i.id === "power_hour");
    expect(powerHour!.subtitle).toContain("12 AM");
  });

  it("handles hour 12 (noon) via formatHour", () => {
    const detailed = makeDetailed({
      productivity: {
        peak_hour: 12,
        peak_hour_label: "",
        peak_day: "Monday",
        avg_session_words: 100,
        avg_session_duration_seconds: 60,
        busiest_week_ever: null,
        busiest_week_words: 0,
        efficiency_score: 80,
      },
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const powerHour = insights.find(i => i.id === "power_hour");
    expect(powerHour!.subtitle).toContain("12 PM");
  });
});


// =============================================================================
// INSIGHT 2: POWER DAY (peak_day)
// =============================================================================

describe("generateInsights — power day", () => {
  it("generates power day insight when peak_day exists", () => {
    const insights = generateInsights(makeUserStats(), makeDetailed());
    const powerDay = insights.find(i => i.id === "power_day");
    expect(powerDay).toBeDefined();
    expect(powerDay!.title).toBe("YOUR POWER DAY");
    expect(powerDay!.subtitle).toContain("Monday");
    expect(powerDay!.priority).toBe(7);
  });

  it("skips power day when peak_day is empty", () => {
    const detailed = makeDetailed({
      productivity: {
        peak_hour: 14,
        peak_hour_label: "2 PM",
        peak_day: "",
        avg_session_words: 100,
        avg_session_duration_seconds: 60,
        busiest_week_ever: null,
        busiest_week_words: 0,
        efficiency_score: 80,
      },
    });
    const insights = generateInsights(makeUserStats(), detailed);
    expect(insights.find(i => i.id === "power_day")).toBeUndefined();
  });
});


// =============================================================================
// INSIGHT 3: GROWTH (week-over-week)
// =============================================================================

describe("generateInsights — growth", () => {
  it("generates ON FIRE insight for >= 20% WoW growth", () => {
    const detailed = makeDetailed({
      growth: {
        words_wow_change: 25,
        words_mom_change: 10,
        transcriptions_wow_change: 15,
        transcriptions_mom_change: 5,
        last_week_words: 1250,
        prev_week_words: 1000,
        last_month_words: 4000,
        prev_month_words: 4000,
      },
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const growth = insights.find(i => i.id === "growth_wow");
    expect(growth).toBeDefined();
    expect(growth!.title).toBe("ON FIRE");
    expect(growth!.subtitle).toContain("25%");
    expect(growth!.priority).toBe(9);
  });

  it("generates BOUNCE BACK insight for <= -30% WoW decline", () => {
    const detailed = makeDetailed({
      growth: {
        words_wow_change: -35,
        words_mom_change: 0,
        transcriptions_wow_change: 0,
        transcriptions_mom_change: 0,
        last_week_words: 650,
        prev_week_words: 1000,
        last_month_words: 4000,
        prev_month_words: 4000,
      },
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const growth = insights.find(i => i.id === "growth_wow");
    expect(growth).toBeDefined();
    expect(growth!.title).toBe("TIME TO BOUNCE BACK");
    expect(growth!.subtitle).toContain("35%");
    expect(growth!.priority).toBe(6);
  });

  it("skips growth insight for moderate change (-29% to +19%)", () => {
    const detailed = makeDetailed({
      growth: {
        words_wow_change: 10,
        words_mom_change: 0,
        transcriptions_wow_change: 0,
        transcriptions_mom_change: 0,
        last_week_words: 1100,
        prev_week_words: 1000,
        last_month_words: 4000,
        prev_month_words: 4000,
      },
    });
    const insights = generateInsights(makeUserStats(), detailed);
    expect(insights.find(i => i.id === "growth_wow")).toBeUndefined();
  });
});


// =============================================================================
// INSIGHT 4: VOLUME (total words >= 1000)
// =============================================================================

describe("generateInsights — volume", () => {
  it("generates volume insight for >= 1000 words", () => {
    const stats = makeUserStats({ total_words: 1000 });
    const insights = generateInsights(stats, makeDetailed());
    const vol = insights.find(i => i.id === "volume_total");
    expect(vol).toBeDefined();
    expect(vol!.title).toBe("WORD WIZARD");
    expect(vol!.subtitle).toContain("1,000");
  });

  it("skips volume insight for < 1000 words", () => {
    const stats = makeUserStats({ total_words: 999 });
    const insights = generateInsights(stats, makeDetailed());
    expect(insights.find(i => i.id === "volume_total")).toBeUndefined();
  });
});


// =============================================================================
// INSIGHT 5: STYLE (context breakdown >= 40%)
// =============================================================================

describe("generateInsights — style", () => {
  it("generates style insight when top context >= 40%", () => {
    const detailed = makeDetailed({
      context_breakdown: [
        { context: "meeting notes", count: 30, words: 3000, percentage: 60 },
        { context: "email", count: 20, words: 2000, percentage: 40 },
      ],
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const style = insights.find(i => i.id === "style_context");
    expect(style).toBeDefined();
    expect(style!.subtitle).toContain("60%");
    expect(style!.subtitle).toContain("meeting notes");
  });

  it("uses meeting style message for meeting context", () => {
    const detailed = makeDetailed({
      context_breakdown: [
        { context: "meeting notes", count: 30, words: 3000, percentage: 60 },
        { context: "other", count: 20, words: 2000, percentage: 40 },
      ],
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const style = insights.find(i => i.id === "style_context");
    expect(style!.description).toBe("You're all business!");
  });

  it("uses email style message for email context", () => {
    const detailed = makeDetailed({
      context_breakdown: [
        { context: "email drafts", count: 30, words: 3000, percentage: 60 },
        { context: "other", count: 20, words: 2000, percentage: 40 },
      ],
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const style = insights.find(i => i.id === "style_context");
    expect(style!.description).toBe("You're a communication pro!");
  });

  it("uses creative style message for creative context", () => {
    const detailed = makeDetailed({
      context_breakdown: [
        { context: "creative writing", count: 30, words: 3000, percentage: 60 },
        { context: "other", count: 20, words: 2000, percentage: 40 },
      ],
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const style = insights.find(i => i.id === "style_context");
    expect(style!.description).toBe("Let that creativity flow!");
  });

  it("uses casual style message for casual context", () => {
    const detailed = makeDetailed({
      context_breakdown: [
        { context: "casual chat", count: 30, words: 3000, percentage: 60 },
        { context: "other", count: 20, words: 2000, percentage: 40 },
      ],
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const style = insights.find(i => i.id === "style_context");
    expect(style!.description).toBe("Keeping it casual - nice!");
  });

  it("uses formal style message for formal context", () => {
    const detailed = makeDetailed({
      context_breakdown: [
        { context: "formal report", count: 30, words: 3000, percentage: 60 },
        { context: "other", count: 20, words: 2000, percentage: 40 },
      ],
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const style = insights.find(i => i.id === "style_context");
    expect(style!.description).toBe("Professional and polished!");
  });

  it("uses generic high-percentage message for unknown context >= 60%", () => {
    const detailed = makeDetailed({
      context_breakdown: [
        { context: "dictation", count: 40, words: 4000, percentage: 80 },
        { context: "other", count: 10, words: 1000, percentage: 20 },
      ],
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const style = insights.find(i => i.id === "style_context");
    // 40/(40+10) = 80% >= 60%
    expect(style!.description).toBe("You've found your groove!");
  });

  it("uses generic message for unknown context 40-59%", () => {
    const detailed = makeDetailed({
      context_breakdown: [
        { context: "dictation", count: 21, words: 2100, percentage: 42 },
        { context: "other", count: 29, words: 2900, percentage: 58 },
      ],
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const style = insights.find(i => i.id === "style_context");
    expect(style!.description).toBe("You know what works for you!");
  });

  it("skips style insight when top context < 40%", () => {
    const detailed = makeDetailed({
      context_breakdown: [
        { context: "meeting", count: 3, words: 300, percentage: 30 },
        { context: "email", count: 3, words: 300, percentage: 30 },
        { context: "other", count: 4, words: 400, percentage: 40 },
      ],
    });
    const insights = generateInsights(makeUserStats(), detailed);
    // Top context is "other" at 4/(3+3+4) = 40% — exactly 40, should pass
    // "meeting" is 3/10 = 30%, email is 30% — first entry is "meeting" at 30%
    expect(insights.find(i => i.id === "style_context")).toBeUndefined();
  });
});


// =============================================================================
// INSIGHT 6: STREAK (>= 3 days)
// =============================================================================

describe("generateInsights — streak", () => {
  it("generates streak insight for >= 3 day streak", () => {
    const detailed = makeDetailed({ current_streak_days: 5, longest_streak_days: 10 });
    const insights = generateInsights(makeUserStats(), detailed);
    const streak = insights.find(i => i.id === "streak_current");
    expect(streak).toBeDefined();
    expect(streak!.subtitle).toBe("5 day streak!");
  });

  it("shows ALMOST THERE when close to milestone (within 3 days)", () => {
    const detailed = makeDetailed({ current_streak_days: 5, longest_streak_days: 10 });
    const insights = generateInsights(makeUserStats(), detailed);
    const streak = insights.find(i => i.id === "streak_current");
    // 5 days: next milestone is 7, remaining = 2 (within 3)
    expect(streak!.title).toBe("ALMOST THERE");
    expect(streak!.description).toContain("2 more days");
  });

  it("shows STREAK MASTER when not close to milestone", () => {
    const detailed = makeDetailed({ current_streak_days: 8, longest_streak_days: 20 });
    const insights = generateInsights(makeUserStats(), detailed);
    const streak = insights.find(i => i.id === "streak_current");
    // 8 days: next milestone is 14, remaining = 6 (> 3)
    expect(streak!.title).toBe("STREAK MASTER");
    expect(streak!.description).toContain("record is 20 days");
  });

  it("shows longest streak ever message when on record streak", () => {
    const detailed = makeDetailed({ current_streak_days: 20, longest_streak_days: 20 });
    const insights = generateInsights(makeUserStats(), detailed);
    const streak = insights.find(i => i.id === "streak_current");
    expect(streak!.description).toBe("You're on your longest streak ever!");
  });

  it("has priority 10 for >= 7 day streak", () => {
    const detailed = makeDetailed({ current_streak_days: 7, longest_streak_days: 10 });
    const insights = generateInsights(makeUserStats(), detailed);
    const streak = insights.find(i => i.id === "streak_current");
    expect(streak!.priority).toBe(10);
  });

  it("has priority 8 for < 7 day streak", () => {
    const detailed = makeDetailed({ current_streak_days: 4, longest_streak_days: 10 });
    const insights = generateInsights(makeUserStats(), detailed);
    const streak = insights.find(i => i.id === "streak_current");
    expect(streak!.priority).toBe(8);
  });

  it("skips streak insight for < 3 days", () => {
    const detailed = makeDetailed({ current_streak_days: 2 });
    const insights = generateInsights(makeUserStats(), detailed);
    expect(insights.find(i => i.id === "streak_current")).toBeUndefined();
  });
});


// =============================================================================
// INSIGHT 7: TIME SAVED (>= 300s = 5 minutes)
// =============================================================================

describe("generateInsights — time saved", () => {
  it("generates time saved insight for >= 300 seconds", () => {
    const stats = makeUserStats({ time_saved_seconds: 600 });
    const insights = generateInsights(stats, makeDetailed());
    const ts = insights.find(i => i.id === "time_saved");
    expect(ts).toBeDefined();
    expect(ts!.title).toBe("TIME RECLAIMED");
    expect(ts!.subtitle).toContain("10 minutes");
  });

  it("formats hours and minutes for large values", () => {
    const stats = makeUserStats({ time_saved_seconds: 7500 }); // 2h 5m
    const insights = generateInsights(stats, makeDetailed());
    const ts = insights.find(i => i.id === "time_saved");
    expect(ts!.subtitle).toContain("2h 5m");
  });

  it("skips time saved for < 300 seconds", () => {
    const stats = makeUserStats({ time_saved_seconds: 299 });
    const insights = generateInsights(stats, makeDetailed());
    expect(insights.find(i => i.id === "time_saved")).toBeUndefined();
  });
});


// =============================================================================
// INSIGHT 8: PERSONAL RECORD (best day >= 500 words)
// =============================================================================

describe("generateInsights — personal record", () => {
  it("generates record insight for best day >= 500 words", () => {
    const detailed = makeDetailed({
      most_productive_day: "2025-06-15",
      most_productive_day_words: 2000,
    });
    const insights = generateInsights(makeUserStats(), detailed);
    const record = insights.find(i => i.id === "record_best_day");
    expect(record).toBeDefined();
    expect(record!.title).toBe("PERSONAL RECORD");
    expect(record!.subtitle).toContain("2,000");
    // Date display depends on local timezone — just verify it includes "Jun"
    expect(record!.description).toMatch(/Jun \d+/);
  });

  it("skips record for < 500 words best day", () => {
    const detailed = makeDetailed({
      most_productive_day: "2025-06-15",
      most_productive_day_words: 499,
    });
    const insights = generateInsights(makeUserStats(), detailed);
    expect(insights.find(i => i.id === "record_best_day")).toBeUndefined();
  });

  it("skips record when most_productive_day is null", () => {
    const detailed = makeDetailed({
      most_productive_day: null,
      most_productive_day_words: 1000,
    });
    const insights = generateInsights(makeUserStats(), detailed);
    expect(insights.find(i => i.id === "record_best_day")).toBeUndefined();
  });
});


// =============================================================================
// INSIGHT 9: AUDIO TIME (>= 600s = 10 minutes)
// =============================================================================

describe("generateInsights — audio time", () => {
  it("generates audio insight for >= 600 seconds", () => {
    const stats = makeUserStats({ total_audio_seconds: 900 }); // 15 min
    const insights = generateInsights(stats, makeDetailed());
    const audio = insights.find(i => i.id === "audio_time");
    expect(audio).toBeDefined();
    expect(audio!.title).toBe("VOICE POWER");
    expect(audio!.subtitle).toContain("15 minutes");
  });

  it("skips audio insight for < 600 seconds", () => {
    const stats = makeUserStats({ total_audio_seconds: 599 });
    const insights = generateInsights(stats, makeDetailed());
    expect(insights.find(i => i.id === "audio_time")).toBeUndefined();
  });
});


// =============================================================================
// INSIGHT 10: ACHIEVEMENT CLOSE TO UNLOCK (>= 75% progress)
// =============================================================================

describe("generateInsights — achievement close", () => {
  it("generates close-to-unlock insight for >= 75% progress", () => {
    const achievements: AchievementItem[] = [
      makeAchievement({ progress: 0.8, threshold: 10000, current_value: 8000, name: "Word Master" }),
    ];
    const insights = generateInsights(makeUserStats(), makeDetailed(), achievements);
    const close = insights.find(i => i.id === "achievement_close");
    expect(close).toBeDefined();
    expect(close!.title).toBe("SO CLOSE");
    expect(close!.subtitle).toContain("80%");
    expect(close!.subtitle).toContain("Word Master");
    expect(close!.description).toContain("2,000");
  });

  it("picks the closest-to-completion achievement", () => {
    const achievements: AchievementItem[] = [
      makeAchievement({ id: "a1", progress: 0.8, threshold: 10000, current_value: 8000, name: "Lesser" }),
      makeAchievement({ id: "a2", progress: 0.95, threshold: 1000, current_value: 950, name: "Almost" }),
    ];
    const insights = generateInsights(makeUserStats(), makeDetailed(), achievements);
    const close = insights.find(i => i.id === "achievement_close");
    expect(close!.subtitle).toContain("Almost");
    expect(close!.subtitle).toContain("95%");
  });

  it("ignores already unlocked achievements", () => {
    const achievements: AchievementItem[] = [
      makeAchievement({ progress: 1.0, unlocked: true }),
    ];
    const insights = generateInsights(makeUserStats(), makeDetailed(), achievements);
    expect(insights.find(i => i.id === "achievement_close")).toBeUndefined();
  });

  it("ignores achievements < 75% progress", () => {
    const achievements: AchievementItem[] = [
      makeAchievement({ progress: 0.5 }),
    ];
    const insights = generateInsights(makeUserStats(), makeDetailed(), achievements);
    expect(insights.find(i => i.id === "achievement_close")).toBeUndefined();
  });

  it("skips when no achievements provided", () => {
    const insights = generateInsights(makeUserStats(), makeDetailed());
    expect(insights.find(i => i.id === "achievement_close")).toBeUndefined();
  });
});


// =============================================================================
// INSIGHT 11: DAILY AVERAGE (>= 100 words/day)
// =============================================================================

describe("generateInsights — daily average", () => {
  it("generates daily average for >= 100 words/day", () => {
    const detailed = makeDetailed({ total_words: 5000, total_active_days: 50 }); // 100/day
    const insights = generateInsights(makeUserStats(), detailed);
    const avg = insights.find(i => i.id === "daily_average");
    expect(avg).toBeDefined();
    expect(avg!.title).toBe("STEADY PROGRESS");
    expect(avg!.subtitle).toContain("100");
    expect(avg!.priority).toBe(4);
  });

  it("skips daily average for < 100 words/day", () => {
    const detailed = makeDetailed({ total_words: 4900, total_active_days: 50 }); // 98/day
    const insights = generateInsights(makeUserStats(), detailed);
    expect(insights.find(i => i.id === "daily_average")).toBeUndefined();
  });

  it("skips daily average when total_active_days is 0", () => {
    const detailed = makeDetailed({ total_active_days: 0 });
    const insights = generateInsights(makeUserStats(), detailed);
    expect(insights.find(i => i.id === "daily_average")).toBeUndefined();
  });
});


// =============================================================================
// INSIGHT 12: SPEAKING SPEED (>= 120 WPM)
// =============================================================================

describe("generateInsights — speaking speed", () => {
  it("generates speed insight for >= 120 WPM", () => {
    const stats = makeUserStats({ average_words_per_minute: 130 });
    const insights = generateInsights(stats, makeDetailed());
    const speed = insights.find(i => i.id === "speaking_speed");
    expect(speed).toBeDefined();
    expect(speed!.title).toBe("SPEED DEMON");
    expect(speed!.subtitle).toContain("130 WPM");
  });

  it("returns auctioneer description for >= 180 WPM", () => {
    const stats = makeUserStats({ average_words_per_minute: 200 });
    const insights = generateInsights(stats, makeDetailed());
    const speed = insights.find(i => i.id === "speaking_speed");
    expect(speed!.description).toBe("You speak faster than most auctioneers!");
  });

  it("returns rapid-fire description for 160-179 WPM", () => {
    const stats = makeUserStats({ average_words_per_minute: 165 });
    const insights = generateInsights(stats, makeDetailed());
    const speed = insights.find(i => i.id === "speaking_speed");
    expect(speed!.description).toBe("You're a rapid-fire communicator!");
  });

  it("returns brisk description for 140-159 WPM", () => {
    const stats = makeUserStats({ average_words_per_minute: 145 });
    const insights = generateInsights(stats, makeDetailed());
    const speed = insights.find(i => i.id === "speaking_speed");
    expect(speed!.description).toBe("You've got a brisk speaking pace!");
  });

  it("returns purpose description for 120-139 WPM", () => {
    const stats = makeUserStats({ average_words_per_minute: 125 });
    const insights = generateInsights(stats, makeDetailed());
    const speed = insights.find(i => i.id === "speaking_speed");
    expect(speed!.description).toBe("You speak with purpose and clarity!");
  });

  it("skips speed insight for < 120 WPM", () => {
    const stats = makeUserStats({ average_words_per_minute: 119 });
    const insights = generateInsights(stats, makeDetailed());
    expect(insights.find(i => i.id === "speaking_speed")).toBeUndefined();
  });
});


// =============================================================================
// SORTING AND CAPPING
// =============================================================================

describe("generateInsights — sorting and capping", () => {
  it("returns insights sorted by priority (highest first)", () => {
    const stats = makeUserStats({
      total_words: 5000,
      time_saved_seconds: 600,
      average_words_per_minute: 130,
      total_audio_seconds: 900,
    });
    const detailed = makeDetailed({
      current_streak_days: 10,
      longest_streak_days: 10,
    });
    const insights = generateInsights(stats, detailed);
    for (let i = 1; i < insights.length; i++) {
      expect(insights[i - 1].priority).toBeGreaterThanOrEqual(insights[i].priority);
    }
  });

  it("caps at 8 insights maximum", () => {
    // Build stats that trigger all 12 insights
    const stats = makeUserStats({
      total_words: 5000,
      time_saved_seconds: 3600,
      average_words_per_minute: 200,
      total_audio_seconds: 1800,
    });
    const detailed = makeDetailed({
      current_streak_days: 5,
      longest_streak_days: 10,
      most_productive_day: "2025-06-15",
      most_productive_day_words: 2000,
      total_active_days: 50,
      total_words: 5000,
      context_breakdown: [
        { context: "meeting notes", count: 30, words: 3000, percentage: 60 },
        { context: "other", count: 20, words: 2000, percentage: 40 },
      ],
      growth: {
        words_wow_change: 50,
        words_mom_change: 10,
        transcriptions_wow_change: 15,
        transcriptions_mom_change: 5,
        last_week_words: 1500,
        prev_week_words: 1000,
        last_month_words: 4000,
        prev_month_words: 4000,
      },
      hourly_activity: [
        { hour: 14, transcriptions: 10, words: 5000 },
      ],
    });
    const achievements: AchievementItem[] = [
      makeAchievement({ progress: 0.9, threshold: 10000, current_value: 9000 }),
    ];
    const insights = generateInsights(stats, detailed, achievements);
    expect(insights.length).toBeLessThanOrEqual(8);
  });

  it("every insight has required fields", () => {
    const insights = generateInsights(makeUserStats(), makeDetailed());
    for (const insight of insights) {
      expect(insight.id).toBeTruthy();
      expect(insight.type).toBeTruthy();
      expect(insight.title).toBeTruthy();
      expect(insight.subtitle).toBeTruthy();
      expect(insight.description).toBeTruthy();
      expect(insight.icon).toBeTruthy();
      expect(insight.gradient).toBeTruthy();
      expect(typeof insight.priority).toBe("number");
    }
  });
});


// =============================================================================
// EDGE CASES
// =============================================================================

describe("generateInsights — edge cases", () => {
  it("returns empty array with null detailed stats", () => {
    const stats = makeUserStats({ total_words: 0, time_saved_seconds: 0, average_words_per_minute: 0, total_audio_seconds: 0 });
    const insights = generateInsights(stats, null);
    // Only non-detailed insights could fire, but all thresholds are 0
    expect(insights).toEqual([]);
  });

  it("handles minimal stats gracefully (no insights triggered)", () => {
    const stats = makeUserStats({
      total_words: 10,
      time_saved_seconds: 0,
      average_words_per_minute: 50,
      total_audio_seconds: 0,
    });
    const detailed = makeDetailed({
      current_streak_days: 0,
      total_active_days: 0,
      most_productive_day: null,
      most_productive_day_words: 0,
      context_breakdown: [],
      productivity: {
        peak_hour: null as unknown as number,
        peak_hour_label: "",
        peak_day: "",
        avg_session_words: 0,
        avg_session_duration_seconds: 0,
        busiest_week_ever: null,
        busiest_week_words: 0,
        efficiency_score: 0,
      },
      growth: {
        words_wow_change: 0,
        words_mom_change: 0,
        transcriptions_wow_change: 0,
        transcriptions_mom_change: 0,
        last_week_words: 0,
        prev_week_words: 0,
        last_month_words: 0,
        prev_month_words: 0,
      },
    });
    const insights = generateInsights(stats, detailed);
    expect(insights.length).toBe(0);
  });
});


// =============================================================================
// getTopInsight
// =============================================================================

describe("getTopInsight", () => {
  it("returns the highest priority insight", () => {
    const stats = makeUserStats();
    const detailed = makeDetailed({ current_streak_days: 10, longest_streak_days: 10 });
    const top = getTopInsight(stats, detailed);
    expect(top).not.toBeNull();
    // Streak >= 7 days gets priority 10 — should be #1
    expect(top!.id).toBe("streak_current");
  });

  it("returns null when no insights generated", () => {
    const stats = makeUserStats({
      total_words: 0,
      time_saved_seconds: 0,
      average_words_per_minute: 0,
      total_audio_seconds: 0,
    });
    const top = getTopInsight(stats, null);
    expect(top).toBeNull();
  });
});


// =============================================================================
// getStoryInsights
// =============================================================================

describe("getStoryInsights", () => {
  it("returns at most 5 insights", () => {
    const stats = makeUserStats({
      total_words: 5000,
      time_saved_seconds: 3600,
      average_words_per_minute: 200,
      total_audio_seconds: 1800,
    });
    const detailed = makeDetailed({
      current_streak_days: 10,
      longest_streak_days: 10,
      most_productive_day: "2025-06-15",
      most_productive_day_words: 2000,
      total_active_days: 50,
      total_words: 5000,
      growth: {
        words_wow_change: 50,
        words_mom_change: 10,
        transcriptions_wow_change: 15,
        transcriptions_mom_change: 5,
        last_week_words: 1500,
        prev_week_words: 1000,
        last_month_words: 4000,
        prev_month_words: 4000,
      },
    });
    const story = getStoryInsights(stats, detailed);
    expect(story.length).toBeLessThanOrEqual(5);
    // Should also be sorted by priority
    for (let i = 1; i < story.length; i++) {
      expect(story[i - 1].priority).toBeGreaterThanOrEqual(story[i].priority);
    }
  });

  it("returns empty array when no insights", () => {
    const stats = makeUserStats({
      total_words: 0,
      time_saved_seconds: 0,
      average_words_per_minute: 0,
      total_audio_seconds: 0,
    });
    const story = getStoryInsights(stats, null);
    expect(story).toEqual([]);
  });
});
