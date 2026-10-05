import { describe, expect, it } from "vitest";
import type { Task } from "../types";
import { blockBar, formatDuration, parseCriteria, percent, statusTone, taskCode } from "./format";
import { attentionTasks, currentTask, historyTasks, moveItem, queuedTasks } from "./queue";
import { errorMessage } from "./ipc";

function task(over: Partial<Task>): Task {
  return {
    id: "t",
    project_id: "p",
    title: "t",
    description: "",
    acceptance_criteria: [],
    priority: 1,
    queue_position: 0,
    status: "QUEUED",
    progress: null,
    created_at: "2026-01-01T00:00:00Z",
    started_at: null,
    completed_at: null,
    claude_session_id: null,
    validation_status: null,
    attention_reason: null,
    attention_detail: null,
    current_activity: null,
    fix_attempts: 0,
    ...over,
  };
}

describe("moveItem", () => {
  it("moves up, down, and to the front", () => {
    expect(moveItem(["a", "b", "c"], 2, 0)).toEqual(["c", "a", "b"]);
    expect(moveItem(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
    expect(moveItem(["a", "b", "c"], 1, 99)).toEqual(["a", "c", "b"]);
  });
  it("ignores invalid indexes without mutating input", () => {
    const src = ["a", "b"];
    expect(moveItem(src, 5, 0)).toEqual(["a", "b"]);
    expect(moveItem(src, 1, 1)).not.toBe(src);
  });
});

describe("progress display", () => {
  it("renders weighted progress honestly", () => {
    expect(blockBar(0.5, 10)).toBe("█████░░░░░");
    expect(blockBar(null, 4)).toBe("····");
    expect(percent(0.678)).toBe("68%");
    expect(percent(null)).toBe("—");
    expect(percent(1.7)).toBe("100%");
  });
});

describe("formatting", () => {
  it("formats durations", () => {
    expect(formatDuration(5_000)).toBe("5s");
    expect(formatDuration(65_000)).toBe("1m 05s");
    expect(formatDuration(3_723_000)).toBe("1h 02m");
    expect(formatDuration(-1)).toBe("—");
  });
  it("parses criteria lines and strips bullets", () => {
    expect(parseCriteria("- comments persist\n\n2. tests pass\n  • build ok  ")).toEqual([
      "comments persist",
      "tests pass",
      "build ok",
    ]);
  });
  it("maps statuses to tones", () => {
    expect(statusTone("COMPLETED")).toBe("ok");
    expect(statusTone("RUNNING")).toBe("busy");
    expect(statusTone("NEEDS_USER")).toBe("warn");
    expect(statusTone("FAILED")).toBe("error");
  });
  it("numbers tasks by creation order", () => {
    const a = task({ id: "a", created_at: "2026-01-01T00:00:00Z" });
    const b = task({ id: "b", created_at: "2026-01-02T00:00:00Z" });
    expect(taskCode(b, [b, a])).toBe("002");
  });
  it("extracts backend error messages", () => {
    expect(errorMessage({ kind: "conflict", message: "busy" })).toBe("busy");
    expect(errorMessage(new Error("boom"))).toBe("boom");
    expect(errorMessage(undefined)).toMatch(/went wrong/);
  });
});

describe("queue selectors", () => {
  const tasks = [
    task({ id: "q2", queue_position: 2 }),
    task({ id: "q1", queue_position: 1 }),
    task({ id: "run", status: "RUNNING" }),
    task({ id: "need", status: "NEEDS_USER" }),
    task({ id: "done1", status: "COMPLETED", completed_at: "2026-01-01T00:00:00Z" }),
    task({ id: "done2", status: "FAILED", completed_at: "2026-01-03T00:00:00Z" }),
  ];
  it("orders queued tasks by position", () => {
    expect(queuedTasks(tasks).map((t) => t.id)).toEqual(["q1", "q2"]);
  });
  it("features the active task, else one needing attention", () => {
    expect(currentTask(tasks, "run")?.id).toBe("run");
    expect(currentTask(tasks.filter((t) => t.id !== "run"), null)?.id).toBe("need");
    expect(currentTask([task({})], null)).toBeNull();
  });
  it("lists other tasks needing attention", () => {
    expect(attentionTasks(tasks, "run").map((t) => t.id)).toEqual(["need"]);
    expect(attentionTasks(tasks, "need")).toEqual([]);
  });
  it("lists history newest first", () => {
    expect(historyTasks(tasks).map((t) => t.id)).toEqual(["done2", "done1"]);
  });
});
