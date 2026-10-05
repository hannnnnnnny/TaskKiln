import { invoke } from "@tauri-apps/api/core";
import type {
  ClaudeStatus,
  NewTask,
  Project,
  Settings,
  Snapshot,
  Task,
  TaskEvent,
  TaskLog,
  TaskUpdate,
  UserAction,
  ValidationCommand,
} from "../types";

/** Backend errors arrive as {kind, message}; anything else is unexpected. */
export function errorMessage(err: unknown): string {
  if (err && typeof err === "object" && "message" in err) {
    const msg = (err as { message: unknown }).message;
    if (typeof msg === "string" && msg.length > 0) return msg;
  }
  if (typeof err === "string" && err.length > 0) return err;
  return "Something went wrong. Check the log for details.";
}

export const ipc = {
  snapshot: () => invoke<Snapshot>("get_snapshot"),
  refreshClaude: () => invoke<ClaudeStatus>("refresh_claude"),

  addProject: (path: string) => invoke<Project>("add_project", { path }),
  removeProject: (id: string) => invoke<void>("remove_project", { id }),
  redetectCommands: (projectId: string) =>
    invoke<ValidationCommand[]>("redetect_commands", { projectId }),
  setCommandFlags: (id: string, approved: boolean, enabled: boolean) =>
    invoke<void>("set_command_flags", { id, approved, enabled }),

  createTask: (input: NewTask) => invoke<Task>("create_task", { input }),
  updateTask: (id: string, input: TaskUpdate) => invoke<Task>("update_task", { id, input }),
  deleteTask: (id: string) => invoke<void>("delete_task", { id }),
  reorderQueue: (ids: string[]) => invoke<void>("reorder_queue", { ids }),
  moveToFront: (id: string) => invoke<void>("move_to_front", { id }),
  draftCriteria: (title: string, description: string) =>
    invoke<string[]>("draft_criteria", { title, description }),

  startQueue: () => invoke<void>("start_queue"),
  pauseQueue: () => invoke<void>("pause_queue"),
  stopTask: (id: string) => invoke<void>("stop_task", { id }),
  taskAction: (id: string, action: UserAction) => invoke<void>("task_action", { id, action }),
  taskLog: (taskId: string) => invoke<TaskLog>("get_task_log", { taskId }),
  recentEvents: () => invoke<TaskEvent[]>("get_recent_events"),

  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  showMainWindow: () => invoke<void>("show_main_window"),
  openProjectFolder: (projectId: string) => invoke<void>("open_project_folder", { projectId }),
  quitApp: () => invoke<void>("quit_app"),
};
