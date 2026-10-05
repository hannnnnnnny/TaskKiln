import { useState } from "react";
import { Button, Field, Modal, Select, TextArea, TextInput } from "../../components/ui";
import { PRIORITY_LABELS, parseCriteria } from "../../lib/format";
import { errorMessage, ipc } from "../../lib/ipc";
import { useApp } from "../../stores/app";
import type { Project, Task } from "../../types";

interface Props {
  open: boolean;
  onClose: () => void;
  projects: Project[];
  /** Edit this task; omit to create a new one. */
  task?: Task;
  defaultProjectId?: string;
}

export function TaskEditor(props: Props) {
  // Remount the form whenever the target changes so state starts fresh.
  if (!props.open) return null;
  return <TaskForm key={props.task?.id ?? "new"} {...props} />;
}

function TaskForm({ onClose, projects, task, defaultProjectId }: Props) {
  const { run, toast } = useApp();
  const [projectId, setProjectId] = useState(task?.project_id ?? defaultProjectId ?? projects[0]?.id ?? "");
  const [title, setTitle] = useState(task?.title ?? "");
  const [description, setDescription] = useState(task?.description ?? "");
  const [criteriaText, setCriteriaText] = useState(task?.acceptance_criteria.join("\n") ?? "");
  const [priority, setPriority] = useState(task?.priority ?? 1);
  const [drafting, setDrafting] = useState(false);
  const [saving, setSaving] = useState(false);

  const criteria = parseCriteria(criteriaText);
  const valid = title.trim().length > 0 && title.trim().length <= 200 && projectId !== "";

  const draft = async () => {
    setDrafting(true);
    try {
      const drafted = await ipc.draftCriteria(title, description);
      setCriteriaText(drafted.join("\n"));
      toast("info", "Claude drafted criteria — review and edit before saving");
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setDrafting(false);
    }
  };

  const save = async () => {
    setSaving(true);
    const body = { title, description, acceptance_criteria: criteria, priority };
    const result = task
      ? await run(() => ipc.updateTask(task.id, body), "Task updated")
      : await run(() => ipc.createTask({ ...body, project_id: projectId }), "Task queued");
    setSaving(false);
    if (result) onClose();
  };

  return (
    <Modal
      open
      title={task ? "Edit task" : "Add task"}
      onClose={onClose}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={!valid || saving} onClick={save}>
            {task ? "Save changes" : "Add to queue"}
          </Button>
        </>
      }
    >
      <form
        className="space-y-4"
        onSubmit={(e) => {
          e.preventDefault();
          if (valid) void save();
        }}
      >
        <div className="grid gap-4 sm:grid-cols-[1fr_140px]">
          <Field label="Project">
            <Select value={projectId} onChange={(e) => setProjectId(e.target.value)} disabled={!!task}>
              {projects.map((p) => (
                <option key={p.id} value={p.id} disabled={!p.path_exists}>
                  {p.name}
                  {p.path_exists ? "" : " (missing)"}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Priority">
            <Select value={priority} onChange={(e) => setPriority(Number(e.target.value))}>
              {PRIORITY_LABELS.map((l, i) => (
                <option key={l} value={i}>
                  {l}
                </option>
              ))}
            </Select>
          </Field>
        </div>
        <Field label="Title">
          <TextInput value={title} maxLength={200} onChange={(e) => setTitle(e.target.value)} placeholder="Add event comments" />
        </Field>
        <Field label="Description" hint="What should change and why. Claude sees this verbatim.">
          <TextArea
            rows={5}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Users should be able to comment on events…"
          />
        </Field>
        <Field
          label="Acceptance criteria"
          hint={
            <span className="flex flex-wrap items-center justify-between gap-2">
              <span>One per line. TaskKiln validates every criterion before advancing the queue.</span>
              <Button size="sm" disabled={!title.trim() || drafting} onClick={draft}>
                {drafting ? "Drafting…" : "Generate with Claude"}
              </Button>
            </span>
          }
        >
          <TextArea
            rows={7}
            value={criteriaText}
            onChange={(e) => setCriteriaText(e.target.value)}
            placeholder={"authenticated users can post comments\ncomments persist after refresh\nbuild succeeds\ntests pass"}
          />
        </Field>
      </form>
    </Modal>
  );
}
