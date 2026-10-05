import type { Task, TaskStatus, ValidationStatus } from "../types";

export const ACTIVE_STATUSES: TaskStatus[] = ["PLANNING", "RUNNING", "TESTING", "VALIDATING"];

export function isActive(status: TaskStatus): boolean {
  return ACTIVE_STATUSES.includes(status);
}

/** Visual tone for a status: drives status-light and label colors. */
export type Tone = "ok" | "busy" | "warn" | "error" | "idle";

export function statusTone(status: TaskStatus): Tone {
  switch (status) {
    case "COMPLETED":
      return "ok";
    case "PLANNING":
    case "RUNNING":
    case "TESTING":
    case "VALIDATING":
      return "busy";
    case "NEEDS_USER":
    case "PAUSED":
      return "warn";
    case "FAILED":
      return "error";
    default:
      return "idle";
  }
}

export function validationTone(v: ValidationStatus | null): Tone {
  switch (v) {
    case "PASS":
      return "ok";
    case "WARNING":
    case "OVERRIDDEN":
      return "warn";
    case "FAIL":
      return "error";
    default:
      return "idle";
  }
}

export function statusLabel(status: TaskStatus): string {
  return status.replace("_", " ");
}

export const PRIORITY_LABELS = ["LOW", "NORMAL", "HIGH", "URGENT"] as const;

export function priorityLabel(p: number): string {
  return PRIORITY_LABELS[p] ?? "NORMAL";
}

/** Text progress bar: █ filled, ░ empty. `null` progress renders as unknown. */
export function blockBar(progress: number | null, width = 24): string {
  if (progress === null || Number.isNaN(progress)) return "·".repeat(width);
  const clamped = Math.min(1, Math.max(0, progress));
  const filled = Math.round(clamped * width);
  return "█".repeat(filled) + "░".repeat(width - filled);
}

export function percent(progress: number | null): string {
  if (progress === null) return "—";
  return `${Math.round(Math.min(1, Math.max(0, progress)) * 100)}%`;
}

export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return "—";
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m`;
  if (m > 0) return `${m}m ${String(sec).padStart(2, "0")}s`;
  return `${sec}s`;
}

export function taskDuration(t: Pick<Task, "started_at" | "completed_at">, now = Date.now()): string {
  if (!t.started_at) return "—";
  const end = t.completed_at ? Date.parse(t.completed_at) : now;
  return formatDuration(end - Date.parse(t.started_at));
}

export function clock(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "--:--:--";
  return d.toLocaleTimeString([], { hour12: false });
}

export function dateTime(iso: string | null): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return `${d.toLocaleDateString([], { month: "short", day: "2-digit" })} ${d.toLocaleTimeString([], { hour12: false, hour: "2-digit", minute: "2-digit" })}`;
}

/** Stable 3-digit task code shown in the UI ("TASK 014"). */
export function taskCode(task: Pick<Task, "created_at" | "id">, all: Pick<Task, "created_at" | "id">[]): string {
  const ordered = [...all].sort((a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id));
  const idx = ordered.findIndex((t) => t.id === task.id);
  return String(idx + 1).padStart(3, "0");
}

/** Parse a textarea of acceptance criteria: one per line, bullets stripped. */
export function parseCriteria(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.replace(/^\s*(?:[-*•]|\d+[.)])\s*/, "").trim())
    .filter((l) => l.length > 0);
}
