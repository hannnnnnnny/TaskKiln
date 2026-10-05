import type { ReactNode } from "react";
import { Button, StatusLight, cx } from "../../components/ui";
import { ipc } from "../../lib/ipc";
import { useApp } from "../../stores/app";
import type { Snapshot } from "../../types";
import type { Tone } from "../../lib/format";
import { pickAndAddProject } from "../projects/ProjectsPanel";
import { claudeLabel, claudeState, claudeTone } from "./claudeState";

function Step({ n, title, tone, children }: { n: number; title: string; tone: Tone; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[32px_1fr] gap-3 border-b border-line py-5 last:border-b-0">
      <div className="font-mono text-[12px] text-muted">{String(n).padStart(2, "0")}</div>
      <div>
        <div className="mb-2 flex items-center gap-2 font-mono text-[12px] uppercase tracking-[0.14em]">
          <StatusLight tone={tone} />
          <span className={cx(tone === "ok" && "text-ok", tone === "error" && "text-err", tone === "warn" && "text-warn")}>{title}</span>
        </div>
        <div className="space-y-2 text-[12.5px] text-fg/80">{children}</div>
      </div>
    </div>
  );
}

function Cmd({ children }: { children: string }) {
  return (
    <div className="flex items-center justify-between gap-2 border border-line bg-bg px-2.5 py-1.5 font-mono text-[12px]">
      <code className="break-all">{children}</code>
      <button type="button" className="shrink-0 font-mono text-[10px] uppercase text-muted hover:text-fg" onClick={() => navigator.clipboard?.writeText(children)}>
        copy
      </button>
    </div>
  );
}

export function SetupScreen({ snapshot, onContinue }: { snapshot: Snapshot; onContinue: () => void }) {
  const run = useApp((s) => s.run);
  const claude = snapshot.claude;
  const state = claudeState(claude);
  const isWin = navigator.userAgent.includes("Windows");
  const quoted = claude.path ? (claude.path.includes(" ") ? `"${claude.path}"` : claude.path) : "claude";
  const canContinue = (state === "connected" || state === "signed_out") && snapshot.projects.length > 0;

  return (
    <div className="flex min-h-0 flex-1 items-start justify-center overflow-y-auto p-4 sm:p-10">
      <div className="w-full max-w-2xl border border-line bg-panel">
        <header className="border-b border-line px-5 py-4">
          <div className="font-mono text-[11px] uppercase tracking-[0.2em] text-clay">TaskKiln · system check</div>
          <h1 className="mt-1 text-[18px] font-semibold">Connect Claude Code and choose a project</h1>
        </header>
        <div className="px-5">
          <Step n={1} title={claudeLabel(claude)} tone={claudeTone(claude)}>
            {state === "missing" && (
              <>
                <p>TaskKiln drives the official Claude Code CLI on this machine. It was not found on PATH or in the usual install locations.</p>
                <p>Install it, then press Re-check:</p>
                <Cmd>{isWin ? "irm https://claude.ai/install.ps1 | iex" : "curl -fsSL https://claude.ai/install.sh | bash"}</Cmd>
                <p className="text-muted">Already installed somewhere else? Set a custom path in Settings.</p>
              </>
            )}
            {state === "unsupported" && (
              <p>
                Claude Code {claude.version ?? ""} at <code className="font-mono">{claude.path}</code> lacks flags TaskKiln requires
                (print mode, stream-json output, session ids, resume). Update it with <code className="font-mono">claude update</code>.
              </p>
            )}
            {(state === "connected" || state === "signed_out") && (
              <p className="font-mono text-[11.5px] text-muted">
                {claude.path} · v{claude.version ?? "?"}
              </p>
            )}
            {claude.error && <p className="text-err">{claude.error}</p>}
            <Button size="sm" onClick={() => run(() => ipc.refreshClaude())}>
              Re-check
            </Button>
          </Step>
          <Step
            n={2}
            title={state === "connected" ? "Authentication: signed in" : state === "signed_out" ? "Authentication: not signed in" : "Authentication"}
            tone={state === "connected" ? "ok" : state === "signed_out" ? "warn" : "idle"}
          >
            <p>TaskKiln never handles credentials. It uses Claude Code's own login.</p>
            {state === "signed_out" && (
              <>
                <p>Sign in once from a terminal, then press Re-check:</p>
                <Cmd>{`${isWin && quoted.startsWith('"') ? "& " : ""}${quoted} auth login`}</Cmd>
              </>
            )}
            {state === "connected" && claude.logged_in === null && <p className="text-muted">Auth status could not be read; runs will report it if login is needed.</p>}
          </Step>
          <Step n={3} title={snapshot.projects.length > 0 ? `Projects: ${snapshot.projects.length} selected` : "Select a project"} tone={snapshot.projects.length > 0 ? "ok" : "idle"}>
            <p>Choose a local repository. Claude only works inside the project directory you pick.</p>
            {snapshot.projects.map((p) => (
              <div key={p.id} className="font-mono text-[11.5px] text-muted">
                {p.name} — {p.path}
              </div>
            ))}
            <Button size="sm" variant={snapshot.projects.length ? "default" : "primary"} onClick={() => void pickAndAddProject()}>
              {snapshot.projects.length ? "+ Add another" : "Choose project directory"}
            </Button>
          </Step>
        </div>
        <footer className="flex items-center justify-between gap-3 border-t border-line px-5 py-3">
          <span className="font-mono text-[11px] text-muted">
            {canContinue ? (state === "signed_out" ? "Tasks will fail until Claude Code is signed in." : "Ready.") : "Complete the steps above to continue."}
          </span>
          <Button variant="primary" disabled={!canContinue} onClick={onContinue}>
            Open control panel
          </Button>
        </footer>
      </div>
    </div>
  );
}
