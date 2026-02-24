/**
 * API client for the Vaak web service.
 * Centralized fetch wrapper with auth, error categorization, and retry.
 */

export type ErrorCategory = "network" | "auth" | "validation" | "rate_limit" | "server" | "unknown";

export class ApiError extends Error {
  category: ErrorCategory;
  status: number;
  detail: string;

  constructor(status: number, detail: string, category: ErrorCategory) {
    super(detail);
    this.name = "ApiError";
    this.status = status;
    this.detail = detail;
    this.category = category;
  }

  /** User-friendly message based on error category */
  get userMessage(): string {
    switch (this.category) {
      case "network":
        return "Connection lost. Check your internet and try again.";
      case "auth":
        return "Please log in again to continue.";
      case "validation":
        return this.detail;
      case "rate_limit":
        return "Too many requests. Please wait a moment and try again.";
      case "server":
        return "Something went wrong on our end. Please try again.";
      default:
        return "An unexpected error occurred.";
    }
  }
}

function categorizeStatus(status: number): ErrorCategory {
  if (status === 401 || status === 403) return "auth";
  if (status === 422 || status === 400) return "validation";
  if (status === 429) return "rate_limit";
  if (status >= 500) return "server";
  return "unknown";
}

let authToken: string | null = null;

export function setToken(token: string | null) {
  authToken = token;
  if (token) {
    localStorage.setItem("vaak_token", token);
  } else {
    localStorage.removeItem("vaak_token");
  }
}

export function getToken(): string | null {
  if (authToken) return authToken;
  authToken = localStorage.getItem("vaak_token");
  return authToken;
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const token = getToken();
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(options.headers as Record<string, string>),
  };
  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  let response: Response;
  try {
    response = await fetch(`/api/v1${path}`, { ...options, headers });
  } catch {
    throw new ApiError(0, "Network error", "network");
  }

  if (!response.ok) {
    let detail = "Request failed";
    try {
      const body = await response.json();
      detail = body.detail || body.error || detail;
    } catch {
      // Response body not JSON
    }
    throw new ApiError(response.status, detail, categorizeStatus(response.status));
  }

  if (response.status === 204) return undefined as T;
  return response.json();
}

// --- Auth ---

export interface AuthResponse {
  access_token: string;
  token_type: string;
}

export interface UserResponse {
  id: number;
  email: string;
  full_name: string | null;
  tier: string;
}

export async function signup(email: string, password: string, fullName?: string): Promise<AuthResponse> {
  return request("/auth/signup", {
    method: "POST",
    body: JSON.stringify({ email, password, full_name: fullName }),
  });
}

export async function login(email: string, password: string): Promise<AuthResponse> {
  return request("/auth/login", {
    method: "POST",
    body: JSON.stringify({ email, password }),
  });
}

export async function getMe(): Promise<UserResponse> {
  return request("/auth/me");
}

// --- Projects ---

export interface ProjectResponse {
  id: string;
  name: string;
  roles: Record<string, RoleConfig>;
  owner_id: number;
  created_at: string;
}

export interface RoleConfig {
  title: string;
  description: string;
  tags: string[];
  permissions: string[];
  maxInstances: number;
  provider: { provider: string; model: string };
}

export async function createProject(name: string, template?: string): Promise<ProjectResponse> {
  return request("/projects/", {
    method: "POST",
    body: JSON.stringify({ name, template }),
  });
}

export async function listProjects(): Promise<ProjectResponse[]> {
  return request("/projects/");
}

export async function getProject(id: string): Promise<ProjectResponse> {
  return request(`/projects/${id}`);
}

export async function deleteProject(id: string): Promise<void> {
  return request(`/projects/${id}`, { method: "DELETE" });
}

export async function updateRoleProvider(
  projectId: string,
  roleSlug: string,
  provider: string,
  model: string
): Promise<void> {
  return request(`/projects/${projectId}/roles/${roleSlug}/provider`, {
    method: "PUT",
    body: JSON.stringify({ provider, model }),
  });
}

export async function startAgent(projectId: string, roleSlug: string): Promise<void> {
  return request(`/projects/${projectId}/roles/${roleSlug}/start`, { method: "POST" });
}

export async function stopAgent(projectId: string, roleSlug: string): Promise<void> {
  return request(`/projects/${projectId}/roles/${roleSlug}/stop`, { method: "POST" });
}

// --- Messages ---

export interface BoardMessage {
  id: number;
  from: string;
  to: string;
  type: string;
  subject: string;
  body: string;
  timestamp: string;
  metadata: Record<string, unknown>;
}

export async function getMessages(projectId: string, sinceId = 0): Promise<{ messages: BoardMessage[]; total: number }> {
  return request(`/messages/${projectId}?since_id=${sinceId}`);
}

export async function sendMessage(
  projectId: string,
  to: string,
  type: string,
  subject: string,
  body: string
): Promise<BoardMessage> {
  return request(`/messages/${projectId}`, {
    method: "POST",
    body: JSON.stringify({ to, type, subject, body }),
  });
}

// --- Billing ---

export interface SubscriptionStatus {
  active: boolean;
  plan: string;
  usage: { tokens_used: number; tokens_limit: number; cost_usd: number };
}

export async function getSubscriptionStatus(): Promise<SubscriptionStatus> {
  return request("/billing/status");
}

export async function createCheckout(plan: string): Promise<{ url: string }> {
  return request("/billing/checkout", {
    method: "POST",
    body: JSON.stringify({ plan }),
  });
}

// --- Providers ---

export interface ModelCatalog {
  models: Array<{ id: string; provider: string; name: string; input_cost: number; output_cost: number }>;
}

export async function getModelCatalog(): Promise<ModelCatalog> {
  return request("/providers/models");
}

export async function getUsageSummary(projectId: string): Promise<Record<string, unknown>> {
  return request(`/providers/usage/${projectId}`);
}
