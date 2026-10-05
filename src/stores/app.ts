import { create } from "zustand";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { errorMessage, ipc } from "../lib/ipc";
import type { LogLine, Snapshot, TaskEvent } from "../types";

const LIVE_LINES = 400;
const REFRESH_DEBOUNCE_MS = 120;

export interface Toast {
  id: number;
  tone: "error" | "ok" | "info";
  message: string;
}

interface AppStore {
  snapshot: Snapshot | null;
  loadError: string | null;
  /** Recent output lines per task, fed by backend `tk://log` events. */
  live: Record<string, LogLine[]>;
  liveEvents: Record<string, TaskEvent[]>;
  toasts: Toast[];
  refresh: () => Promise<void>;
  primeTask: (taskId: string) => Promise<void>;
  /** Run a backend call; surface failures as a toast. Returns success. */
  run: <T>(fn: () => Promise<T>, success?: string) => Promise<T | undefined>;
  toast: (tone: Toast["tone"], message: string) => void;
  dismiss: (id: number) => void;
  connect: () => Promise<UnlistenFn>;
}

let refreshTimer: ReturnType<typeof setTimeout> | null = null;
let toastSeq = 0;

function append<T>(list: T[] | undefined, item: T): T[] {
  const next = [...(list ?? []), item];
  return next.length > LIVE_LINES ? next.slice(next.length - LIVE_LINES) : next;
}

export const useApp = create<AppStore>((set, get) => ({
  snapshot: null,
  loadError: null,
  live: {},
  liveEvents: {},
  toasts: [],

  refresh: async () => {
    try {
      const snapshot = await ipc.snapshot();
      set({ snapshot, loadError: null });
    } catch (e) {
      set({ loadError: errorMessage(e) });
    }
  },

  primeTask: async (taskId) => {
    try {
      const log = await ipc.taskLog(taskId);
      set((s) => ({
        live: { ...s.live, [taskId]: log.logs.slice(-LIVE_LINES) },
        liveEvents: { ...s.liveEvents, [taskId]: log.events.slice(-LIVE_LINES) },
      }));
    } catch (e) {
      get().toast("error", errorMessage(e));
    }
  },

  run: async (fn, success) => {
    try {
      const result = await fn();
      if (success) get().toast("ok", success);
      await get().refresh();
      return result;
    } catch (e) {
      get().toast("error", errorMessage(e));
      await get().refresh();
      return undefined;
    }
  },

  toast: (tone, message) => {
    const id = ++toastSeq;
    set((s) => ({ toasts: [...s.toasts.slice(-3), { id, tone, message }] }));
    setTimeout(() => get().dismiss(id), tone === "error" ? 8000 : 3500);
  },

  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),

  connect: async () => {
    const scheduleRefresh = () => {
      if (refreshTimer) clearTimeout(refreshTimer);
      refreshTimer = setTimeout(() => void get().refresh(), REFRESH_DEBOUNCE_MS);
    };
    const offs = await Promise.all([
      listen("tk://changed", scheduleRefresh),
      listen<LogLine>("tk://log", ({ payload }) =>
        set((s) => ({ live: { ...s.live, [payload.task_id]: append(s.live[payload.task_id], payload) } })),
      ),
      listen<TaskEvent>("tk://event", ({ payload }) => {
        if (!payload.task_id) return scheduleRefresh();
        const tid = payload.task_id;
        set((s) => ({ liveEvents: { ...s.liveEvents, [tid]: append(s.liveEvents[tid], payload) } }));
        scheduleRefresh();
      }),
    ]);
    await get().refresh();
    return () => offs.forEach((off) => off());
  },
}));
