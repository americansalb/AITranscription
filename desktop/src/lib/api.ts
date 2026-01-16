/**
 * API client for communicating with the Scribe backend
 */

// Remove trailing slash if present to avoid double slashes in URLs
const rawUrl = import.meta.env.VITE_API_URL || "http://localhost:8000";
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
    throw new ApiError(
      error.detail || `HTTP ${response.status}`,
      response.status,
      error.detail
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

  // Add timeout to prevent infinite hang (60 seconds for transcription)
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), 60000);

  try {
    const response = await fetch(`${API_BASE_URL}/api/v1/transcribe-and-polish`, {
      method: "POST",
      headers: getAuthHeaders(),
      body: formData,
      signal: controller.signal,
    });

    clearTimeout(timeoutId);
    return handleResponse<TranscribeAndPolishResponse>(response);
  } catch (error) {
    clearTimeout(timeoutId);
    if (error instanceof Error && error.name === "AbortError") {
      throw new ApiError("Transcription request timed out after 60 seconds", 408);
    }
    throw error;
  }
}

/**
 * Transcribe audio only (without polish)
 */
export async function transcribe(
  audioBlob: Blob,
  language?: string
): Promise<TranscribeResponse> {
  // Use correct filename based on actual audio format
  const filename = getAudioFilename(audioBlob);

  const formData = new FormData();
  formData.append("audio", audioBlob, filename);

  if (language) {
    formData.append("language", language);
  }

  const response = await fetch(`${API_BASE_URL}/api/v1/transcribe`, {
    method: "POST",
    headers: getAuthHeaders(),
    body: formData,
  });

  return handleResponse<TranscribeResponse>(response);
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
    localStorage.setItem("scribe_token", token);
  } else {
    localStorage.removeItem("scribe_token");
  }
}

export function getAuthToken(): string | null {
  if (!authToken) {
    authToken = localStorage.getItem("scribe_token");
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
  return {
    total_transcriptions: data.total_transcriptions,
    total_words: data.total_words,
    total_audio_seconds: data.total_audio_seconds,
    transcriptions_today: data.transcriptions_today,
    words_today: data.words_today,
    average_words_per_transcription: data.average_words_per_transcription,
    average_words_per_minute: data.average_words_per_minute,
    time_saved_seconds: Math.max(0, (data.total_words / 40) * 60 - data.total_audio_seconds),
    time_saved_today_seconds: Math.max(0, (data.words_today / 40) * 60),
    typing_wpm: 40,
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
 * Note: Backend endpoint not yet implemented, returns empty achievements
 */
export async function getUserAchievements(): Promise<AchievementsResponse> {
  // TODO: Implement backend endpoint
  return {
    achievements: [],
    total_achievements: 0,
    total_unlocked: 0,
  };
}

/**
 * Update user's typing WPM for time saved calculation
 * Note: Backend endpoint not yet implemented
 */
export async function updateTypingWpm(wpm: number): Promise<void> {
  // TODO: Implement backend endpoint
  console.log("Typing WPM update not yet implemented:", wpm);
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
