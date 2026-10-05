import { Button, EmptyState, Label, Panel, StateTag } from "../../components/ui";
import { confirmAction } from "../../components/confirm";
import { blockBar, isActive, percent, statusLabel, statusTone, taskCode, taskDuration } from "../../lib/format";
import { ipc } from "../../lib/ipc";
import { currentTask } from "../../lib/queue";
import { useApp } from "../../stores/app";
import { AttentionPanel } from "../validation/AttentionPanel";
import { ActivityFeed } from "./Activity";
import { CheckpointList } from "./Checkpoints";
import type { Snapshot, Task } from "../../types";

function ProgressReadout({ task, hasPlan }: { task: Task; hasPlan: boolean }) {
  const unavailable = task.progress === null;
  return (
    <div>
      <Label className="mb-1">Progress</Label>
      <div className="flex items-center gap-3 font-mono">
        <span className={unavailable ? "text-dim" : "text-clay"} aria-hidden>
          {blockBar(task.progress, 28)}
        </span>
        <span className="text-[13px] tabular-nums">{percent(task.progress)}</span>
      </div>
      {unavailable && isActive(task.status) && (
        <div className="mt-1 font-mono text-[11px] text-muted">
          {hasPlan ? "Progress unavailable — Claude is not reporting checkpoints" : "Progress unavailable until the plan exists"}
        </div>
      )}
    </div>
  );
}

export function CurrentTask({ snapshot, onOpenLog }: { snapshot: Snapshot; onOpenLog: (taskId: string) => void }) {
  const run = useApp((s) => s.run);
  const { queue } = snapshot;
  const task = currentTask(snapshot.tasks, queue.active_task_id);

  if (!task) {
    const queued = snapshot.tasks.some((t) => t.status === "QUEUED");
    return (
      <Panel title="Current task" className="flex-1">
        {queue.queue_complete ? (
          <EmptyState title="■ Queue complete" tone="ok">
            Every queued task finished and passed validation (or was explicitly overridden).
          </EmptyState>
        ) : (
          <EmptyState title="Idle">
            {queued ? "Tasks are queued. Press START QUEUE to let Claude work through them." : "No task is running. Add a task to the queue to begin."}
          </EmptyState>
        )}
      </Panel>
    );
  }

  const project = snapshot.projects.find((p) => p.id === task.project_id);
  const checkpoints = snapshot.checkpoints[task.id] ?? [];
  const active = isActive(task.status);
  const busyElsewhere = queue.active_task_id !== null && queue.active_task_id !== task.id;

  const stop = async () => {
    const ok = await confirmAction({
      title: "Stop the running task?",
      message:
        "TaskKiln will terminate Claude Code (and any command it started) for this task. Partial file changes stay in the project. The task becomes PAUSED and can be resumed.",
      confirmLabel: "Stop task",
      danger: true,
    });
    if (ok) await run(() => ipc.stopTask(task.id), "Task stopped");
  };

  return (
    <Panel
      title={
        <span className="flex items-center gap-3">
          Current task
          <span className="text-muted">TASK {taskCode(task, snapshot.tasks)}</span>
        </span>
      }
      right={<StateTag tone={statusTone(task.status)}>{statusLabel(task.status)}</StateTag>}
      className="flex-1"
      bodyClassName="overflow-y-auto"
    >
      <div className="tk-readout space-y-5 p-4">
        <div>
          <h3 className="text-[19px] font-semibold leading-snug tracking-tight">{task.title}</h3>
          <div className="mt-1 flex flex-wrap gap-x-4 gap-y-1 font-mono text-[11px] text-muted">
            <span>{project?.name ?? "unknown project"}</span>
            {task.started_at && active && <span>elapsed {taskDuration(task)}</span>}
            {task.fix_attempts > 0 && <span>fix rounds {task.fix_attempts}</span>}
            {task.claude_session_id && <span title={task.claude_session_id}>session {task.claude_session_id.slice(0, 8)}</span>}
          </div>
        </div>

        {(task.status === "NEEDS_USER" || task.status === "PAUSED") && (
          <AttentionPanel task={task} validation={snapshot.validations[task.id]} busy={busyElsewhere} />
        )}

        <ProgressReadout task={task} hasPlan={checkpoints.length > 0} />

        <div>
          <Label className="mb-1.5">Current activity</Label>
          {task.current_activity && active && (
            <div className="mb-1.5 font-mono text-[12px] text-clay">» {task.current_activity}</div>
          )}
          <ActivityFeed taskId={task.id} live={active} />
        </div>

        <div>
          <Label className="mb-1.5">Checkpoints</Label>
          <CheckpointList checkpoints={checkpoints} />
        </div>

        <div className="flex flex-wrap gap-2 border-t border-line pt-4">
          {queue.running ? (
            <Button onClick={() => run(() => ipc.pauseQueue(), "Queue will pause after this task")} title="Finish the current task, then stop">
              Pause queue
            </Button>
          ) : (
            <Button onClick={() => run(() => ipc.startQueue())} disabled={busyElsewhere || task.status === "NEEDS_USER"}>
              Resume queue
            </Button>
          )}
          <Button variant="danger" disabled={!active} onClick={stop}>
            Stop
          </Button>
          <Button disabled={!project?.path_exists} onClick={() => project && run(() => ipc.openProjectFolder(project.id))}>
            Open project
          </Button>
          <Button onClick={() => onOpenLog(task.id)}>Open log</Button>
        </div>
      </div>
    </Panel>
  );
}
