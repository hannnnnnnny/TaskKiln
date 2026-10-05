import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Button, EmptyState, StateTag, StatusLight, cx } from "./components/ui";
import { ConfirmHost, Toasts, confirmAction } from "./components/confirm";
import { HistoryPanel } from "./features/history/HistoryPanel";
import { LogView } from "./features/log/LogView";
import { ProjectsPanel } from "./features/projects/ProjectsPanel";
import { QueuePanel } from "./features/queue/QueuePanel";
import { TaskEditor } from "./features/queue/TaskEditor";
import { CurrentTask } from "./features/runner/CurrentTask";
import { SettingsPanel } from "./features/settings/SettingsPanel";
import { SetupScreen } from "./features/setup/SetupScreen";
import { claudeLabel, claudeState, claudeTone } from "./features/setup/claudeState";
import { ipc } from "./lib/ipc";
import { attentionTasks, currentTask, queuedTasks } from "./lib/queue";
import { useApp } from "./stores/app";
import type { Snapshot, Task } from "./types";

type Dialog = { kind: "none" } | { kind: "projects" } | { kind: "settings" } | { kind: "task"; task?: Task };

function Header({ snapshot, onDialog }: { snapshot: Snapshot; onDialog: (d: Dialog) => void }) {
  const run = useApp((s) => s.run);
  const { queue } = snapshot;
  const queued = queuedTasks(snapshot.tasks).length;
  const queueLabel = queue.running ? "RUNNING" : queue.queue_complete ? "COMPLETE" : queue.active_task_id ? "PAUSING" : "PAUSED";
  const queueTone = queue.running ? "busy" : queue.queue_complete ? "ok" : "idle";
  const usable = ["connected", "signed_out"].includes(claudeState(snapshot.claude));

  return (
    <header className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2 border-b border-line bg-panel px-4 py-2">
      <div className="flex items-center gap-5">
        <div className="flex items-baseline gap-2">
          <span className="font-mono text-[14px] font-bold tracking-[0.22em]">
            TASK<span className="text-clay">KILN</span>
          </span>
          <span className="hidden font-mono text-[10px] uppercase tracking-[0.16em] text-dim md:inline">claude code control</span>
        </div>
        <div className="hidden items-center gap-4 sm:flex">
          <StateTag tone={claudeTone(snapshot.claude)}>{claudeLabel(snapshot.claude).replace("CLAUDE CLI: ", "CLI ")}</StateTag>
          <StateTag tone={queueTone}>QUEUE {queueLabel}</StateTag>
          <span className="font-mono text-[11px] text-muted">{queued} QUEUED</span>
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        {queue.running ? (
          <Button onClick={() => run(() => ipc.pauseQueue(), "Queue will pause after the current task")}>❚❚ Pause queue</Button>
        ) : (
          <Button variant="primary" disabled={!usable || queued === 0 || queue.active_task_id !== null} onClick={() => run(() => ipc.startQueue(), "Queue started")}>
            ▶ Start queue
          </Button>
        )}
        <Button variant="ghost" onClick={() => onDialog({ kind: "task" })} disabled={snapshot.projects.length === 0}>
          + Task
        </Button>
        <Button variant="ghost" onClick={() => onDialog({ kind: "projects" })}>
          Projects
        </Button>
        <Button variant="ghost" onClick={() => onDialog({ kind: "settings" })}>
          Settings
        </Button>
      </div>
    </header>
  );
}

function Banners({ snapshot }: { snapshot: Snapshot }) {
  const items: { tone: "err" | "warn"; text: string }[] = [];
  if (snapshot.db_error) items.push({ tone: "err", text: snapshot.db_error });
  if (snapshot.queue.alert) items.push({ tone: "warn", text: snapshot.queue.alert });
  if (claudeState(snapshot.claude) === "signed_out")
    items.push({ tone: "warn", text: "Claude Code is not signed in. Run `claude auth login` in a terminal, then re-check in Settings." });
  if (claudeState(snapshot.claude) === "missing" || claudeState(snapshot.claude) === "unsupported")
    items.push({ tone: "err", text: snapshot.claude.error ?? "Claude Code CLI unavailable. Open Settings to configure it." });
  const missing = snapshot.projects.filter((p) => !p.path_exists);
  if (missing.length) items.push({ tone: "err", text: `Project directory missing: ${missing.map((p) => p.path).join(", ")}` });
  if (items.length === 0) return null;
  return (
    <div className="space-y-px">
      {items.map((b, i) => (
        <div key={i} className={cx("flex items-center gap-2 px-4 py-1.5 font-mono text-[11.5px]", b.tone === "err" ? "bg-err/10 text-err" : "bg-warn/10 text-warn")}>
          <StatusLight tone={b.tone === "err" ? "error" : "warn"} /> {b.text}
        </div>
      ))}
    </div>
  );
}

function OtherAttention({ snapshot, onOpenLog }: { snapshot: Snapshot; onOpenLog: (id: string) => void }) {
  const featured = currentTask(snapshot.tasks, snapshot.queue.active_task_id);
  const others = attentionTasks(snapshot.tasks, featured?.id ?? null);
  if (others.length === 0) return null;
  return (
    <div className="border border-warn/40 bg-panel px-3 py-2">
      <div className="mb-1 font-mono text-[10.5px] uppercase tracking-[0.14em] text-warn">Also waiting on you</div>
      {others.map((t) => (
        <div key={t.id} className="flex items-center justify-between gap-3 py-1 text-[12.5px]">
          <span className="truncate">{t.title}</span>
          <span className="flex shrink-0 items-center gap-2">
            <span className="font-mono text-[10.5px] text-muted">{t.attention_reason ?? t.status}</span>
            <Button size="sm" variant="ghost" onClick={() => onOpenLog(t.id)}>
              Log
            </Button>
          </span>
        </div>
      ))}
      <p className="mt-1 font-mono text-[10.5px] text-dim">These appear in CURRENT TASK once the active task finishes.</p>
    </div>
  );
}

export default function App() {
  const { snapshot, loadError, connect, refresh } = useApp();
  const [dialog, setDialog] = useState<Dialog>({ kind: "none" });
  const [logTask, setLogTask] = useState<string | null>(null);
  const [setupDone, setSetupDone] = useState(false);

  useEffect(() => {
    const off = connect();
    const offQuit = listen("tk://confirm-quit", async () => {
      const ok = await confirmAction({
        title: "Quit TaskKiln?",
        message: "A task is running. Quitting stops Claude Code for that task; it will be flagged as INTERRUPTED next time TaskKiln starts.",
        confirmLabel: "Stop task & quit",
        danger: true,
      });
      if (ok) await ipc.quitApp();
    });
    // Re-check durations and project directories periodically.
    const timer = setInterval(() => void refresh(), 15_000);
    return () => {
      clearInterval(timer);
      void off.then((f) => f());
      void offQuit.then((f) => f());
    };
  }, [connect, refresh]);

  if (!snapshot) {
    return (
      <div className="flex h-full items-center justify-center">
        {loadError ? (
          <EmptyState title="TaskKiln failed to load" tone="error" action={<Button onClick={() => void refresh()}>Retry</Button>}>
            {loadError}
          </EmptyState>
        ) : (
          <EmptyState title="Starting TaskKiln…" />
        )}
      </div>
    );
  }

  const state = claudeState(snapshot.claude);
  const needsSetup = !setupDone && (state === "missing" || state === "unsupported" || snapshot.projects.length === 0);
  const close = () => setDialog({ kind: "none" });

  return (
    <div className="flex h-full flex-col">
      <Header snapshot={snapshot} onDialog={setDialog} />
      <Banners snapshot={snapshot} />
      {needsSetup ? (
        <SetupScreen snapshot={snapshot} onContinue={() => setSetupDone(true)} />
      ) : (
        <main className="grid min-h-0 flex-1 gap-3 overflow-y-auto p-3 lg:grid-cols-[minmax(0,1.15fr)_minmax(340px,1fr)] lg:grid-rows-[minmax(0,1fr)_minmax(180px,34%)] lg:overflow-hidden">
          <div className="flex min-h-0 flex-col gap-3">
            <OtherAttention snapshot={snapshot} onOpenLog={setLogTask} />
            <CurrentTask snapshot={snapshot} onOpenLog={setLogTask} />
          </div>
          <QueuePanel snapshot={snapshot} onAdd={() => setDialog({ kind: "task" })} onEdit={(task) => setDialog({ kind: "task", task })} />
          <div className="min-h-[220px] lg:col-span-2 lg:min-h-0">
            <HistoryPanel snapshot={snapshot} onOpenLog={setLogTask} />
          </div>
        </main>
      )}
      <TaskEditor
        open={dialog.kind === "task"}
        task={dialog.kind === "task" ? dialog.task : undefined}
        projects={snapshot.projects}
        onClose={close}
      />
      <ProjectsPanel open={dialog.kind === "projects"} onClose={close} snapshot={snapshot} />
      <SettingsPanel open={dialog.kind === "settings"} onClose={close} snapshot={snapshot} />
      <LogView taskId={logTask} onClose={() => setLogTask(null)} />
      <ConfirmHost />
      <Toasts />
    </div>
  );
}
