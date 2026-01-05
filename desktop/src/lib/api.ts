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

/**
 * Transcribe audio and polish the result
 */
export async function transcribeAndPolish(
  audioBlob: Blob,
  options: {
    language?: string;
    context?: string;
    formality?: "casual" | "neutral" | "formal";
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

  const response = await fetch(`${API_BASE_URL}/api/v1/transcribe-and-polish`, {
    method: "POST",
    headers: getAuthHeaders(),
    body: formData,
  });

  return handleResponse<TranscribeAndPolishResponse>(response);
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
 * Get the current user's statistics
 */
export async function getUserStats(): Promise<UserStatsResponse> {
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
