import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { StatusLight, cx, toneText } from "../../components/ui";
import { blockBar, percent, statusLabel, statusTone } from "../../lib/format";
import { ipc } from "../../lib/ipc";
import { currentTask, queuedTasks } from "../../lib/queue";
import { useApp } from "../../stores/app";
import { claudeState, claudeTone } from "../setup/claudeState";

/** Compact always-on-top window. Click to open the control panel; drag the header to move. */
export function StatusBar() {
  const { snapshot, connect, primeTask } = useApp();
  const live = useApp((s) => s.live);

  useEffect(() => {
    document.body.classList.add("bar-window");
    const off = connect();
    return () => void off.then((f) => f());
  }, [connect]);

  const task = snapshot ? currentTask(snapshot.tasks, snapshot.queue.active_task_id) : null;
  useEffect(() => {
    if (task) void primeTask(task.id);
  }, [task?.id, primeTask]);

  if (!snapshot) {
    return <div className="flex h-screen items-center px-3 font-mono text-[11px] text-muted">TASKKILN · connecting…</div>;
  }

  const claude = claudeState(snapshot.claude);
  const queued = queuedTasks(snapshot.tasks).length;
  const tone = task ? statusTone(task.status) : snapshot.queue.queue_complete ? "ok" : "idle";
  const idleState = { connected: "IDLE", signed_out: "NOT SIGNED IN", missing: "CLI NOT FOUND", unsupported: "CLI UNSUPPORTED" }[claude];
  const state = task ? statusLabel(task.status) : snapshot.queue.queue_complete ? "QUEUE COMPLETE" : idleState;
  const lastLine = task ? (live[task.id] ?? []).filter((l) => l.stream === "tool" || l.stream === "validation").at(-1)?.line : undefined;
  const activity = task?.current_activity ?? lastLine ?? (task ? task.attention_detail ?? "" : queued ? `${queued} task(s) queued` : "no tasks queued");

  return (
    <div className="flex h-screen select-none flex-col border border-line-strong bg-bg font-mono text-[11px]">
      <div data-tauri-drag-region className="flex h-6 cursor-move items-center justify-between border-b border-line px-2.5">
        <span data-tauri-drag-region className="flex items-center gap-2">
          <StatusLight tone={claudeTone(snapshot.claude)} title={`Claude CLI ${claude}`} />
          <span data-tauri-drag-region className="tracking-[0.14em] text-muted">CLAUDE /</span>
          <span data-tauri-drag-region className={cx("tracking-[0.14em]", toneText(tone))}>{state}</span>
        </span>
        <span data-tauri-drag-region className="flex items-center gap-3 text-muted">
          <span>Q {queued}</span>
          {task && <span className="text-fg">{percent(task.progress)}</span>}
          <button
            type="button"
            aria-label="Hide status bar"
            className="text-dim hover:text-fg"
            onClick={() => void getCurrentWindow().hide()}
          >
            ✕
          </button>
        </span>
      </div>
      <button
        type="button"
        className="flex flex-1 flex-col justify-center gap-1 px-2.5 text-left hover:bg-panel"
        onClick={() => void ipc.showMainWindow()}
        title="Open TaskKiln control panel"
      >
        <span className={cx("leading-none", task ? "text-clay" : "text-dim")}>{blockBar(task?.progress ?? (task ? null : 0), 34)}</span>
        <span className="w-full truncate text-fg/85">
          {task ? <span className="text-fg">{task.title}</span> : "TaskKiln"}
          <span className="text-muted"> · {activity}</span>
        </span>
      </button>
    </div>
  );
}
