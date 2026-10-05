import { Button, EmptyState, Panel, StateTag, cx } from "../../components/ui";
import { dateTime, statusLabel, statusTone, taskCode, taskDuration, validationTone } from "../../lib/format";
import { ipc } from "../../lib/ipc";
import { historyTasks } from "../../lib/queue";
import { useApp } from "../../stores/app";
import type { Snapshot, Task } from "../../types";

function resultLabel(t: Task): string {
  if (t.validation_status === "OVERRIDDEN") return "COMPLETED W/ WARNING";
  return statusLabel(t.status);
}

export function HistoryPanel({ snapshot, onOpenLog }: { snapshot: Snapshot; onOpenLog: (id: string) => void }) {
  const run = useApp((s) => s.run);
  const tasks = historyTasks(snapshot.tasks);
  return (
    <Panel title={<span className="flex items-center gap-3">History <span className="text-muted">{tasks.length}</span></span>} bodyClassName="overflow-auto">
      {tasks.length === 0 ? (
        <EmptyState title="No history yet">Completed, failed, and overridden tasks are recorded here with their validation result.</EmptyState>
      ) : (
        <table className="w-full min-w-[640px] border-collapse text-left">
          <thead>
            <tr className="font-mono text-[10px] uppercase tracking-[0.14em] text-muted">
              {["Task", "Title", "Project", "Result", "Validation", "Duration", "Finished", ""].map((h) => (
                <th key={h} className="border-b border-line px-3 py-2 font-normal">
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {tasks.map((t) => {
              const project = snapshot.projects.find((p) => p.id === t.project_id);
              const tone = t.validation_status === "OVERRIDDEN" ? "warn" : statusTone(t.status);
              return (
                <tr key={t.id} className="border-b border-line/70 hover:bg-panel-2">
                  <td className="px-3 py-2 font-mono text-[11px] text-muted">{taskCode(t, snapshot.tasks)}</td>
                  <td className="max-w-[280px] truncate px-3 py-2 text-[12.5px]" title={t.title}>
                    {t.title}
                  </td>
                  <td className="px-3 py-2 text-[12px] text-muted">{project?.name ?? "—"}</td>
                  <td className="px-3 py-2">
                    <StateTag tone={tone}>{resultLabel(t)}</StateTag>
                  </td>
                  <td className={cx("px-3 py-2 font-mono text-[11px]", t.validation_status ? "" : "text-dim")}>
                    <span className={validationTone(t.validation_status) === "ok" ? "text-ok" : validationTone(t.validation_status) === "error" ? "text-err" : "text-warn"}>
                      {t.validation_status ?? "—"}
                    </span>
                    {t.fix_attempts > 0 && <span className="ml-2 text-muted">· {t.fix_attempts} fix</span>}
                  </td>
                  <td className="px-3 py-2 font-mono text-[11px] tabular-nums text-muted">{taskDuration(t)}</td>
                  <td className="px-3 py-2 font-mono text-[11px] text-muted">{dateTime(t.completed_at)}</td>
                  <td className="whitespace-nowrap px-3 py-1.5 text-right">
                    <Button size="sm" variant="ghost" onClick={() => onOpenLog(t.id)}>
                      Log
                    </Button>
                    {t.status === "FAILED" && (
                      <Button size="sm" variant="ghost" onClick={() => run(() => ipc.taskAction(t.id, { type: "return_to_queue" }), "Requeued")}>
                        Retry
                      </Button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </Panel>
  );
}
