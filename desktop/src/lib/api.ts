/**
 * API client for communicating with the Vaak backend
 */

// Remove trailing slash if present to avoid double slashes in URLs
const rawUrl = import.meta.env.VITE_API_URL || "http://127.0.0.1:19836";
const API_BASE_URL = rawUrl.endsWith("/") ? rawUrl.slice(0, -1) : rawUrl;

export interface TranscribeResponse {
  raw_text: string;
  duration: number | null;
  language: string | null;
}

export interface PolishResponse {
  text: string;
  input_tokens: number;
  output_tokens: number;
}

export interface TranscribeAndPolishResponse {
  raw_text: string;
  polished_text: string;
  duration: number | null;
  language: string | null;
  usage: {
    input_tokens: number;
    output_tokens: number;
  };
  saved: boolean;
}

export interface HealthResponse {
  status: string;
  version: string;
  groq_configured: boolean;
  anthropic_configured: boolean;
}

export interface TokenResponse {
  access_token: string;
  token_type: string;
}

export interface UserResponse {
  id: number;
  email: string;
  full_name: string | null;
  tier: "access" | "standard" | "enterprise" | "developer";
  is_active: boolean;
  accessibility_verified: boolean;
}

export interface UserStatsResponse {
  total_transcriptions: number;
  total_words: number;
  total_audio_seconds: number;
  transcriptions_today: number;
  words_today: number;
  average_words_per_transcription: number;
  average_words_per_minute: number;
  typing_wpm: number;
}

export interface ContextStats {
  context: string;
  count: number;
  words: number;
  percentage: number;
}

export interface DailyStats {
  date: string;
  transcriptions: number;
  words: number;
}

export interface HourlyStats {
  hour: number;
  transcriptions: number;
  words: number;
}

export interface DayOfWeekStats {
  day: string;
  day_index: number;
  transcriptions: number;
  words: number;
  percentage: number;
}

export interface MonthlyStats {
  month: string;
  month_label: string;
  transcriptions: number;
  words: number;
  audio_minutes: number;
}

export interface FormalityStats {
  formality: string;
  count: number;
  words: number;
  percentage: number;
}

export interface WordLengthDistribution {
  range_label: string;
  min_words: number;
  max_words: number;
  count: number;
  percentage: number;
}

export interface Achievement {
  id: string;
  name: string;
  description: string;
  icon: string;
  earned: boolean;
  earned_at?: string | null;
  progress: number;
  target?: number | null;
  current?: number | null;
}

export interface GrowthMetrics {
  words_wow_change: number;
  words_mom_change: number;
  transcriptions_wow_change: number;
  transcriptions_mom_change: number;
  last_week_words: number;
  prev_week_words: number;
  last_month_words: number;
  prev_month_words: number;
}

export interface ProductivityInsights {
  peak_hour: number;
  peak_hour_label: string;
  peak_day: string;
  avg_session_words: number;
  avg_session_duration_seconds: number;
  busiest_week_ever: string | null;
  busiest_week_words: number;
  efficiency_score: number;
}

export interface DetailedStatsResponse {
  // Totals
  total_transcriptions: number;
  total_words: number;
  total_audio_seconds: number;
  total_characters: number;

  // Time-based
  transcriptions_today: number;
  words_today: number;
  transcriptions_this_week: number;
  words_this_week: number;
  transcriptions_this_month: number;
  words_this_month: number;

  // Averages
  average_words_per_transcription: number;
  average_words_per_minute: number;
  average_transcriptions_per_day: number;
  average_audio_duration_seconds: number;

  // Time saved
  estimated_time_saved_minutes: number;

  // Context breakdown
  context_breakdown: ContextStats[];
  formality_breakdown: FormalityStats[];

  // Daily activity (last 7 days)
  daily_activity: DailyStats[];
  hourly_activity: HourlyStats[];
  day_of_week_breakdown: DayOfWeekStats[];
  monthly_trends: MonthlyStats[];
  word_length_distribution: WordLengthDistribution[];

  // Streaks
  current_streak_days: number;
  longest_streak_days: number;

  // Records
  most_productive_day: string | null;
  most_productive_day_words: number;
  longest_transcription_words: number;
  shortest_transcription_words: number;
  fastest_wpm: number;
  slowest_wpm: number;

  // Growth and productivity
  growth: GrowthMetrics;
  productivity: ProductivityInsights;

  // Achievements
  achievements: Achievement[];

  // Member info
  member_since: string;
  days_as_member: number;
  total_active_days: number;
}

export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
    public detail?: string
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function handleResponse<T>(response: Response): Promise<T> {
  if (!response.ok) {
    const error = await response.json().catch(() => ({}));
    // Handle error.detail properly - it might be a string, object, or array (FastAPI validation errors)
    let errorMessage: string;
    if (typeof error.detail === "string") {
      errorMessage = error.detail;
    } else if (Array.isArray(error.detail)) {
      // FastAPI validation errors come as array of {loc, msg, type}
      errorMessage = error.detail.map((e: { msg?: string }) => e.msg || "Validation error").join(", ");
    } else if (error.detail && typeof error.detail === "object") {
      errorMessage = JSON.stringify(error.detail);
    } else {
      errorMessage = `HTTP ${response.status}`;
    }
    // Auto-logout on expired/invalid token so user can re-authenticate
    if (response.status === 401) {
      authToken = null;
      localStorage.removeItem("vaak_token");
    }
    throw new ApiError(
      errorMessage,
      response.status,
      typeof error.detail === "string" ? error.detail : JSON.stringify(error.detail)
    );
  }
  return response.json();
}

/**
 * Check backend health and configuration
 */
export async function checkHealth(): Promise<HealthResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/health`);
  return handleResponse<HealthResponse>(response);
}

/**
 * Get filename with correct extension based on audio MIME type
 */
function getAudioFilename(blob: Blob): string {
  const mimeType = blob.type || "audio/webm";
  if (mimeType.includes("mp4") || mimeType.includes("m4a")) return "recording.mp4";
  if (mimeType.includes("ogg")) return "recording.ogg";
  if (mimeType.includes("wav")) return "recording.wav";
  return "recording.webm";
}

// Available Whisper models
export type WhisperModel = "whisper-large-v3" | "whisper-large-v3-turbo";

export const WHISPER_MODELS: { value: WhisperModel; label: string; description: string }[] = [
  { value: "whisper-large-v3-turbo", label: "Turbo", description: "Faster, cost-effective" },
  { value: "whisper-large-v3", label: "Large V3", description: "Higher accuracy" },
];

/**
 * Transcribe audio and polish the result
 */
export async function transcribeAndPolish(
  audioBlob: Blob,
  options: {
    language?: string;
    context?: string;
    formality?: "casual" | "neutral" | "formal";
    model?: WhisperModel;
    signal?: AbortSignal;
  } = {}
): Promise<TranscribeAndPolishResponse> {
  // Use correct filename based on actual audio format
  const filename = getAudioFilename(audioBlob);

  const formData = new FormData();
  formData.append("audio", audioBlob, filename);

  if (options.language) {
    formData.append("language", options.language);
  }
  if (options.context) {
    formData.append("context", options.context);
  }
  if (options.formality) {
    formData.append("formality", options.formality);
  }
  if (options.model) {
    formData.append("model", options.model);
  }

  // Proportional timeout based on audio file size
  // ~16KB per second of audio at typical compression
  // Minimum 60 seconds, maximum 5 minutes (down from 10 min)
  const estimatedDurationSeconds = Math.max(audioBlob.size / 16000, 1);
  const timeoutMs = Math.min(
    Math.max((estimatedDurationSeconds + 60) * 1000, 60000),  // min 60 seconds
    300000  // max 5 minutes
  );

  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), timeoutMs);

  // If caller provides an external signal (e.g. user cancel button), forward abort
  if (options.signal) {
    if (options.signal.aborted) {
      clearTimeout(timeoutId);
      throw new ApiError("Transcription cancelled", 0);
    }
    options.signal.addEventListener("abort", () => controller.abort(), { once: true });
  }

  try {
    const response = await fetch(`${API_BASE_URL}/api/v1/transcribe-and-polish`, {
      method: "POST",
      headers: getAuthHeaders(),
      body: formData,
      signal: controller.signal,
    });

    clearTimeout(timeoutId);
    const data = await handleResponse<TranscribeAndPolishResponse>(response);

    // CRITICAL: Validate response has actual transcription text
    // For medical use, we cannot accept empty or missing transcriptions
    if (!data.raw_text || typeof data.raw_text !== "string" || data.raw_text.trim().length === 0) {
      throw new ApiError("Transcription returned empty - no speech detected in audio", 422, "Please speak clearly and try again");
    }
    if (!data.polished_text || typeof data.polished_text !== "string") {
      throw new ApiError("Text processing failed", 422, "Could not process transcription");
    }

    return data;
  } catch (error) {
    clearTimeout(timeoutId);
    if (error instanceof Error && error.name === "AbortError") {
      // Distinguish user cancel from timeout
      if (options.signal?.aborted) {
        throw new ApiError("Transcription cancelled", 0);
      }
      const timeoutSecs = Math.round(timeoutMs / 1000);
      throw new ApiError(`Transcription request timed out after ${timeoutSecs} seconds`, 408);
    }
    throw error;
  }
}

/**
 * Transcribe audio only (without polish)
 */
export async function transcribe(
  audioBlob: Blob,
  language?: string,
  signal?: AbortSignal
): Promise<TranscribeResponse> {
  // Use correct filename based on actual audio format
  const filename = getAudioFilename(audioBlob);

  const formData = new FormData();
  formData.append("audio", audioBlob, filename);

  if (language) {
    formData.append("language", language);
  }

  // Proportional timeout: min 60s, max 5 min (matches transcribe+polish ceiling)
  const estimatedDurationSeconds = Math.max(audioBlob.size / 16000, 1);
  const timeoutMs = Math.min(
    Math.max((estimatedDurationSeconds + 30) * 1000, 60000),
    300000  // max 5 minutes
  );

  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), timeoutMs);

  if (signal) {
    if (signal.aborted) {
      clearTimeout(timeoutId);
      throw new ApiError("Transcription cancelled", 0);
    }
    signal.addEventListener("abort", () => controller.abort(), { once: true });
  }

  try {
    const response = await fetch(`${API_BASE_URL}/api/v1/transcribe`, {
      method: "POST",
      headers: getAuthHeaders(),
      body: formData,
      signal: controller.signal,
    });

    clearTimeout(timeoutId);
    return handleResponse<TranscribeResponse>(response);
  } catch (error) {
    clearTimeout(timeoutId);
    if (error instanceof Error && error.name === "AbortError") {
      if (signal?.aborted) {
        throw new ApiError("Transcription cancelled", 0);
      }
      const timeoutSecs = Math.round(timeoutMs / 1000);
      throw new ApiError(`Transcription request timed out after ${timeoutSecs} seconds`, 408);
    }
    throw error;
  }
}

/**
 * Polish text only
 */
export async function polish(
  text: string,
  options: {
    context?: string;
    formality?: "casual" | "neutral" | "formal";
    custom_words?: string[];
  } = {}
): Promise<PolishResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/polish`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      text,
      ...options,
    }),
  });

  return handleResponse<PolishResponse>(response);
}

// Token storage
let authToken: string | null = null;

export function setAuthToken(token: string | null) {
  authToken = token;
  if (token) {
    localStorage.setItem("vaak_token", token);
  } else {
    localStorage.removeItem("vaak_token");
  }
}

export function getAuthToken(): string | null {
  if (!authToken) {
    authToken = localStorage.getItem("vaak_token");
  }
  return authToken;
}

function getAuthHeaders(): HeadersInit {
  const token = getAuthToken();
  if (token) {
    return { Authorization: `Bearer ${token}` };
  }
  return {};
}

/**
 * Sign up a new user
 */
export async function signup(
  email: string,
  password: string,
  fullName?: string
): Promise<TokenResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/auth/signup`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      email,
      password,
      full_name: fullName,
    }),
  });

  const result = await handleResponse<TokenResponse>(response);
  setAuthToken(result.access_token);
  return result;
}

/**
 * Log in an existing user
 */
export async function login(email: string, password: string): Promise<TokenResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/auth/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, password }),
  });

  const result = await handleResponse<TokenResponse>(response);
  setAuthToken(result.access_token);
  return result;
}

/**
 * Get the current user's information
 */
export async function getCurrentUser(): Promise<UserResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/auth/me`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<UserResponse>(response);
}

/**
 * Log out the current user
 */
export function logout() {
  setAuthToken(null);
}

/**
 * Check if user is logged in
 */
export function isLoggedIn(): boolean {
  return getAuthToken() !== null;
}

/**
 * Get the current user's statistics (basic)
 */
export async function getBasicUserStats(): Promise<UserStatsResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/auth/stats`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<UserStatsResponse>(response);
}

/**
 * Get detailed statistics with insights
 */
export async function getDetailedStats(): Promise<DetailedStatsResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/auth/stats/detailed`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<DetailedStatsResponse>(response);
}

/**
 * Get the API base URL
 */
export function getApiBaseUrl(): string {
  return API_BASE_URL;
}

// ============================================
// User Stats Types (extended version for StatsPanel)
// ============================================

export interface UserStats {
  total_transcriptions: number;
  total_words: number;
  total_audio_seconds: number;
  transcriptions_today: number;
  words_today: number;
  average_words_per_transcription: number;
  average_words_per_minute: number;
  time_saved_seconds: number;
  time_saved_today_seconds: number;
  typing_wpm: number;
}

/**
 * Get the current user's statistics with time saved calculations
 */
export async function getUserStats(): Promise<UserStats> {
  const response = await fetch(`${API_BASE_URL}/api/v1/auth/stats`, {
    headers: getAuthHeaders(),
  });

  const data = await handleResponse<UserStatsResponse>(response);

  // Convert to UserStats with calculated fields
  const typingWpm = data.typing_wpm || 40;
  return {
    total_transcriptions: data.total_transcriptions,
    total_words: data.total_words,
    total_audio_seconds: data.total_audio_seconds,
    transcriptions_today: data.transcriptions_today,
    words_today: data.words_today,
    average_words_per_transcription: data.average_words_per_transcription,
    average_words_per_minute: data.average_words_per_minute,
    time_saved_seconds: Math.max(0, (data.total_words / typingWpm) * 60 - data.total_audio_seconds),
    time_saved_today_seconds: Math.max(0, (data.words_today / typingWpm) * 60),
    typing_wpm: typingWpm,
  };
}

export interface TranscriptItem {
  id: number;
  raw_text: string;
  polished_text: string;
  word_count: number;
  audio_duration_seconds: number | null;
  words_per_minute: number;
  context: string | null;
  formality: string | null;
  created_at: string;
  transcript_type: "input" | "output";
  session_id: string | null;
}

export interface AchievementItem {
  id: string;
  name: string;
  description: string;
  icon: string;
  unlocked: boolean;
  unlocked_at: string | null;
  progress: number;
  category: string;
  threshold: number;
  current_value: number;
}

export interface AchievementsResponse {
  achievements: AchievementItem[];
  total_achievements: number;
  total_unlocked: number;
}

/**
 * Get transcript history
 */
export async function getTranscriptHistory(offset: number = 0, limit: number = 50): Promise<TranscriptItem[]> {
  const response = await fetch(`${API_BASE_URL}/api/v1/auth/transcripts?skip=${offset}&limit=${limit}`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<TranscriptItem[]>(response);
}

/**
 * Get user achievements
 * Returns empty - achievements are now displayed directly from detailedStats
 */
export async function getUserAchievements(): Promise<AchievementsResponse> {
  return {
    achievements: [],
    total_achievements: 0,
    total_unlocked: 0,
  };
}

/**
 * Update user's typing WPM for time saved calculation
 */
export async function updateTypingWpm(wpm: number): Promise<void> {
  const response = await fetch(`${API_BASE_URL}/api/v1/auth/settings/typing-wpm`, {
    method: "PATCH",
    headers: {
      ...getAuthHeaders(),
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ wpm }),
  });

  if (!response.ok) {
    throw new Error("Failed to update typing WPM");
  }
}

// ============================================
// Learning System Types and Functions
// ============================================

export interface LearningStats {
  total_corrections: number;
  total_applications: number;
  audio_samples: number;
  audio_duration_seconds: number;
  correction_model_version: string | null;
  whisper_model_version: string | null;
  corrections_by_type: Record<string, number>;
  ready_for_whisper_training: boolean;
}

export interface Correction {
  id: number;
  original_text: string;
  corrected_text: string;
  correction_type: string | null;
  correction_count: number;
  created_at: string;
}

export interface CorrectionRule {
  id: number;
  pattern: string;
  replacement: string;
  is_regex: boolean;
  priority: number;
  hit_count: number;
  created_at: string;
}

export interface FeedbackResponse {
  success: boolean;
  message: string;
  correction_id?: number;
}

export interface TrainingResponse {
  success: boolean;
  message: string;
}

/**
 * Submit learning feedback when user edits a transcription
 */
export async function submitFeedback(originalText: string, correctedText: string): Promise<FeedbackResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/feedback`, {
    method: "POST",
    headers: {
      ...getAuthHeaders(),
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      original_text: originalText,
      corrected_text: correctedText,
    }),
  });

  return handleResponse<FeedbackResponse>(response);
}

/**
 * Get learning statistics
 */
export async function getLearningStats(): Promise<LearningStats> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/stats`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<LearningStats>(response);
}

/**
 * Get user's corrections
 */
export async function getCorrections(limit: number = 50): Promise<{ corrections: Correction[] }> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/corrections?limit=${limit}`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<{ corrections: Correction[] }>(response);
}

/**
 * Get user's correction rules
 */
export async function getCorrectionRules(): Promise<{ rules: CorrectionRule[] }> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/rules`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<{ rules: CorrectionRule[] }>(response);
}

/**
 * Delete a correction
 */
export async function deleteCorrection(id: number): Promise<void> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/corrections/${id}`, {
    method: "DELETE",
    headers: getAuthHeaders(),
  });

  await handleResponse<{ success: boolean }>(response);
}

/**
 * Delete a correction rule
 */
export async function deleteCorrectionRule(id: number): Promise<void> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/rules/${id}`, {
    method: "DELETE",
    headers: getAuthHeaders(),
  });

  await handleResponse<{ success: boolean }>(response);
}

/**
 * Add a new correction rule
 */
export async function addCorrectionRule(
  pattern: string,
  replacement: string,
  isRegex: boolean,
  priority: number
): Promise<CorrectionRule> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/rules`, {
    method: "POST",
    headers: {
      ...getAuthHeaders(),
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      pattern,
      replacement,
      is_regex: isRegex,
      priority,
    }),
  });

  return handleResponse<CorrectionRule>(response);
}

/**
 * Train the correction model
 */
export async function trainCorrectionModel(): Promise<TrainingResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/train/corrections`, {
    method: "POST",
    headers: getAuthHeaders(),
  });

  return handleResponse<TrainingResponse>(response);
}

/**
 * Train the Whisper model
 */
export async function trainWhisperModel(): Promise<TrainingResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/train/whisper`, {
    method: "POST",
    headers: getAuthHeaders(),
  });

  return handleResponse<TrainingResponse>(response);
}

// ============================================
// Gamification Types and Functions
// ============================================

export type PrestigeTier = "bronze" | "silver" | "gold" | "platinum" | "diamond" | "master" | "legend";
export type AchievementRarity = "common" | "rare" | "epic" | "legendary";
export type AchievementCategory =
  | "volume"
  | "streak"
  | "speed"
  | "context"
  | "formality"
  | "learning"
  | "temporal"
  | "records"
  | "combo"
  | "special";

export interface TierProgress {
  current_tier: PrestigeTier;
  next_tier: PrestigeTier | null;
  tier_start_xp: number;
  tier_end_xp: number | null;
  xp_in_tier: number;
  progress: number;
  color: string;
}

export interface AchievementStats {
  unlocked: number;
  total: number;
  progress: number;
  by_rarity: {
    common: number;
    rare: number;
    epic: number;
    legendary: number;
  };
}

export interface GamificationProgress {
  user_id: number;
  current_level: number;
  current_xp: number;
  xp_to_next_level: number;
  level_progress: number;
  lifetime_xp: number;
  prestige_tier: PrestigeTier;
  tier_color: string;
  tier_progress: TierProgress;
  xp_multiplier: number;
  achievements: AchievementStats;
  last_xp_earned_at: string | null;
}

export interface GamificationAchievement {
  id: string;
  name: string;
  description: string;
  category: AchievementCategory;
  rarity: AchievementRarity;
  xp_reward: number;
  icon: string;
  tier: number;
  threshold: number;
  metric_type: string;
  is_hidden: boolean;
  is_unlocked: boolean;
  current_value: number;
  progress: number;
  unlocked_at: string | null;
}

export interface AchievementListResponse {
  achievements: GamificationAchievement[];
  total: number;
  page: number;
  page_size: number;
  total_pages: number;
}

export interface NewlyUnlockedResponse {
  achievements: GamificationAchievement[];
  xp_gained: number;
  level_changes: {
    current_level: number;
    prestige_tier: PrestigeTier;
    lifetime_xp: number;
  } | null;
}

export interface XPTransaction {
  id: number;
  amount: number;
  final_amount: number;
  multiplier: number;
  source: string;
  source_id: string | null;
  description: string | null;
  level_before: number;
  level_after: number;
  created_at: string;
}

export interface LeaderboardEntry {
  rank: number;
  user_id: number;
  display_name: string;
  level: number;
  tier: PrestigeTier;
  achievements: number;
  lifetime_xp?: number;
  total_words?: number;
}

export interface LeaderboardResponse {
  leaderboard: LeaderboardEntry[];
  user_rank: {
    user_id: number;
    rank: number;
    total_users: number;
    metric: string;
    value: number;
    percentile: number;
  } | null;
}

/**
 * Get the current user's gamification progress
 */
export async function getGamificationProgress(): Promise<GamificationProgress> {
  const response = await fetch(`${API_BASE_URL}/api/v1/gamification/progress`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<GamificationProgress>(response);
}

/**
 * Check for newly unlocked achievements
 */
export async function checkAchievements(): Promise<NewlyUnlockedResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/gamification/check`, {
    method: "POST",
    headers: getAuthHeaders(),
  });

  return handleResponse<NewlyUnlockedResponse>(response);
}

/**
 * Get paginated achievements with filters
 */
export async function getGamificationAchievements(params: {
  category?: AchievementCategory;
  rarity?: AchievementRarity;
  page?: number;
  page_size?: number;
  unlocked_only?: boolean;
}): Promise<AchievementListResponse> {
  const searchParams = new URLSearchParams();
  if (params.category) searchParams.set("category", params.category);
  if (params.rarity) searchParams.set("rarity", params.rarity);
  if (params.page) searchParams.set("page", params.page.toString());
  if (params.page_size) searchParams.set("page_size", params.page_size.toString());
  if (params.unlocked_only) searchParams.set("unlocked_only", "true");

  const response = await fetch(
    `${API_BASE_URL}/api/v1/gamification/achievements?${searchParams.toString()}`,
    {
      headers: getAuthHeaders(),
    }
  );

  return handleResponse<AchievementListResponse>(response);
}

/**
 * Get unnotified achievements (for toast notifications)
 */
export async function getUnnotifiedAchievements(): Promise<GamificationAchievement[]> {
  const response = await fetch(`${API_BASE_URL}/api/v1/gamification/achievements/unnotified`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<GamificationAchievement[]>(response);
}

/**
 * Mark achievements as notified
 */
export async function markAchievementsNotified(achievementIds: string[]): Promise<void> {
  const response = await fetch(`${API_BASE_URL}/api/v1/gamification/achievements/mark-notified`, {
    method: "POST",
    headers: {
      ...getAuthHeaders(),
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ achievement_ids: achievementIds }),
  });

  await handleResponse<{ status: string }>(response);
}

/**
 * Get recent XP transactions
 */
export async function getXPTransactions(limit: number = 20): Promise<XPTransaction[]> {
  const response = await fetch(`${API_BASE_URL}/api/v1/gamification/transactions?limit=${limit}`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<XPTransaction[]>(response);
}

/**
 * Get the leaderboard
 */
export async function getLeaderboard(
  metric: "lifetime_xp" | "achievements" | "words" = "lifetime_xp",
  limit: number = 100,
  includeUserRank: boolean = true
): Promise<LeaderboardResponse> {
  const response = await fetch(
    `${API_BASE_URL}/api/v1/gamification/leaderboard?metric=${metric}&limit=${limit}&include_user_rank=${includeUserRank}`,
    {
      headers: getAuthHeaders(),
    }
  );

  return handleResponse<LeaderboardResponse>(response);
}

/**
 * Describe a screenshot using Claude Vision API
 */
export async function describeScreen(imageBase64: string, blindMode: boolean, detail: number): Promise<{
  description: string;
  input_tokens: number;
  output_tokens: number;
}> {
  const response = await fetch(`${API_BASE_URL}/api/v1/describe-screen`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      image_base64: imageBase64,
      blind_mode: blindMode,
      detail: detail,
    }),
  });
  if (!response.ok) {
    const error = await response.text().catch(() => "Unknown error");
    throw new ApiError(`Screen description failed`, response.status, error);
  }
  return response.json();
}

/**
 * Get available achievement categories and rarities
 */
export async function getAchievementCategories(): Promise<{
  categories: AchievementCategory[];
  rarities: AchievementRarity[];
}> {
  const response = await fetch(`${API_BASE_URL}/api/v1/gamification/categories`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<{ categories: AchievementCategory[]; rarities: AchievementRarity[] }>(response);
}
