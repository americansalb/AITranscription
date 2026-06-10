// The single UI2 store — one store, one direction (decree §3.4):
// disk/events → store → components. No component fetches on its own.
import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { deriveFeed, type CatchupRange, type DerivedFeed } from "./digest";
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
  catchup: CatchupRange | null;
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

// liveness dots must not freeze when file events stop — that is exactly the
// warm-zombie scenario (reviews msg 281 HIGH-1 / msg 282 HIGH). Pure re-derive
// from the cached project on a fixed cadence; no fetch.
const DOT_REFRESH_MS = 30_000;

let unlisten: UnlistenFn | null = null;
let dotTimer: ReturnType<typeof setInterval> | null = null;

function rederive(state: Pick<Ui2State, "project" | "mutedAtId" | "catchup">) {
  const messages = state.project?.messages ?? [];
  const feed = deriveFeed(messages, state.mutedAtId, state.catchup);
  const dock = deriveDock(messages, feed.classified);
  const dots = state.project ? deriveSeatDots(state.project, Date.now()) : [];
  return { feed, dock, dots };
}

async function postDirective(dir: string, subject: string, body: string, metadata: object) {
  await invoke("send_team_message", {
    dir,
    to: "all",
    subject,
    body,
    msg_type: "directive",
    metadata,
  });
}

export const useUi2Store = create<Ui2State>((set, get) => ({
  projectDir: null,
  project: null,
  feed: EMPTY_FEED,
  dock: [],
  dots: [],
  mutedAtId: null,
  catchup: null,
  engineRoomOpen: false,
  expandedRows: new Set(),
  error: null,

  connect: async (dir) => {
    set({ projectDir: dir, error: null });
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
    if (dotTimer) {
      clearInterval(dotTimer);
      dotTimer = null;
    }
    unlisten = await listen("project-file-changed", () => void get().refresh());
    dotTimer = setInterval(() => {
      const project = get().project;
      if (project) set({ dots: deriveSeatDots(project, Date.now()) });
    }, DOT_REFRESH_MS);
    await get().refresh();
  },

  refresh: async () => {
    const { projectDir, mutedAtId, catchup } = get();
    if (!projectDir) return;
    try {
      const project = await invoke<ParsedProject | null>("watch_project_dir", { dir: projectDir });
      if (!project) return;
      set({ project, error: null, ...rederive({ project, mutedAtId, catchup }) });
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
    const lastId = project?.messages.length ? project.messages[project.messages.length - 1].id : 0;

    if (mutedAtId !== null) {
      // unmute: accrued range becomes ONE catch-up row (IA table §2)
      const catchup: CatchupRange | null = lastId > mutedAtId ? { from: mutedAtId + 1, to: lastId } : null;
      set((s) => ({
        mutedAtId: null,
        catchup,
        ...rederive({ project: s.project, mutedAtId: null, catchup }),
      }));
      if (projectDir) {
        try {
          // symmetric directive — without it, compliant agents hold forever
          // (review msg 281 HIGH-2)
          await postDirective(projectDir, "Room unmuted by human", "human has unmuted the room — normal posting may resume.", { ui2_mute: false });
        } catch (e) {
          set({ error: `Unmute notice failed to post: ${String(e)}` });
        }
      }
      return;
    }

    // mute: experience-first — the feed goes silent NOW, regardless of agent compliance
    set((s) => ({
      mutedAtId: lastId,
      catchup: null,
      ...rederive({ project: s.project, mutedAtId: lastId, catchup: null }),
    }));
    if (projectDir) {
      try {
        await postDirective(projectDir, "Room muted by human", "human has muted the room — hold all posts until unmuted.", { ui2_mute: true });
      } catch (e) {
        // local silence already holds; still surface the failed directive
        set({ error: `Mute directive failed to post: ${String(e)}` });
      }
    }
  },

  sendMessage: async (to, body) => {
    const dir = get().projectDir;
    if (!dir || !body.trim()) return;
    try {
      await invoke("send_team_message", {
        dir,
        to,
        subject: "",
        body,
        msg_type: "directive",
        metadata: {},
      });
    } catch (e) {
      // a swallowed send on the human's surface is a trust-killer (msg 281 HIGH-3)
      set({ error: `Send failed: ${String(e)}` });
      throw e;
    }
    await get().refresh();
  },

  resolveCard: async (cardId, choiceId, text) => {
    const dir = get().projectDir;
    if (!dir) return;
    try {
      await invoke("send_team_message", {
        dir,
        to: "all",
        subject: `Re: #${cardId}`,
        body: text,
        msg_type: "directive",
        metadata: { in_reply_to: cardId, choice_id: choiceId },
      });
    } catch (e) {
      set({ error: `Decision failed to send: ${String(e)}` });
      throw e;
    }
    await get().refresh();
  },
}));
