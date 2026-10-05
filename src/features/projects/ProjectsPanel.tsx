import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Button, EmptyState, IconButton, Label, Modal, StatusLight, cx } from "../../components/ui";
import { confirmAction } from "../../components/confirm";
import { errorMessage, ipc } from "../../lib/ipc";
import { useApp } from "../../stores/app";
import type { Project, Snapshot, ValidationCommand } from "../../types";

/** Open the native folder picker and register the chosen project. */
export async function pickAndAddProject(): Promise<Project | undefined> {
  const { run, toast } = useApp.getState();
  let selected: string | string[] | null;
  try {
    selected = await openDialog({ directory: true, multiple: false, title: "Select a project directory" });
  } catch (e) {
    toast("error", errorMessage(e));
    return undefined;
  }
  if (typeof selected !== "string") return undefined;
  return run(() => ipc.addProject(selected as string), "Project added — review its validation commands");
}

function commandLabel(c: ValidationCommand): string {
  return `${c.program} ${c.args.join(" ")}`;
}

function CommandRow({ cmd }: { cmd: ValidationCommand }) {
  const run = useApp((s) => s.run);
  const approve = async () => {
    const ok = await confirmAction({
      title: "Approve validation command?",
      message: `TaskKiln will run this command in the project directory after each task:\n\n  ${commandLabel(cmd)}\n\nDetected from ${cmd.source}. It runs without a shell, with CI=true.`,
      confirmLabel: "Approve",
    });
    if (ok) await run(() => ipc.setCommandFlags(cmd.id, true, cmd.enabled), "Command approved");
  };
  return (
    <li className="grid grid-cols-[52px_1fr_auto] items-center gap-3 py-1.5">
      <span className="font-mono text-[10.5px] uppercase tracking-wider text-muted">{cmd.kind}</span>
      <div className="min-w-0">
        <div className={cx("truncate font-mono text-[12px]", !cmd.enabled && "text-dim line-through")}>{commandLabel(cmd)}</div>
        <div className="font-mono text-[10.5px] text-dim">{cmd.source}</div>
      </div>
      <div className="flex items-center gap-2">
        {cmd.approved ? (
          <span className="inline-flex items-center gap-1.5 font-mono text-[10.5px] text-ok">
            <StatusLight tone="ok" /> APPROVED
          </span>
        ) : (
          <Button size="sm" variant="primary" onClick={approve}>
            Approve
          </Button>
        )}
        <Button size="sm" variant="ghost" onClick={() => run(() => ipc.setCommandFlags(cmd.id, cmd.approved, !cmd.enabled))}>
          {cmd.enabled ? "Disable" : "Enable"}
        </Button>
      </div>
    </li>
  );
}

function ProjectCard({ project, commands }: { project: Project; commands: ValidationCommand[] }) {
  const run = useApp((s) => s.run);
  const remove = async () => {
    const ok = await confirmAction({
      title: "Remove project from TaskKiln?",
      message: `"${project.name}" and its tasks/history will be removed from TaskKiln.\nThe directory on disk is NOT touched.`,
      confirmLabel: "Remove",
      danger: true,
    });
    if (ok) await run(() => ipc.removeProject(project.id), "Project removed");
  };
  const pending = commands.filter((c) => !c.approved && c.enabled).length;
  return (
    <div className="border border-line bg-panel-2 p-3">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <StatusLight tone={project.path_exists ? "ok" : "error"} title={project.path_exists ? "directory present" : "directory missing"} />
            <span className="font-medium">{project.name}</span>
            {project.is_git_repo && <span className="font-mono text-[10px] uppercase tracking-wider text-muted">git</span>}
          </div>
          <div className="mt-0.5 break-all font-mono text-[11px] text-muted">{project.path}</div>
          {!project.path_exists && (
            <div className="mt-1 font-mono text-[11px] text-err">Directory no longer exists. Tasks for this project cannot run.</div>
          )}
          {project.path_exists && !project.is_git_repo && (
            <div className="mt-1 font-mono text-[11px] text-warn">Not a git repository — changed-file detection is unavailable.</div>
          )}
        </div>
        <div className="flex shrink-0 items-center">
          <IconButton label="Re-detect validation commands" disabled={!project.path_exists} onClick={() => run(() => ipc.redetectCommands(project.id), "Commands re-detected")}>
            ⟳
          </IconButton>
          <IconButton label={`Remove ${project.name}`} onClick={remove} className="hover:text-err">
            ✕
          </IconButton>
        </div>
      </div>
      <div className="mt-3 border-t border-line pt-2">
        <Label className="mb-1">
          Validation commands {pending > 0 && <span className="text-warn">· {pending} awaiting approval</span>}
        </Label>
        {commands.length === 0 ? (
          <div className="py-1 font-mono text-[11.5px] text-muted">
            None detected (no package.json scripts, Cargo.toml, pytest config or go.mod). Validation relies on git changes and the Claude review.
          </div>
        ) : (
          <ul>{commands.map((c) => <CommandRow key={c.id} cmd={c} />)}</ul>
        )}
      </div>
    </div>
  );
}

export function ProjectsPanel({ open, onClose, snapshot }: { open: boolean; onClose: () => void; snapshot: Snapshot }) {
  return (
    <Modal
      open={open}
      title="Projects"
      onClose={onClose}
      footer={
        <Button variant="primary" onClick={() => void pickAndAddProject()}>
          + Add project directory
        </Button>
      }
    >
      {snapshot.projects.length === 0 ? (
        <EmptyState title="No projects">Choose a local repository for Claude to work in.</EmptyState>
      ) : (
        <div className="space-y-3">
          {snapshot.projects.map((p) => (
            <ProjectCard key={p.id} project={p} commands={snapshot.commands[p.id] ?? []} />
          ))}
        </div>
      )}
    </Modal>
  );
}
