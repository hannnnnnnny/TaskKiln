import { useState } from "react";
import { Button, Field, Label, Modal, Select, StateTag, TextArea, TextInput, Toggle } from "../../components/ui";
import { confirmAction } from "../../components/confirm";
import { ipc } from "../../lib/ipc";
import { useApp } from "../../stores/app";
import type { Settings, Snapshot } from "../../types";
import { claudeTone, claudeLabel } from "../setup/claudeState";

export function SettingsPanel({ open, onClose, snapshot }: { open: boolean; onClose: () => void; snapshot: Snapshot }) {
  if (!open) return null;
  return <SettingsForm onClose={onClose} snapshot={snapshot} />;
}

function SettingsForm({ onClose, snapshot }: { onClose: () => void; snapshot: Snapshot }) {
  const run = useApp((s) => s.run);
  const [s, setS] = useState<Settings>(snapshot.settings);
  const [customPath, setCustomPath] = useState(snapshot.settings.claude_path !== "");
  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setS((prev) => ({ ...prev, [k]: v }));
  const claude = snapshot.claude;

  const save = async () => {
    const next = { ...s, claude_path: customPath ? s.claude_path.trim() : "" };
    if (next.permission_mode === "bypassPermissions" && snapshot.settings.permission_mode !== "bypassPermissions") {
      const ok = await confirmAction({
        title: "Bypass all permission checks?",
        message:
          "Claude Code will run every tool — including arbitrary shell commands — without restriction, and the disallowed-tools list no longer protects you. Only use this in a sandboxed environment.",
        confirmLabel: "Use bypass mode",
        danger: true,
      });
      if (!ok) return;
    }
    if (await run(() => ipc.saveSettings(next), "Settings saved")) onClose();
  };

  return (
    <Modal
      open
      title="Settings"
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={save}>
            Save
          </Button>
        </>
      }
    >
      <div className="space-y-6">
        <section>
          <Label className="mb-2">Claude Code CLI</Label>
          <div className="mb-3 space-y-1 border border-line bg-bg p-3 font-mono text-[11.5px]">
            <StateTag tone={claudeTone(claude)}>{claudeLabel(claude)}</StateTag>
            {claude.path && <div className="break-all text-muted">path {claude.path} ({claude.source})</div>}
            {claude.version && <div className="text-muted">version {claude.version}</div>}
            {claude.error && <div className="text-err">{claude.error}</div>}
          </div>
          <div className="mb-2 flex gap-2">
            <Button size="sm" variant={customPath ? "default" : "primary"} onClick={() => setCustomPath(false)}>
              Auto detect
            </Button>
            <Button size="sm" variant={customPath ? "primary" : "default"} onClick={() => setCustomPath(true)}>
              Custom path
            </Button>
            <Button size="sm" variant="ghost" onClick={() => run(() => ipc.refreshClaude(), "Claude CLI re-detected")}>
              Re-detect
            </Button>
          </div>
          {customPath && (
            <TextInput
              value={s.claude_path}
              onChange={(e) => set("claude_path", e.target.value)}
              placeholder={navigator.userAgent.includes("Windows") ? "C:\\Users\\you\\.local\\bin\\claude.exe" : "/usr/local/bin/claude"}
              aria-label="Custom Claude CLI path"
            />
          )}
        </section>

        <section className="divide-y divide-line">
          <Label className="mb-1">Behaviour</Label>
          <Toggle label="Auto-run next task" description="Start the next queued task after a task passes validation." checked={s.auto_run_next} onChange={(v) => set("auto_run_next", v)} />
          <Toggle label="Desktop notifications" description="Task completed, queue complete, validation failed, Claude crashed." checked={s.notifications} onChange={(v) => set("notifications", v)} />
          <Toggle label="Show compact status bar" checked={s.show_status_bar} onChange={(v) => set("show_status_bar", v)} />
          <Toggle label="Status bar always on top" checked={s.always_on_top} onChange={(v) => set("always_on_top", v)} disabled={!s.show_status_bar} />
        </section>

        <section className="divide-y divide-line">
          <Label className="mb-1">Validation</Label>
          <Toggle label="Build checks" checked={s.check_build} onChange={(v) => set("check_build", v)} />
          <Toggle label="Test checks" description="When on and no test command exists, validation reports a WARNING." checked={s.check_test} onChange={(v) => set("check_test", v)} />
          <Toggle label="Lint checks" description="Lint failures are warnings, not failures." checked={s.check_lint} onChange={(v) => set("check_lint", v)} />
          <Toggle label="Claude acceptance review" description="A separate read-only Claude session checks each criterion against the code." checked={s.claude_review} onChange={(v) => set("claude_review", v)} />
        </section>

        <section className="space-y-4">
          <Label>Claude execution</Label>
          <Field label="Permission mode" hint="Runs are non-interactive: anything not permitted is denied and reported as a validation warning.">
            <Select value={s.permission_mode} onChange={(e) => set("permission_mode", e.target.value as Settings["permission_mode"])}>
              <option value="acceptEdits">acceptEdits — file edits allowed, shell limited to allowed tools (recommended)</option>
              <option value="auto">auto — Claude Code's classifier approves safe actions</option>
              <option value="dontAsk">dontAsk — only explicitly allowed tools</option>
              <option value="bypassPermissions">bypassPermissions — no checks (sandbox only)</option>
            </Select>
          </Field>
          <Field label="Allowed tools" hint="Space-separated Claude Code permission rules, e.g. Bash(npm run *).">
            <TextArea rows={3} value={s.allowed_tools} onChange={(e) => set("allowed_tools", e.target.value)} />
          </Field>
          <Field label="Disallowed tools" hint="Always denied, even if allowed above.">
            <TextArea rows={2} value={s.disallowed_tools} onChange={(e) => set("disallowed_tools", e.target.value)} />
          </Field>
          <Field label="Model" hint="Optional. Empty uses Claude Code's default (e.g. sonnet, opus).">
            <TextInput value={s.model} onChange={(e) => set("model", e.target.value)} placeholder="default" />
          </Field>
        </section>

        <section>
          <Label className="mb-1">Theme</Label>
          <div className="font-mono text-[12px] text-muted">Industrial dark (the only theme in V1)</div>
          <Label className="mb-1 mt-4">Data</Label>
          <div className="break-all font-mono text-[11.5px] text-muted">{snapshot.db_path || "in-memory"}</div>
        </section>
      </div>
    </Modal>
  );
}
