import { useState } from "react";
import { Button, Label, Modal, TextArea, cx } from "../../components/ui";
import { confirmAction } from "../../components/confirm";
import { parseCriteria } from "../../lib/format";
import { ipc } from "../../lib/ipc";
import { useApp } from "../../stores/app";
import type { Finding, Task, UserAction, ValidationResult } from "../../types";

interface Props {
  task: Task;
  validation: ValidationResult | undefined;
  /** Another task is active; launching actions must wait. */
  busy: boolean;
}

function headline(task: Task): { title: string; tone: "err" | "warn" } {
  if (task.status === "PAUSED") return { title: "TASK STOPPED", tone: "warn" };
  switch (task.attention_reason) {
    case "INTERRUPTED":
      return { title: "TASK INTERRUPTED", tone: "warn" };
    case "CLAUDE_FAILED":
    case "PLAN_FAILED":
      return { title: "CLAUDE PROCESS FAILED", tone: "err" };
    case "VALIDATION_WARNING":
      return { title: "TASK NEEDS ATTENTION · WARNING", tone: "warn" };
    default:
      return { title: "TASK NEEDS ATTENTION", tone: "err" };
  }
}

function FindingRows({ findings }: { findings: Finding[] }) {
  const relevant = findings.filter((f) => f.severity !== "info");
  if (relevant.length === 0) return null;
  return (
    <ul className="space-y-1.5">
      {relevant.slice(0, 8).map((f, i) => (
        <li key={i} className="grid grid-cols-[72px_1fr] gap-3 font-mono text-[12px]">
          <span className={cx("uppercase", f.severity === "fail" ? "text-err" : "text-warn")}>
            {f.severity === "fail" ? "FAIL" : "WARN"} · {f.source}
          </span>
          <span className="break-words text-fg/90">{f.message}</span>
        </li>
      ))}
      {relevant.length > 8 && <li className="font-mono text-[11px] text-muted">+{relevant.length - 8} more in OPEN LOG → VALIDATION</li>}
    </ul>
  );
}

export function AttentionPanel({ task, validation, busy }: Props) {
  const run = useApp((s) => s.run);
  const [adjusting, setAdjusting] = useState(false);
  const { title, tone } = headline(task);
  const act = (action: UserAction, msg?: string) => run(() => ipc.taskAction(task.id, action), msg);

  const reason = task.attention_reason;
  const isValidation = task.status === "NEEDS_USER" && (reason === "VALIDATION_FAIL" || reason === "VALIDATION_WARNING");
  const canFix = isValidation || (task.status === "NEEDS_USER" && reason === "CLAUDE_FAILED");
  const isRecovery = task.status === "PAUSED" || reason === "INTERRUPTED" || reason === "PLAN_FAILED";

  const ignore = async () => {
    const ok = await confirmAction({
      title: "Ignore validation result?",
      message: "The task will be marked COMPLETED WITH WARNING and the override is recorded in history. The queue then continues with the next task.",
      confirmLabel: "Ignore & continue",
    });
    if (ok) await act({ type: "ignore" }, "Validation overridden");
  };
  const markFailed = async () => {
    const ok = await confirmAction({
      title: "Mark task failed?",
      message: "The task moves to history as FAILED. Changes Claude already made to the project are left as they are.",
      confirmLabel: "Mark failed",
      danger: true,
    });
    if (ok) await act({ type: "mark_failed" });
  };

  return (
    <div
      className={cx("border-l-2 bg-panel-2 p-4", tone === "err" ? "border-err" : "border-warn")}
      role="alert"
    >
      <div className={cx("mb-3 font-mono text-[12px] font-semibold tracking-[0.18em]", tone === "err" ? "text-err" : "text-warn")}>
        ▲ {title}
      </div>
      {task.attention_detail && (
        <div className="mb-3">
          <Label className="mb-1">Detail</Label>
          <p className="whitespace-pre-line text-[12.5px] text-fg/85">{task.attention_detail}</p>
        </div>
      )}
      {validation && isValidation && (
        <div className="mb-4">
          <Label className="mb-1.5">Validation</Label>
          <FindingRows findings={validation.findings} />
        </div>
      )}
      {busy && <p className="mb-2 font-mono text-[11px] text-muted">Another task is running. Actions that start Claude are available once it finishes.</p>}
      <div className="flex flex-wrap gap-2">
        {canFix && (
          <Button variant="primary" disabled={busy} onClick={() => act({ type: "fix" }, "Claude is fixing the findings")}>
            Ask Claude to fix
          </Button>
        )}
        {isValidation && (
          <Button disabled={busy} onClick={() => setAdjusting(true)}>
            Adjust requirement
          </Button>
        )}
        {isValidation && <Button onClick={ignore}>Ignore &amp; continue</Button>}
        {isRecovery && (
          <Button variant="primary" disabled={busy} onClick={() => act({ type: "resume" }, "Resuming task")}>
            Resume
          </Button>
        )}
        {(isRecovery || canFix) && reason !== "PLAN_FAILED" && (
          <Button disabled={busy} onClick={() => act({ type: "retry_validation" }, "Re-running validation")}>
            Retry validation
          </Button>
        )}
        {isRecovery && <Button onClick={() => act({ type: "return_to_queue" }, "Returned to front of queue")}>Return to queue</Button>}
        {(isRecovery || canFix) && (
          <Button variant="danger" onClick={markFailed}>
            Mark failed
          </Button>
        )}
        {task.status === "NEEDS_USER" && (
          <Button variant="ghost" onClick={() => act({ type: "stop_queue" }, "Queue stopped")}>
            Stop queue
          </Button>
        )}
      </div>
      <AdjustRequirement task={task} open={adjusting} onClose={() => setAdjusting(false)} />
    </div>
  );
}

function AdjustRequirement({ task, open, onClose }: { task: Task; open: boolean; onClose: () => void }) {
  const run = useApp((s) => s.run);
  const [text, setText] = useState(task.acceptance_criteria.join("\n"));
  const criteria = parseCriteria(text);
  const submit = async () => {
    await run(() => ipc.taskAction(task.id, { type: "adjust", criteria }), "Claude is reconciling the new requirements");
    onClose();
  };
  return (
    <Modal
      open={open}
      title="Adjust requirement"
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={criteria.length === 0} onClick={submit}>
            Save &amp; ask Claude to reconcile
          </Button>
        </>
      }
    >
      <p className="mb-3 text-[12.5px] text-muted">
        Edit the acceptance criteria (one per line). Claude will reconcile the implementation in the same session, then
        TaskKiln validates again.
      </p>
      <TextArea rows={10} value={text} onChange={(e) => setText(e.target.value)} aria-label="Acceptance criteria" />
    </Modal>
  );
}
