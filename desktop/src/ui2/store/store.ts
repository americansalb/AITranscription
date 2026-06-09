// The single UI2 store — one store, one direction (decree §3.4):
// disk/events → store → components. No component fetches on its own.
import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { deriveFeed, type DerivedFeed } from "./digest";
import { deriveDock } from "./dock";
import { deriveSeatDots } from "./liveness";
import type { DecisionCardState, ParsedProject, SeatDot } from "./types";

interface Ui2State {
  projectDir: string | null;
  project: ParsedProject | null;
  feed: DerivedFeed;
  dock: DecisionCardState[];
  dots: SeatDot[];
  mutedAtId: number | null;
  engineRoomOpen: boolean;
  expandedRows: Set<string>; // in-memory only — never persisted (§4.1)
  error: string | null;

  connect: (dir: string) => Promise<void>;
  refresh: () => Promise<void>;
  toggleRow: (key: string) => void;
  setEngineRoom: (open: boolean) => void;
  toggleMute: () => Promise<void>;
  sendMessage: (to: string, body: string) => Promise<void>;
  resolveCard: (cardId: number, choiceId: string, text: string) => Promise<void>;
}

const EMPTY_FEED: DerivedFeed = {
  rows: [],
  engineOnly: [],
  protocolViolations: 0,
  classified: new Map(),
};

let unlisten: UnlistenFn | null = null;

function rederive(state: Pick<Ui2State, "project" | "mutedAtId">) {
  const messages = state.project?.messages ?? [];
  const feed = deriveFeed(messages, state.mutedAtId);
  const dock = deriveDock(messages, feed.classified);
  const dots = state.project ? deriveSeatDots(state.project, Date.now()) : [];
  return { feed, dock, dots };
}

export const useUi2Store = create<Ui2State>((set, get) => ({
  projectDir: null,
  project: null,
  feed: EMPTY_FEED,
  dock: [],
  dots: [],
  mutedAtId: null,
  engineRoomOpen: false,
  expandedRows: new Set(),
  error: null,

  connect: async (dir) => {
    set({ projectDir: dir, error: null });
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
    unlisten = await listen("project-file-changed", () => void get().refresh());
    await get().refresh();
  },

  refresh: async () => {
    const dir = get().projectDir;
    if (!dir) return;
    try {
      const project = await invoke<ParsedProject | null>("watch_project_dir", { dir });
      if (!project) return;
      set({ project, error: null, ...rederive({ project, mutedAtId: get().mutedAtId }) });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  toggleRow: (key) =>
    set((s) => {
      const next = new Set(s.expandedRows);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return { expandedRows: next };
    }),

  setEngineRoom: (open) => set({ engineRoomOpen: open }),

  toggleMute: async () => {
    const { mutedAtId, project, projectDir } = get();
    if (mutedAtId !== null) {
      // unmute: feed re-derives; accrued traffic appears as one catch-up row
      set((s) => ({ mutedAtId: null, ...rederive({ project: s.project, mutedAtId: null }) }));
      return;
    }
    const lastId = project?.messages.length ? project.messages[project.messages.length - 1].id : 0;
    // experience-first: the feed goes silent NOW, regardless of agent compliance
    set((s) => ({ mutedAtId: lastId, ...rederive({ project: s.project, mutedAtId: lastId }) }));
    if (projectDir) {
      try {
        await invoke("send_team_message", {
          dir: projectDir,
          to: "all",
          subject: "Room muted by human",
          body: "human has muted the room — hold all posts until unmuted.",
          msg_type: "directive",
          metadata: { ui2_mute: true },
        });
      } catch {
        // directive post is best-effort; the local silence already holds
      }
    }
  },

  sendMessage: async (to, body) => {
    const dir = get().projectDir;
    if (!dir || !body.trim()) return;
    await invoke("send_team_message", {
      dir,
      to,
      subject: "",
      body,
      msg_type: "directive",
      metadata: {},
    });
    await get().refresh();
  },

  resolveCard: async (cardId, choiceId, text) => {
    const dir = get().projectDir;
    if (!dir) return;
    await invoke("send_team_message", {
      dir,
      to: "all",
      subject: `Re: #${cardId}`,
      body: text,
      msg_type: "directive",
      metadata: { in_reply_to: cardId, choice_id: choiceId },
    });
    await get().refresh();
  },
}));
