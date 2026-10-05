import { useState } from "react";
import { Button, EmptyState, IconButton, Panel, StatusLight, cx } from "../../components/ui";
import { confirmAction } from "../../components/confirm";
import { priorityLabel, taskCode } from "../../lib/format";
import { ipc } from "../../lib/ipc";
import { moveItem, queuedTasks } from "../../lib/queue";
import { useApp } from "../../stores/app";
import type { Snapshot, Task } from "../../types";

const PRIORITY_CLS = ["text-dim", "text-muted", "text-fg", "text-clay"];

export function QueuePanel({
  snapshot,
  onAdd,
  onEdit,
}: {
  snapshot: Snapshot;
  onAdd: () => void;
  onEdit: (task: Task) => void;
}) {
  const run = useApp((s) => s.run);
  const queued = queuedTasks(snapshot.tasks);
  const [dragFrom, setDragFrom] = useState<number | null>(null);
  const [dragOver, setDragOver] = useState<number | null>(null);
  const hasProjects = snapshot.projects.length > 0;

  const reorder = (from: number, to: number) => {
    if (from === to) return;
    const ids = moveItem(queued.map((t) => t.id), from, to);
    void run(() => ipc.reorderQueue(ids));
  };

  const remove = async (t: Task) => {
    const ok = await confirmAction({
      title: "Delete task?",
      message: `"${t.title}" will be removed from the queue. This does not touch any project files.`,
      confirmLabel: "Delete",
      danger: true,
    });
    if (ok) await run(() => ipc.deleteTask(t.id), "Task deleted");
  };

  return (
    <Panel
      title={
        <span className="flex items-center gap-3">
          Queue <span className="text-muted">{queued.length}</span>
        </span>
      }
      right={
        <Button variant="primary" size="sm" onClick={onAdd} disabled={!hasProjects} title={hasProjects ? undefined : "Add a project first"}>
          + Add task
        </Button>
      }
      bodyClassName="overflow-y-auto"
    >
      {queued.length === 0 ? (
        <EmptyState title="Queue empty" action={hasProjects ? <Button size="sm" onClick={onAdd}>+ Add task</Button> : undefined}>
          {hasProjects ? "Queue coding tasks for Claude. They run one at a time, top to bottom." : "Add a project before creating tasks."}
        </EmptyState>
      ) : (
        <ol aria-label="Task queue">
          {queued.map((t, i) => {
            const project = snapshot.projects.find((p) => p.id === t.project_id);
            return (
              <li
                key={t.id}
                draggable
                onDragStart={(e) => {
                  setDragFrom(i);
                  e.dataTransfer.effectAllowed = "move";
                }}
                onDragOver={(e) => {
                  e.preventDefault();
                  setDragOver(i);
                }}
                onDragLeave={() => setDragOver((v) => (v === i ? null : v))}
                onDrop={(e) => {
                  e.preventDefault();
                  if (dragFrom !== null) reorder(dragFrom, i);
                  setDragFrom(null);
                  setDragOver(null);
                }}
                onDragEnd={() => {
                  setDragFrom(null);
                  setDragOver(null);
                }}
                className={cx(
                  "group grid cursor-grab grid-cols-[28px_1fr_auto] items-center gap-3 border-b border-line px-3 py-2.5 hover:bg-panel-2",
                  dragOver === i && dragFrom !== i && "border-t border-t-clay",
                  dragFrom === i && "opacity-40",
                )}
              >
                <span className="font-mono text-[12px] tabular-nums text-muted">{String(i + 1).padStart(2, "0")}</span>
                <div className="min-w-0">
                  <div className="truncate text-[13px]" title={t.title}>
                    {t.title}
                  </div>
                  <div className="mt-0.5 flex flex-wrap items-center gap-x-3 font-mono text-[10.5px] uppercase tracking-wider text-muted">
                    <span>T{taskCode(t, snapshot.tasks)}</span>
                    <span className={cx("normal-case tracking-normal", project && !project.path_exists && "text-err")}>
                      {project?.name ?? "?"}
                      {project && !project.path_exists && " (missing)"}
                    </span>
                    <span className="inline-flex items-center gap-1.5">
                      <StatusLight tone="idle" /> queued
                    </span>
                    <span className={PRIORITY_CLS[t.priority] ?? "text-muted"}>{priorityLabel(t.priority)}</span>
                    <span>{t.acceptance_criteria.length} criteria</span>
                  </div>
                </div>
                <div className="flex items-center opacity-70 group-hover:opacity-100 group-focus-within:opacity-100">
                  <IconButton label={`Move "${t.title}" up`} disabled={i === 0} onClick={() => reorder(i, i - 1)}>
                    ↑
                  </IconButton>
                  <IconButton label={`Move "${t.title}" down`} disabled={i === queued.length - 1} onClick={() => reorder(i, i + 1)}>
                    ↓
                  </IconButton>
                  <IconButton label={`Move "${t.title}" to front`} disabled={i === 0} onClick={() => run(() => ipc.moveToFront(t.id))}>
                    ⤒
                  </IconButton>
                  <IconButton label={`Edit "${t.title}"`} onClick={() => onEdit(t)}>
                    ✎
                  </IconButton>
                  <IconButton label={`Delete "${t.title}"`} onClick={() => remove(t)} className="hover:text-err">
                    ✕
                  </IconButton>
                </div>
              </li>
            );
          })}
        </ol>
      )}
      {queued.length > 0 && snapshot.queue.active_task_id && (
        <p className="px-3 py-2 font-mono text-[10.5px] text-dim">
          Reordering never interrupts the running task; the new order applies when it finishes.
        </p>
      )}
    </Panel>
  );
}
