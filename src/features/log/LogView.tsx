import { useEffect, useMemo, useState } from "react";
import { Button, EmptyState, Label, Modal, StateTag, cx } from "../../components/ui";
import { clock, dateTime, formatDuration, validationTone } from "../../lib/format";
import { errorMessage, ipc } from "../../lib/ipc";
import { useApp } from "../../stores/app";
import type { TaskLog } from "../../types";

type Tab = "events" | "output" | "validation";

const EVENT_TONE: Record<string, string> = {
  TASK_COMPLETED: "text-ok",
  VALIDATION_PASS: "text-ok",
  CHECKPOINT_COMPLETED: "text-ok/80",
  VALIDATION_FAIL: "text-err",
  TASK_FAILED: "text-err",
  TASK_ERROR: "text-err",
  VALIDATION_WARNING: "text-warn",
  TASK_INTERRUPTED: "text-warn",
  TASK_PAUSED: "text-warn",
  SESSION_RESUME_FAILED: "text-warn",
  PROGRESS_UNAVAILABLE: "text-warn",
  COMMAND_EXECUTED: "text-clay",
  CLAUDE_STARTED: "text-clay",
};

export function LogView({ taskId, onClose }: { taskId: string | null; onClose: () => void }) {
  const [tab, setTab] = useState<Tab>("events");
  const [log, setLog] = useState<TaskLog | null>(null);
  const [error, setError] = useState<string | null>(null);
  const liveLines = useApp((s) => (taskId ? s.live[taskId] : undefined));
  const liveEvents = useApp((s) => (taskId ? s.liveEvents[taskId] : undefined));
  const task = useApp((s) => s.snapshot?.tasks.find((t) => t.id === taskId));

  useEffect(() => {
    if (!taskId) return;
    setLog(null);
    setError(null);
    ipc.taskLog(taskId).then(setLog, (e) => setError(errorMessage(e)));
  }, [taskId, task?.status]);

  // Merge persisted history with lines streamed since it was loaded.
  const lines = useMemo(() => mergeById(log?.logs ?? [], liveLines ?? []), [log, liveLines]);
  const events = useMemo(() => mergeById(log?.events ?? [], liveEvents ?? []), [log, liveEvents]);

  return (
    <Modal open={taskId !== null} title={`Execution log · ${task?.title ?? ""}`} onClose={onClose} width="max-w-5xl">
      <div className="mb-3 flex gap-1 border-b border-line" role="tablist">
        {(["events", "output", "validation"] as Tab[]).map((t) => (
          <button
            key={t}
            role="tab"
            aria-selected={tab === t}
            onClick={() => setTab(t)}
            className={cx(
              "-mb-px border-b px-3 py-2 font-mono text-[11px] uppercase tracking-wider",
              tab === t ? "border-clay text-fg" : "border-transparent text-muted hover:text-fg",
            )}
          >
            {t}
          </button>
        ))}
      </div>
      {error && <EmptyState title="Could not load log" tone="error">{error}</EmptyState>}
      {!error && !log && <EmptyState title="Loading…" />}
      {log && tab === "events" && (
        <Pre empty="No events recorded.">
          {events.map((e) => (
            <div key={e.id} className="grid grid-cols-[72px_190px_1fr] gap-3">
              <span className="text-dim">{clock(e.created_at)}</span>
              <span className={EVENT_TONE[e.kind] ?? "text-muted"}>{e.kind}</span>
              <span className="break-words text-fg/85">{e.message}</span>
            </div>
          ))}
        </Pre>
      )}
      {log && tab === "output" && (
        <Pre empty="No output recorded.">
          {lines.map((l) => (
            <div key={l.id} className="grid grid-cols-[72px_80px_1fr] gap-3">
              <span className="text-dim">{clock(l.created_at)}</span>
              <span className={l.stream === "stderr" ? "text-err/80" : "text-muted"}>{l.stream}</span>
              <span className="whitespace-pre-wrap break-words text-fg/85">{l.line}</span>
            </div>
          ))}
        </Pre>
      )}
      {log && tab === "validation" && <ValidationHistory log={log} />}
    </Modal>
  );
}

function mergeById<T extends { id: number }>(base: T[], extra: T[]): T[] {
  const seen = new Set(base.map((x) => x.id));
  return [...base, ...extra.filter((x) => !seen.has(x.id))].sort((a, b) => a.id - b.id);
}

function Pre({ children, empty }: { children: React.ReactNode[]; empty: string }) {
  if (children.length === 0) return <EmptyState title={empty} />;
  return (
    <div className="max-h-[56vh] space-y-0.5 overflow-auto border border-line bg-bg p-3 font-mono text-[11.5px] leading-relaxed">
      {children}
    </div>
  );
}

function ValidationHistory({ log }: { log: TaskLog }) {
  if (log.validations.length === 0) return <EmptyState title="Not validated yet" />;
  return (
    <div className="space-y-4">
      {[...log.validations].reverse().map((v) => (
        <div key={v.id} className="border border-line p-3">
          <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
            <StateTag tone={validationTone(v.status)}>{v.status}</StateTag>
            <span className="font-mono text-[11px] text-muted">{dateTime(v.created_at)}</span>
          </div>
          <p className="mb-3 text-[12.5px] text-fg/85">{v.summary}</p>
          <Label className="mb-1">Findings</Label>
          <ul className="mb-3 space-y-1 font-mono text-[11.5px]">
            {v.findings.map((f, i) => (
              <li key={i} className={f.severity === "fail" ? "text-err" : f.severity === "warning" ? "text-warn" : "text-muted"}>
                [{f.severity.toUpperCase()}] {f.source}: {f.message}
              </li>
            ))}
          </ul>
          {v.changed_files.length > 0 && (
            <>
              <Label className="mb-1">Changed files</Label>
              <div className="mb-3 font-mono text-[11.5px] text-fg/80">{v.changed_files.join("  ·  ")}</div>
            </>
          )}
          {v.commands.map((c, i) => (
            <details key={i} className="mb-1">
              <summary className={cx("cursor-pointer font-mono text-[11.5px]", c.success ? "text-ok" : "text-err")}>
                $ {c.program} {c.args.join(" ")} — {c.success ? "passed" : `failed (exit ${c.exit_code ?? "none"})`} · {formatDuration(c.duration_ms)}
              </summary>
              <pre className="mt-1 max-h-60 overflow-auto bg-bg p-2 font-mono text-[11px] text-muted">{c.output_tail || "(no output)"}</pre>
            </details>
          ))}
        </div>
      ))}
      <div className="text-right">
        <Button size="sm" variant="ghost" onClick={() => navigator.clipboard?.writeText(JSON.stringify(log.validations, null, 2))}>
          Copy as JSON
        </Button>
      </div>
    </div>
  );
}
