/**
 * Insight Generator
 * Generates personalized narrative insights from user statistics
 * Inspired by Spotify Wrapped and Duolingo-style engagement
 */

import { DetailedStatsResponse, UserStats, AchievementItem } from './api';
import {
  getWordEquivalent,
  getTimeSavedEquivalent,
  getAudioEquivalent,
  getStreakMilestone,
} from './statsEquivalents';

export interface Insight {
  id: string;
  type: 'power_hour' | 'growth' | 'volume' | 'style' | 'streak' | 'time_saved' | 'milestone' | 'record';
  title: string;
  subtitle: string;
  description: string;
  icon: string;
  gradient: string;
  priority: number; // Higher = more important
  value?: string | number;
}

// Gradient presets for different insight types
const GRADIENTS = {
  power_hour: 'linear-gradient(135deg, #6366f1 0%, #8b5cf6 100%)',
  growth: 'linear-gradient(135deg, #22c55e 0%, #10b981 100%)',
  volume: 'linear-gradient(135deg, #f59e0b 0%, #f97316 100%)',
  style: 'linear-gradient(135deg, #ec4899 0%, #f43f5e 100%)',
  streak: 'linear-gradient(135deg, #ef4444 0%, #f97316 100%)',
  time_saved: 'linear-gradient(135deg, #06b6d4 0%, #3b82f6 100%)',
  milestone: 'linear-gradient(135deg, #fbbf24 0%, #f59e0b 100%)',
  record: 'linear-gradient(135deg, #a855f7 0%, #6366f1 100%)',
};

/**
 * Generate all applicable insights from user stats
 */
export function generateInsights(
  stats: UserStats,
  detailed: DetailedStatsResponse | null,
  achievements?: AchievementItem[]
): Insight[] {
  const insights: Insight[] = [];

  // 1. Peak Productivity Time (from productivity insights)
  if (detailed?.productivity?.peak_hour !== null && detailed?.productivity?.peak_hour !== undefined) {
    const hourStr = detailed.productivity.peak_hour_label || formatHour(detailed.productivity.peak_hour);
    const percentage = calculatePeakPercentage(detailed);
    insights.push({
      id: 'power_hour',
      type: 'power_hour',
      title: 'YOUR POWER HOUR',
      subtitle: `You're most productive at ${hourStr}`,
      description: percentage
        ? `That's when ${percentage}% of your words happen`
        : 'This is when you do your best work',
      icon: '⚡',
      gradient: GRADIENTS.power_hour,
      priority: 8,
      value: hourStr,
    });
  }

  // 2. Peak Day of Week (from productivity insights)
  if (detailed?.productivity?.peak_day) {
    insights.push({
      id: 'power_day',
      type: 'power_hour',
      title: 'YOUR POWER DAY',
      subtitle: `${detailed.productivity.peak_day}s are your most productive day`,
      description: 'You consistently get more done on this day',
      icon: '📅',
      gradient: GRADIENTS.power_hour,
      priority: 7,
      value: detailed.productivity.peak_day,
    });
  }

  // 3. Week-over-Week Growth (from growth metrics)
  if (detailed?.growth) {
    const wowChange = detailed.growth.words_wow_change;
    if (wowChange >= 20) {
      insights.push({
        id: 'growth_wow',
        type: 'growth',
        title: 'ON FIRE',
        subtitle: `${Math.round(wowChange)}% more words this week vs last!`,
        description: 'Keep the momentum going',
        icon: '📈',
        gradient: GRADIENTS.growth,
        priority: 9,
        value: `+${Math.round(wowChange)}%`,
      });
    } else if (wowChange <= -30) {
      insights.push({
        id: 'growth_wow',
        type: 'growth',
        title: 'TIME TO BOUNCE BACK',
        subtitle: `Activity is down ${Math.round(Math.abs(wowChange))}% this week`,
        description: 'A quick transcription will get you back on track',
        icon: '💪',
        gradient: 'linear-gradient(135deg, #64748b 0%, #475569 100%)',
        priority: 6,
        value: `${Math.round(wowChange)}%`,
      });
    }
  }

  // 4. Total Volume Achievement
  const wordEquiv = getWordEquivalent(stats.total_words);
  if (stats.total_words >= 1000) {
    insights.push({
      id: 'volume_total',
      type: 'volume',
      title: 'WORD WIZARD',
      subtitle: `You've spoken ${stats.total_words.toLocaleString()} words`,
      description: `That's like writing ${wordEquiv.description}`,
      icon: '📚',
      gradient: GRADIENTS.volume,
      priority: 7,
      value: stats.total_words,
    });
  }

  // 5. Context/Style Insight
  if (detailed?.context_breakdown && detailed.context_breakdown.length > 0) {
    const topContext = detailed.context_breakdown[0];
    const total = detailed.context_breakdown.reduce((sum, c) => sum + c.count, 0);
    const percentage = Math.round((topContext.count / total) * 100);

    if (percentage >= 40) {
      const styleMessage = getStyleMessage(topContext.context, percentage);
      insights.push({
        id: 'style_context',
        type: 'style',
        title: 'YOUR STYLE',
        subtitle: `${percentage}% of your transcriptions are ${topContext.context}`,
        description: styleMessage,
        icon: '🎯',
        gradient: GRADIENTS.style,
        priority: 6,
        value: topContext.context,
      });
    }
  }

  // 6. Streak Celebration
  if (detailed?.current_streak_days && detailed.current_streak_days >= 3) {
    const streakMilestone = getStreakMilestone(detailed.current_streak_days);
    const isCloseToMilestone = streakMilestone && streakMilestone.remaining <= 3;

    insights.push({
      id: 'streak_current',
      type: 'streak',
      title: isCloseToMilestone ? 'ALMOST THERE' : 'STREAK MASTER',
      subtitle: `${detailed.current_streak_days} day streak!`,
      description: isCloseToMilestone
        ? `Just ${streakMilestone!.remaining} more day${streakMilestone!.remaining !== 1 ? 's' : ''} to ${streakMilestone!.description}`
        : detailed.longest_streak_days > detailed.current_streak_days
          ? `Your record is ${detailed.longest_streak_days} days - can you beat it?`
          : "You're on your longest streak ever!",
      icon: '🔥',
      gradient: GRADIENTS.streak,
      priority: detailed.current_streak_days >= 7 ? 10 : 8,
      value: detailed.current_streak_days,
    });
  }

  // 7. Time Saved Insight
  if (stats.time_saved_seconds >= 300) { // At least 5 minutes
    const timeEquiv = getTimeSavedEquivalent(stats.time_saved_seconds);
    const hours = Math.floor(stats.time_saved_seconds / 3600);
    const mins = Math.floor((stats.time_saved_seconds % 3600) / 60);
    const timeStr = hours > 0 ? `${hours}h ${mins}m` : `${mins} minutes`;

    insights.push({
      id: 'time_saved',
      type: 'time_saved',
      title: 'TIME RECLAIMED',
      subtitle: `You've saved ${timeStr} of typing`,
      description: `That's enough time for ${timeEquiv.description}`,
      icon: '⏰',
      gradient: GRADIENTS.time_saved,
      priority: 8,
      value: stats.time_saved_seconds,
    });
  }

  // 8. Personal Record - Best Day
  if (detailed?.most_productive_day_words && detailed.most_productive_day_words >= 500 && detailed.most_productive_day) {
    const date = new Date(detailed.most_productive_day);
    const dateStr = date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
    insights.push({
      id: 'record_best_day',
      type: 'record',
      title: 'PERSONAL RECORD',
      subtitle: `${detailed.most_productive_day_words.toLocaleString()} words in a single day`,
      description: `Your best day was ${dateStr}`,
      icon: '🏆',
      gradient: GRADIENTS.record,
      priority: 6,
      value: detailed.most_productive_day_words,
    });
  }

  // 9. Audio Time Achievement
  if (stats.total_audio_seconds >= 600) { // At least 10 minutes
    const audioMinutes = stats.total_audio_seconds / 60;
    const audioEquiv = getAudioEquivalent(audioMinutes);
    insights.push({
      id: 'audio_time',
      type: 'volume',
      title: 'VOICE POWER',
      subtitle: `${Math.round(audioMinutes)} minutes of audio recorded`,
      description: `That's equivalent to ${audioEquiv.description}`,
      icon: '🎤',
      gradient: GRADIENTS.volume,
      priority: 5,
      value: audioMinutes,
    });
  }

  // 10. Achievement Close to Unlock
  if (achievements) {
    const closeToUnlock = achievements.filter(
      a => !a.unlocked && a.progress >= 0.75 && a.progress < 1
    );
    if (closeToUnlock.length > 0) {
      const closest = closeToUnlock.sort((a, b) => b.progress - a.progress)[0];
      const remaining = closest.threshold - closest.current_value;
      insights.push({
        id: 'achievement_close',
        type: 'milestone',
        title: 'SO CLOSE',
        subtitle: `${Math.round(closest.progress * 100)}% to "${closest.name}"`,
        description: `Just ${remaining.toLocaleString()} more to unlock!`,
        icon: '🎖️',
        gradient: GRADIENTS.milestone,
        priority: 9,
        value: closest.progress,
      });
    }
  }

  // 11. Daily Average
  if (detailed?.total_active_days && detailed.total_active_days > 0) {
    const avgWordsPerDay = Math.round(detailed.total_words / detailed.total_active_days);
    if (avgWordsPerDay >= 100) {
      insights.push({
        id: 'daily_average',
        type: 'growth',
        title: 'STEADY PROGRESS',
        subtitle: `${avgWordsPerDay.toLocaleString()} words per active day`,
        description: 'Consistency is key to success',
        icon: '📊',
        gradient: GRADIENTS.growth,
        priority: 4,
        value: avgWordsPerDay,
      });
    }
  }

  // 12. Speaking Speed Insight
  if (stats.average_words_per_minute >= 120) {
    const speedDescription = getSpeedDescription(stats.average_words_per_minute);
    insights.push({
      id: 'speaking_speed',
      type: 'style',
      title: 'SPEED DEMON',
      subtitle: `${Math.round(stats.average_words_per_minute)} WPM average`,
      description: speedDescription,
      icon: '⚡',
      gradient: GRADIENTS.style,
      priority: 4,
      value: stats.average_words_per_minute,
    });
  }

  // Sort by priority (highest first) and return top 8
  return insights
    .sort((a, b) => b.priority - a.priority)
    .slice(0, 8);
}

/**
 * Format hour to readable string
 */
function formatHour(hour: number): string {
  if (hour === 0) return '12 AM';
  if (hour === 12) return '12 PM';
  if (hour < 12) return `${hour} AM`;
  return `${hour - 12} PM`;
}

/**
 * Calculate what percentage of words happen during peak hour
 */
function calculatePeakPercentage(detailed: DetailedStatsResponse): number | null {
  if (!detailed.hourly_activity || detailed.hourly_activity.length === 0) {
    return null;
  }

  const totalWords = detailed.hourly_activity.reduce((sum, h) => sum + h.words, 0);
  if (totalWords === 0) return null;

  const peakHourData = detailed.hourly_activity.find(h => h.hour === detailed.productivity?.peak_hour);
  if (!peakHourData) return null;

  return Math.round((peakHourData.words / totalWords) * 100);
}

/**
 * Get a fun description based on context preference
 */
function getStyleMessage(context: string, percentage: number): string {
  const contextLower = context.toLowerCase();

  if (contextLower.includes('meeting') || contextLower.includes('notes')) {
    return "You're all business!";
  }
  if (contextLower.includes('email') || contextLower.includes('message')) {
    return "You're a communication pro!";
  }
  if (contextLower.includes('creative') || contextLower.includes('story')) {
    return "Let that creativity flow!";
  }
  if (contextLower.includes('casual') || contextLower.includes('personal')) {
    return "Keeping it casual - nice!";
  }
  if (contextLower.includes('formal') || contextLower.includes('professional')) {
    return "Professional and polished!";
  }

  return percentage >= 60
    ? "You've found your groove!"
    : "You know what works for you!";
}

/**
 * Get description for speaking speed
 */
function getSpeedDescription(wpm: number): string {
  if (wpm >= 180) return "You speak faster than most auctioneers!";
  if (wpm >= 160) return "You're a rapid-fire communicator!";
  if (wpm >= 140) return "You've got a brisk speaking pace!";
  return "You speak with purpose and clarity!";
}

/**
 * Generate a single highlight insight for the overview
 */
export function getTopInsight(
  stats: UserStats,
  detailed: DetailedStatsResponse | null,
  achievements?: AchievementItem[]
): Insight | null {
  const insights = generateInsights(stats, detailed, achievements);
  return insights.length > 0 ? insights[0] : null;
}

/**
 * Get insights for "Your Story" narrative cards
 */
export function getStoryInsights(
  stats: UserStats,
  detailed: DetailedStatsResponse | null,
  achievements?: AchievementItem[]
): Insight[] {
  // Return top 5 insights for the story view
  return generateInsights(stats, detailed, achievements).slice(0, 5);
}
