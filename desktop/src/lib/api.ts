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
