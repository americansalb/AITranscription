/**
 * API client for communicating with the Scribe backend
 */

// Remove trailing slash if present to avoid double slashes in URLs
const rawUrl = import.meta.env.VITE_API_URL || "http://localhost:8000";
const API_BASE_URL = rawUrl.endsWith("/") ? rawUrl.slice(0, -1) : rawUrl;

// Debug: log the API URL being used
console.log("API URL configured:", API_BASE_URL);

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
  tier: "access" | "standard" | "enterprise";
  is_active: boolean;
  accessibility_verified: boolean;
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
  const formData = new FormData();
  formData.append("audio", audioBlob, "recording.webm");

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
  const formData = new FormData();
  formData.append("audio", audioBlob, "recording.webm");

  if (language) {
    formData.append("language", language);
  }

  const response = await fetch(`${API_BASE_URL}/api/v1/transcribe`, {
    method: "POST",
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

// Stats and Transcript History types
export interface TranscriptItem {
  id: number;
  raw_text: string;
  polished_text: string;
  word_count: number;
  audio_duration_seconds: number;
  words_per_minute: number;
  context: string | null;
  created_at: string;
}

export interface UserStats {
  total_transcriptions: number;
  total_words: number;
  total_audio_seconds: number;
  transcriptions_today: number;
  words_today: number;
  average_words_per_transcription: number;
  average_words_per_minute: number;
  // Time saved calculation
  typing_wpm: number;
  time_saved_seconds: number;
  time_saved_today_seconds: number;
}

/**
 * Get current user's statistics
 */
export async function getUserStats(): Promise<UserStats> {
  const response = await fetch(`${API_BASE_URL}/api/v1/auth/stats`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<UserStats>(response);
}

/**
 * Get current user's transcript history
 */
export async function getTranscriptHistory(
  skip = 0,
  limit = 50
): Promise<TranscriptItem[]> {
  const response = await fetch(
    `${API_BASE_URL}/api/v1/auth/transcripts?skip=${skip}&limit=${limit}`,
    {
      headers: getAuthHeaders(),
    }
  );

  return handleResponse<TranscriptItem[]>(response);
}

// Achievement types
export interface Achievement {
  id: string;
  name: string;
  description: string;
  icon: string;
  category: string;
  unlocked: boolean;
  progress: number; // 0.0 to 1.0
  current_value: number;
  threshold: number;
  unlocked_at: string | null;
}

export interface AchievementsResponse {
  achievements: Achievement[];
  total_unlocked: number;
  total_achievements: number;
}

/**
 * Get current user's achievements (based on real usage data)
 */
export async function getUserAchievements(): Promise<AchievementsResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/auth/achievements`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<AchievementsResponse>(response);
}

/**
 * Update the user's typing WPM for time saved calculations
 */
export async function updateTypingWpm(typingWpm: number): Promise<{ typing_wpm: number; message: string }> {
  const response = await fetch(`${API_BASE_URL}/api/v1/auth/typing-wpm`, {
    method: "PATCH",
    headers: {
      ...getAuthHeaders(),
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ typing_wpm: typingWpm }),
  });

  return handleResponse<{ typing_wpm: number; message: string }>(response);
}

// ============================================
// Learning System API
// ============================================

export interface LearningStats {
  total_corrections: number;
  unique_types: number;
  total_applications: number;
  corrections_by_type: Record<string, number>;
  audio_samples: number;
  audio_duration_seconds: number;
  ready_for_whisper_training: boolean;
  correction_model_version: string | null;
  whisper_model_version: string | null;
}

export interface Correction {
  id: number;
  original_text: string;
  corrected_text: string;
  correction_type: string | null;
  correction_count: number;
  created_at: string | null;
}

export interface CorrectionRule {
  id: number;
  pattern: string;
  replacement: string;
  is_regex: boolean;
  priority: number;
  hit_count: number;
}

export interface FeedbackResponse {
  success: boolean;
  correction_id: number | null;
  message: string;
}

export interface TrainModelResponse {
  success: boolean;
  message: string;
  version: number | null;
  training_loss: number | null;
  training_samples: number | null;
  epochs_trained: number | null;
}

/**
 * Submit a correction to improve the learning system
 */
export async function submitFeedback(
  originalText: string,
  correctedText: string,
  audioSampleId?: number
): Promise<FeedbackResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/feedback`, {
    method: "POST",
    headers: {
      ...getAuthHeaders(),
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      original_text: originalText,
      corrected_text: correctedText,
      audio_sample_id: audioSampleId,
    }),
  });

  return handleResponse<FeedbackResponse>(response);
}

/**
 * Get learning statistics for the current user
 */
export async function getLearningStats(): Promise<LearningStats> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/stats`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<LearningStats>(response);
}

/**
 * Get list of learned corrections
 */
export async function getCorrections(limit = 50): Promise<{ corrections: Correction[]; total: number }> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/corrections?limit=${limit}`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<{ corrections: Correction[]; total: number }>(response);
}

/**
 * Delete a learned correction
 */
export async function deleteCorrection(correctionId: number): Promise<{ success: boolean; message: string }> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/corrections/${correctionId}`, {
    method: "DELETE",
    headers: getAuthHeaders(),
  });

  return handleResponse<{ success: boolean; message: string }>(response);
}

/**
 * Get list of correction rules
 */
export async function getCorrectionRules(): Promise<{ rules: CorrectionRule[]; total: number }> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/rules`, {
    headers: getAuthHeaders(),
  });

  return handleResponse<{ rules: CorrectionRule[]; total: number }>(response);
}

/**
 * Add a new correction rule
 */
export async function addCorrectionRule(
  pattern: string,
  replacement: string,
  isRegex = false,
  priority = 0
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
 * Delete a correction rule
 */
export async function deleteCorrectionRule(ruleId: number): Promise<{ success: boolean; message: string }> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/rules/${ruleId}`, {
    method: "DELETE",
    headers: getAuthHeaders(),
  });

  return handleResponse<{ success: boolean; message: string }>(response);
}

/**
 * Train the correction neural network
 */
export async function trainCorrectionModel(
  epochs = 10,
  batchSize = 16,
  learningRate = 0.0001
): Promise<TrainModelResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/train`, {
    method: "POST",
    headers: {
      ...getAuthHeaders(),
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      epochs,
      batch_size: batchSize,
      learning_rate: learningRate,
    }),
  });

  return handleResponse<TrainModelResponse>(response);
}

/**
 * Train the Whisper model with LoRA
 */
export async function trainWhisperModel(
  epochs = 3,
  batchSize = 4,
  learningRate = 0.0001
): Promise<TrainModelResponse> {
  const response = await fetch(`${API_BASE_URL}/api/v1/learning/train-whisper`, {
    method: "POST",
    headers: {
      ...getAuthHeaders(),
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      epochs,
      batch_size: batchSize,
      learning_rate: learningRate,
    }),
  });

  return handleResponse<TrainModelResponse>(response);
}

/**
 * Use the hybrid correction system to correct text
 */
export async function correctTextHybrid(
  text: string,
  context?: string,
  useLlm = true
): Promise<{
  original: string;
  corrected: string;
  changed: boolean;
  confidence: number;
  source: string;
  corrections_applied: number;
}> {
  const params = new URLSearchParams({ text });
  if (context) params.append("context", context);
  params.append("use_llm", useLlm.toString());

  const response = await fetch(`${API_BASE_URL}/api/v1/learning/correct?${params}`, {
    method: "POST",
    headers: getAuthHeaders(),
  });

  return handleResponse(response);
}

/**
 * Get the API base URL (for components that need direct access)
 */
export function getApiBaseUrl(): string {
  return API_BASE_URL;
}
