import { useEffect, useRef } from "react";
import { useApp } from "../../stores/app";
import { cx } from "../../components/ui";
import type { LogLine } from "../../types";

const STREAM_STYLE: Record<string, string> = {
  tool: "text-fg/85",
  claude: "text-fg/60",
  validation: "text-muted",
  stderr: "text-err/80",
  result: "text-ok/80",
  system: "text-dim",
  stdout: "text-muted",
};

/** Lines worth showing as "what Claude is doing": tool calls and short notes. */
function interesting(l: LogLine): boolean {
  if (l.stream === "claude") return !/^TASKKILN_CHECKPOINT_/.test(l.line.trim()) && l.line.trim().length > 0;
  return l.stream !== "system";
}

export function ActivityFeed({ taskId, live, rows = 7 }: { taskId: string; live: boolean; rows?: number }) {
  const lines = useApp((s) => s.live[taskId]);
  const primeTask = useApp((s) => s.primeTask);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void primeTask(taskId);
  }, [taskId, primeTask]);

  const shown = (lines ?? []).filter(interesting).slice(-rows);
  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "nearest" });
  }, [shown.length]);

  if (shown.length === 0) {
    return <div className="font-mono text-[12px] text-dim">{live ? "> waiting for Claude output…" : "> no output recorded"}</div>;
  }
  return (
    <div className="space-y-0.5 font-mono text-[12px] leading-[1.55]">
      {shown.map((l, i) => (
        <div
          key={l.id}
          className={cx(
            "truncate",
            STREAM_STYLE[l.stream] ?? "text-muted",
            live && i === shown.length - 1 && "tk-cursor",
          )}
          title={l.line}
        >
          <span className="text-dim">&gt; </span>
          {l.line}
        </div>
      ))}
      <div ref={endRef} />
    </div>
  );
}
