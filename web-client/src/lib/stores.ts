/**
 * Zustand stores — centralized state management.
 * Replaces the 95-useState pattern from the desktop CollabTab.
 * Organized into focused stores: auth, project, messages, ui.
 */

import { create } from "zustand";
import * as api from "./api";
import type { BoardMessage, DiscussionResponse, ProjectResponse, UserResponse } from "./api";

// --- Auth Store ---

interface AuthState {
  user: UserResponse | null;
  loading: boolean;
  error: string | null;
  login: (email: string, password: string) => Promise<void>;
  signup: (email: string, password: string, fullName?: string) => Promise<void>;
  logout: () => void;
  loadUser: () => Promise<void>;
}

export const useAuthStore = create<AuthState>((set) => ({
  user: null,
  loading: false,
  error: null,

  login: async (email, password) => {
    set({ loading: true, error: null });
    try {
      const res = await api.login(email, password);
      api.setToken(res.access_token);
      const user = await api.getMe();
      set({ user, loading: false });
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Login failed";
      set({ error: msg, loading: false });
    }
  },

  signup: async (email, password, fullName) => {
    set({ loading: true, error: null });
    try {
      const res = await api.signup(email, password, fullName);
      api.setToken(res.access_token);
      const user = await api.getMe();
      set({ user, loading: false });
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Signup failed";
      set({ error: msg, loading: false });
    }
  },

  logout: () => {
    api.setToken(null);
    set({ user: null, error: null });
  },

  loadUser: async () => {
    if (!api.getToken()) return;
    set({ loading: true });
    try {
      const user = await api.getMe();
      set({ user, loading: false });
    } catch {
      api.setToken(null);
      set({ user: null, loading: false });
    }
  },
}));

// --- Project Store ---

interface ProjectState {
  projects: ProjectResponse[];
  activeProject: ProjectResponse | null;
  loading: boolean;
  error: string | null;
  loadProjects: () => Promise<void>;
  selectProject: (id: string) => Promise<void>;
  createProject: (name: string, template?: string) => Promise<void>;
  deleteProject: (id: string) => Promise<void>;
  updateRoleProvider: (roleSlug: string, provider: string, model: string) => Promise<void>;
  startAgent: (roleSlug: string) => Promise<void>;
  stopAgent: (roleSlug: string) => Promise<void>;
}

export const useProjectStore = create<ProjectState>((set, get) => ({
  projects: [],
  activeProject: null,
  loading: false,
  error: null,

  loadProjects: async () => {
    set({ loading: true, error: null });
    try {
      const projects = await api.listProjects();
      set({ projects, loading: false });
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to load projects";
      set({ error: msg, loading: false });
    }
  },

  selectProject: async (id) => {
    set({ loading: true, error: null });
    try {
      const project = await api.getProject(id);
      set({ activeProject: project, loading: false });
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to load project";
      set({ error: msg, loading: false });
    }
  },

  createProject: async (name, template) => {
    set({ loading: true, error: null });
    try {
      const project = await api.createProject(name, template);
      set((s) => ({ projects: [project, ...s.projects], activeProject: project, loading: false }));
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to create project";
      set({ error: msg, loading: false });
    }
  },

  deleteProject: async (id) => {
    try {
      await api.deleteProject(id);
      set((s) => ({
        projects: s.projects.filter((p) => p.id !== id),
        activeProject: s.activeProject?.id === id ? null : s.activeProject,
      }));
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to delete project";
      set({ error: msg });
    }
  },

  updateRoleProvider: async (roleSlug, provider, model) => {
    const project = get().activeProject;
    if (!project) return;
    try {
      await api.updateRoleProvider(project.id, roleSlug, provider, model);
      // Optimistic update
      set((s) => {
        if (!s.activeProject) return s;
        const roles = { ...s.activeProject.roles };
        if (roles[roleSlug]) {
          roles[roleSlug] = { ...roles[roleSlug], provider: { provider, model } };
        }
        return { activeProject: { ...s.activeProject, roles } };
      });
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to update provider";
      set({ error: msg });
    }
  },

  startAgent: async (roleSlug) => {
    const project = get().activeProject;
    if (!project) return;
    try {
      await api.startAgent(project.id, roleSlug);
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to start agent";
      set({ error: msg });
    }
  },

  stopAgent: async (roleSlug) => {
    const project = get().activeProject;
    if (!project) return;
    try {
      await api.stopAgent(project.id, roleSlug);
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to stop agent";
      set({ error: msg });
    }
  },
}));

// --- Message Store ---

interface MessageState {
  messages: BoardMessage[];
  loading: boolean;
  error: string | null;
  connected: boolean;
  ws: WebSocket | null;
  loadMessages: (projectId: string) => Promise<void>;
  connectWs: (projectId: string) => void;
  disconnectWs: () => void;
  sendMessage: (projectId: string, to: string, type: string, subject: string, body: string) => Promise<void>;
}

export const useMessageStore = create<MessageState>((set, get) => ({
  messages: [],
  loading: false,
  error: null,
  connected: false,
  ws: null,

  loadMessages: async (projectId) => {
    set({ loading: true, error: null });
    try {
      const res = await api.getMessages(projectId);
      set({ messages: res.messages, loading: false });
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to load messages";
      set({ error: msg, loading: false });
    }
  },

  connectWs: (projectId) => {
    const existing = get().ws;
    if (existing) existing.close();

    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const ws = new WebSocket(`${protocol}//${window.location.host}/api/v1/messages/${projectId}/ws`);
    let reconnectDelay = 3000;
    let reconnectAttempts = 0;
    const MAX_RECONNECT_ATTEMPTS = 10;

    ws.onopen = () => {
      // Send JWT auth as the first message (required by backend)
      const token = api.getToken();
      if (token) {
        ws.send(JSON.stringify({ type: "auth", token }));
      }
      set({ connected: true });
      reconnectDelay = 3000;
      reconnectAttempts = 0;
    };
    ws.onclose = () => {
      set({ connected: false, ws: null });
      if (reconnectAttempts < MAX_RECONNECT_ATTEMPTS) {
        reconnectAttempts++;
        setTimeout(() => {
          if (!get().ws) get().connectWs(projectId);
        }, reconnectDelay);
        reconnectDelay = Math.min(reconnectDelay * 2, 60000);
      } else {
        console.error("[WS] Max reconnect attempts reached, giving up");
      }
    };
    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data) as BoardMessage;
        set((s) => {
          const updated = [...s.messages, msg];
          // Cap at 500 messages to prevent unbounded memory growth
          return { messages: updated.length > 500 ? updated.slice(-500) : updated };
        });
      } catch {
        // Ignore non-message frames (e.g., status acks)
      }
    };

    set({ ws });
  },

  disconnectWs: () => {
    const ws = get().ws;
    if (ws) ws.close();
    set({ ws: null, connected: false });
  },

  sendMessage: async (projectId, to, type, subject, body) => {
    try {
      const msg = await api.sendMessage(projectId, to, type, subject, body);
      set((s) => ({ messages: [...s.messages, msg] }));
    } catch (e) {
      const errMsg = e instanceof api.ApiError ? e.userMessage : "Failed to send message";
      set({ error: errMsg });
    }
  },
}));

// --- Discussion Store ---

interface DiscussionState {
  discussion: DiscussionResponse | null;
  loading: boolean;
  error: string | null;
  loadActive: (projectId: string) => Promise<void>;
  start: (
    projectId: string,
    mode: string,
    topic: string,
    participants?: string[],
    options?: { max_rounds?: number; auto_close_timeout_seconds?: number },
  ) => Promise<void>;
  openRound: (projectId: string) => Promise<void>;
  closeRound: (projectId: string) => Promise<void>;
  end: (projectId: string) => Promise<void>;
  submit: (projectId: string, body: string) => Promise<void>;
  setTeams: (projectId: string, teams: { for: string[]; against: string[] }) => Promise<void>;
  setTimeout: (projectId: string, seconds: number) => Promise<void>;
}

export const useDiscussionStore = create<DiscussionState>((set, get) => ({
  discussion: null,
  loading: false,
  error: null,

  loadActive: async (projectId) => {
    set({ loading: true, error: null });
    try {
      const disc = await api.getActiveDiscussion(projectId);
      set({ discussion: disc, loading: false });
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to load discussion";
      set({ error: msg, loading: false });
    }
  },

  start: async (projectId, mode, topic, participants = [], options = {}) => {
    set({ loading: true, error: null });
    try {
      const disc = await api.startDiscussion(projectId, mode, topic, participants, options);
      set({ discussion: disc, loading: false });
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to start discussion";
      set({ error: msg, loading: false });
    }
  },

  openRound: async (projectId) => {
    const disc = get().discussion;
    if (!disc) return;
    try {
      await api.openNextRound(projectId, disc.id);
      await get().loadActive(projectId);
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to open round";
      set({ error: msg });
    }
  },

  closeRound: async (projectId) => {
    const disc = get().discussion;
    if (!disc) return;
    try {
      await api.closeRound(projectId, disc.id);
      await get().loadActive(projectId);
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to close round";
      set({ error: msg });
    }
  },

  end: async (projectId) => {
    const disc = get().discussion;
    if (!disc) return;
    try {
      await api.endDiscussion(projectId, disc.id);
      set({ discussion: null });
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to end discussion";
      set({ error: msg });
    }
  },

  submit: async (projectId, body) => {
    const disc = get().discussion;
    if (!disc) return;
    try {
      await api.submitToRound(projectId, disc.id, body);
      await get().loadActive(projectId);
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to submit";
      set({ error: msg });
    }
  },

  setTeams: async (projectId, teams) => {
    const disc = get().discussion;
    if (!disc) return;
    try {
      await api.setDiscussionTeams(projectId, disc.id, teams);
      await get().loadActive(projectId);
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to set teams";
      set({ error: msg });
    }
  },

  setTimeout: async (projectId, seconds) => {
    const disc = get().discussion;
    if (!disc) return;
    try {
      await api.setDiscussionTimeout(projectId, disc.id, seconds);
      await get().loadActive(projectId);
    } catch (e) {
      const msg = e instanceof api.ApiError ? e.userMessage : "Failed to set timeout";
      set({ error: msg });
    }
  },
}));

// --- UI Store ---

interface UIState {
  sidebarOpen: boolean;
  activeModal: string | null;
  toasts: Array<{ id: string; message: string; type: "success" | "error" | "info" }>;
  toggleSidebar: () => void;
  openModal: (name: string) => void;
  closeModal: () => void;
  addToast: (message: string, type?: "success" | "error" | "info") => void;
  removeToast: (id: string) => void;
}

export const useUIStore = create<UIState>((set) => ({
  sidebarOpen: true,
  activeModal: null,
  toasts: [],

  toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
  openModal: (name) => set({ activeModal: name }),
  closeModal: () => set({ activeModal: null }),

  addToast: (message, type = "info") => {
    const id = `toast-${Date.now()}`;
    set((s) => ({ toasts: [...s.toasts, { id, message, type }] }));
    // Errors persist longer (10s) for screen reader users; info/success auto-dismiss at 5s
    const duration = type === "error" ? 10000 : 5000;
    setTimeout(() => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), duration);
  },

  removeToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));
