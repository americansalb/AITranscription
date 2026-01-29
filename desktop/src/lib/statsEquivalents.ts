/**
 * Stats Equivalents Calculator
 * Transforms raw statistics into relatable real-world equivalents
 */

// Average book = 80,000 words
const WORDS_PER_BOOK = 80000;
const WORDS_PER_BLOG_POST = 1500;
const WORDS_PER_EMAIL = 200;
const WORDS_PER_TWEET = 50;
const WORDS_PER_SHORT_STORY = 7500;

// Audio durations (in minutes)
const MINUTES_PER_SONG = 3.5;
const MINUTES_PER_PODCAST = 45;
const MINUTES_PER_AUDIOBOOK_HOUR = 60;

export interface WordEquivalent {
  count: number;
  unit: string;
  description: string;
  icon: string;
}

export interface TimeSavedEquivalent {
  description: string;
  icon: string;
}

export interface AudioEquivalent {
  count: number;
  unit: string;
  description: string;
  icon: string;
}

/**
 * Convert word count to a relatable equivalent
 */
export function getWordEquivalent(words: number): WordEquivalent {
  if (words < 500) {
    const tweets = Math.round(words / WORDS_PER_TWEET);
    return {
      count: Math.max(1, tweets),
      unit: tweets === 1 ? 'tweet' : 'tweets',
      description: `${Math.max(1, tweets)} tweet${tweets !== 1 ? 's' : ''} worth`,
      icon: '🐦',
    };
  }

  if (words < 2000) {
    const emails = Math.round(words / WORDS_PER_EMAIL);
    return {
      count: Math.max(1, emails),
      unit: emails === 1 ? 'email' : 'emails',
      description: `${Math.max(1, emails)} email${emails !== 1 ? 's' : ''} worth`,
      icon: '📧',
    };
  }

  if (words < 10000) {
    const posts = Math.round(words / WORDS_PER_BLOG_POST * 10) / 10;
    return {
      count: posts,
      unit: posts === 1 ? 'blog post' : 'blog posts',
      description: `Like writing ${posts} blog post${posts !== 1 ? 's' : ''}`,
      icon: '📝',
    };
  }

  if (words < 50000) {
    const stories = Math.round(words / WORDS_PER_SHORT_STORY * 10) / 10;
    return {
      count: stories,
      unit: stories === 1 ? 'short story' : 'short stories',
      description: `Like writing ${stories} short stor${stories !== 1 ? 'ies' : 'y'}`,
      icon: '📖',
    };
  }

  const novels = Math.round(words / WORDS_PER_BOOK * 10) / 10;
  return {
    count: novels,
    unit: novels === 1 ? 'novel' : 'novels',
    description: `Like writing ${novels} novel${novels !== 1 ? 's' : ''}`,
    icon: '📚',
  };
}

/**
 * Get a brief word equivalent string
 */
export function getWordEquivalentBrief(words: number): string {
  const equiv = getWordEquivalent(words);
  return `${equiv.count} ${equiv.unit}`;
}

/**
 * Convert time saved (seconds) to a relatable equivalent
 */
export function getTimeSavedEquivalent(seconds: number): TimeSavedEquivalent {
  const minutes = seconds / 60;
  const hours = minutes / 60;

  if (minutes < 5) {
    return {
      description: 'a coffee break',
      icon: '☕',
    };
  }

  if (minutes < 30) {
    return {
      description: 'a lunch break',
      icon: '🥪',
    };
  }

  if (minutes < 60) {
    return {
      description: 'a workout session',
      icon: '💪',
    };
  }

  if (hours < 4) {
    return {
      description: 'a Netflix binge',
      icon: '📺',
    };
  }

  if (hours < 8) {
    return {
      description: 'a full work day',
      icon: '💼',
    };
  }

  const workDays = Math.round(hours / 8 * 10) / 10;
  return {
    description: `${workDays} work day${workDays !== 1 ? 's' : ''}`,
    icon: '📅',
  };
}

/**
 * Get time saved description for display
 */
export function getTimeSavedDescription(seconds: number): string {
  const equiv = getTimeSavedEquivalent(seconds);
  return `That's enough time for ${equiv.description}`;
}

/**
 * Convert audio duration (minutes) to relatable equivalent
 */
export function getAudioEquivalent(minutes: number): AudioEquivalent {
  if (minutes < 5) {
    const songs = Math.max(1, Math.round(minutes / MINUTES_PER_SONG));
    return {
      count: songs,
      unit: songs === 1 ? 'song' : 'songs',
      description: `${songs} song${songs !== 1 ? 's' : ''}`,
      icon: '🎵',
    };
  }

  if (minutes < 45) {
    const songs = Math.round(minutes / MINUTES_PER_SONG);
    return {
      count: songs,
      unit: 'songs',
      description: `${songs} songs`,
      icon: '🎵',
    };
  }

  if (minutes < 120) {
    const podcasts = Math.round(minutes / MINUTES_PER_PODCAST * 10) / 10;
    return {
      count: podcasts,
      unit: podcasts === 1 ? 'podcast episode' : 'podcast episodes',
      description: `${podcasts} podcast episode${podcasts !== 1 ? 's' : ''}`,
      icon: '🎙️',
    };
  }

  if (minutes < 180) {
    return {
      count: 1,
      unit: 'feature film',
      description: 'a feature film',
      icon: '🎬',
    };
  }

  const audiobookHours = Math.round(minutes / MINUTES_PER_AUDIOBOOK_HOUR * 10) / 10;
  return {
    count: audiobookHours,
    unit: 'audiobook hours',
    description: `${audiobookHours} audiobook hour${audiobookHours !== 1 ? 's' : ''}`,
    icon: '🎧',
  };
}

/**
 * Get percentage ranking description (for comparative metrics)
 */
export function getPercentileDescription(percentile: number): string {
  if (percentile >= 99) return 'Top 1% of users';
  if (percentile >= 95) return 'Top 5% of users';
  if (percentile >= 90) return 'Top 10% of users';
  if (percentile >= 85) return 'Top 15% of users';
  if (percentile >= 75) return 'Top 25% of users';
  if (percentile >= 50) return 'Above average';
  return 'Getting started';
}

/**
 * Format large numbers with abbreviations
 */
export function formatLargeNumber(num: number): string {
  if (num < 1000) return num.toString();
  if (num < 10000) return num.toLocaleString();
  if (num < 1000000) return `${(num / 1000).toFixed(1)}K`;
  return `${(num / 1000000).toFixed(1)}M`;
}

/**
 * Format duration in a human-readable way
 */
export function formatDurationCompact(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)}s`;

  const hours = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);

  if (hours === 0) return `${mins}m`;
  if (mins === 0) return `${hours}h`;
  return `${hours}h ${mins}m`;
}

/**
 * Format duration in a verbose way
 */
export function formatDurationVerbose(seconds: number): string {
  if (seconds < 60) return `${Math.round(seconds)} seconds`;

  const hours = Math.floor(seconds / 3600);
  const mins = Math.floor((seconds % 3600) / 60);

  if (hours === 0) return `${mins} minute${mins !== 1 ? 's' : ''}`;
  if (mins === 0) return `${hours} hour${hours !== 1 ? 's' : ''}`;
  return `${hours} hour${hours !== 1 ? 's' : ''} ${mins} minute${mins !== 1 ? 's' : ''}`;
}

/**
 * Get streak milestone description
 */
export function getStreakMilestone(days: number): { milestone: number; remaining: number; description: string } | null {
  const milestones = [
    { days: 7, description: '1 week streak' },
    { days: 14, description: '2 week streak' },
    { days: 30, description: '1 month streak' },
    { days: 60, description: '2 month streak' },
    { days: 100, description: 'Century streak' },
    { days: 365, description: '1 year streak' },
  ];

  for (const milestone of milestones) {
    if (days < milestone.days) {
      return {
        milestone: milestone.days,
        remaining: milestone.days - days,
        description: milestone.description,
      };
    }
  }

  return null;
}

/**
 * Calculate week-over-week change percentage
 */
export function calculateWoWChange(thisWeek: number, lastWeek: number): {
  percentage: number;
  direction: 'up' | 'down' | 'same';
  description: string;
} {
  if (lastWeek === 0) {
    if (thisWeek > 0) {
      return { percentage: 100, direction: 'up', description: 'New this week!' };
    }
    return { percentage: 0, direction: 'same', description: 'No change' };
  }

  const change = ((thisWeek - lastWeek) / lastWeek) * 100;
  const rounded = Math.round(Math.abs(change));

  if (change > 5) {
    return {
      percentage: rounded,
      direction: 'up',
      description: `${rounded}% more than last week`,
    };
  }
  if (change < -5) {
    return {
      percentage: rounded,
      direction: 'down',
      description: `${rounded}% less than last week`,
    };
  }
  return { percentage: 0, direction: 'same', description: 'About the same as last week' };
}

/**
 * Get a celebration message for milestones
 */
export function getMilestoneMessage(words: number): string | null {
  const milestones = [
    { threshold: 100, message: 'First 100 words!' },
    { threshold: 500, message: '500 words transcribed!' },
    { threshold: 1000, message: '1K words milestone!' },
    { threshold: 5000, message: '5K words - impressive!' },
    { threshold: 10000, message: '10K words - amazing!' },
    { threshold: 25000, message: '25K words - incredible!' },
    { threshold: 50000, message: '50K words - half a novel!' },
    { threshold: 100000, message: '100K words - a full novel!' },
    { threshold: 250000, message: '250K - you\'re prolific!' },
    { threshold: 500000, message: '500K - legendary status!' },
    { threshold: 1000000, message: '1 MILLION WORDS!' },
  ];

  // Find the most recent milestone achieved
  let recentMilestone: { threshold: number; message: string } | null = null;
  for (const milestone of milestones) {
    if (words >= milestone.threshold) {
      recentMilestone = milestone;
    } else {
      break;
    }
  }

  // Only return if we just crossed this milestone (within 10% overage)
  if (recentMilestone && words < recentMilestone.threshold * 1.1) {
    return recentMilestone.message;
  }

  return null;
}
