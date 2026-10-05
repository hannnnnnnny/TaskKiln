import type { Task } from "../types";
import { isActive } from "./format";

/** Return a copy of `ids` with the item at `from` moved to index `to`. */
export function moveItem<T>(items: readonly T[], from: number, to: number): T[] {
  if (from === to || from < 0 || from >= items.length) return [...items];
  const target = Math.max(0, Math.min(items.length - 1, to));
  const next = [...items];
  const [moved] = next.splice(from, 1);
  next.splice(target, 0, moved);
  return next;
}

export function queuedTasks(tasks: Task[]): Task[] {
  return tasks
    .filter((t) => t.status === "QUEUED")
    .sort((a, b) => a.queue_position - b.queue_position || a.created_at.localeCompare(b.created_at));
}

/** The task to feature in CURRENT TASK: active first, then one needing the user. */
export function currentTask(tasks: Task[], activeId: string | null): Task | null {
  if (activeId) {
    const active = tasks.find((t) => t.id === activeId);
    if (active) return active;
  }
  return (
    tasks.find((t) => isActive(t.status)) ??
    tasks.find((t) => t.status === "NEEDS_USER") ??
    tasks.find((t) => t.status === "PAUSED") ??
    null
  );
}

/** Tasks needing a decision, excluding the one already featured. */
export function attentionTasks(tasks: Task[], featuredId: string | null): Task[] {
  return tasks.filter((t) => (t.status === "NEEDS_USER" || t.status === "PAUSED") && t.id !== featuredId);
}

/** Finished tasks, newest first. */
export function historyTasks(tasks: Task[]): Task[] {
  return tasks
    .filter((t) => t.status === "COMPLETED" || t.status === "FAILED" || t.status === "CANCELLED")
    .sort((a, b) => (b.completed_at ?? "").localeCompare(a.completed_at ?? ""));
}
