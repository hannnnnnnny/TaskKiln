import { cx } from "../../components/ui";
import type { Checkpoint } from "../../types";

const GLYPH: Record<Checkpoint["status"], { g: string; cls: string; label: string }> = {
  COMPLETED: { g: "✓", cls: "text-ok", label: "completed" },
  RUNNING: { g: "●", cls: "text-clay tk-pulse", label: "running" },
  PENDING: { g: "○", cls: "text-dim", label: "pending" },
  FAILED: { g: "✗", cls: "text-err", label: "failed" },
};

export function CheckpointList({ checkpoints }: { checkpoints: Checkpoint[] }) {
  if (checkpoints.length === 0) {
    return <div className="font-mono text-[12px] text-dim">No plan yet — Claude creates checkpoints during PLANNING.</div>;
  }
  return (
    <ol className="space-y-1 font-mono text-[12px]">
      {checkpoints.map((c) => {
        const g = GLYPH[c.status];
        return (
          <li key={c.id} className="flex items-baseline gap-2.5">
            <span className={cx("w-3 text-center", g.cls)} aria-label={g.label}>
              {g.g}
            </span>
            <span className={cx("flex-1", c.status === "PENDING" ? "text-muted" : "text-fg/90")}>{c.title}</span>
            {c.owner === "taskkiln" ? (
              <span className="text-[10px] uppercase tracking-wider text-dim">taskkiln</span>
            ) : (
              <span className="text-[10px] text-dim" title="relative weight">
                w{c.weight}
              </span>
            )}
          </li>
        );
      })}
    </ol>
  );
}
