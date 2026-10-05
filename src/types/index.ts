// Mirrors the Rust models in src-tauri/src/models. Keep in sync.

export type TaskStatus =
  | "QUEUED"
  | "PLANNING"
  | "RUNNING"
  | "TESTING"
  | "VALIDATING"
  | "NEEDS_USER"
  | "PAUSED"
  | "COMPLETED"
  | "FAILED"
  | "CANCELLED";

export type CheckpointStatus = "PENDING" | "RUNNING" | "COMPLETED" | "FAILED";
export type ValidationStatus = "PASS" | "WARNING" | "FAIL" | "OVERRIDDEN";
export type AttentionReason =
  | "VALIDATION_WARNING"
  | "VALIDATION_FAIL"
  | "INTERRUPTED"
  | "CLAUDE_FAILED"
  | "PLAN_FAILED";

export interface Project {
  id: string;
  name: string;
  path: string;
  created_at: string;
  path_exists: boolean;
  is_git_repo: boolean;
}

export interface Task {
  id: string;
  project_id: string;
  title: string;
  description: string;
  acceptance_criteria: string[];
  priority: number;
  queue_position: number;
  status: TaskStatus;
  progress: number | null;
  created_at: string;
  started_at: string | null;
  completed_at: string | null;
  claude_session_id: string | null;
  validation_status: ValidationStatus | null;
  attention_reason: AttentionReason | null;
  attention_detail: string | null;
  current_activity: string | null;
  fix_attempts: number;
}

export interface Checkpoint {
  id: string;
  task_id: string;
  ordinal: number;
  title: string;
  status: CheckpointStatus;
  weight: number;
  owner: "claude" | "taskkiln";
  started_at: string | null;
  completed_at: string | null;
}

export interface Finding {
  source: string;
  severity: "info" | "warning" | "fail";
  message: string;
}

export interface CommandOutcome {
  kind: string;
  program: string;
  args: string[];
  exit_code: number | null;
  success: boolean;
  duration_ms: number;
  output_tail: string;
}

export interface ValidationResult {
  id: string;
  task_id: string;
  status: ValidationStatus;
  summary: string;
  findings: Finding[];
  commands: CommandOutcome[];
  changed_files: string[];
  created_at: string;
}

export interface ValidationCommand {
  id: string;
  project_id: string;
  kind: "build" | "test" | "lint";
  program: string;
  args: string[];
  source: string;
  approved: boolean;
  enabled: boolean;
}

export interface Settings {
  claude_path: string;
  always_on_top: boolean;
  notifications: boolean;
  auto_run_next: boolean;
  check_build: boolean;
  check_test: boolean;
  check_lint: boolean;
  claude_review: boolean;
  permission_mode: "acceptEdits" | "auto" | "bypassPermissions" | "dontAsk";
  allowed_tools: string;
  disallowed_tools: string;
  model: string;
  show_status_bar: boolean;
}

export interface Capabilities {
  print_mode: boolean;
  stream_json: boolean;
  session_id: boolean;
  resume: boolean;
  json_schema: boolean;
  permission_prompts: boolean;
  allowed_tools: boolean;
  tools: boolean;
}

export interface ClaudeStatus {
  found: boolean;
  path: string | null;
  source: string | null;
  version: string | null;
  capabilities: Capabilities;
  logged_in: boolean | null;
  auth_method: string | null;
  error: string | null;
}

export interface QueueInfo {
  running: boolean;
  active_task_id: string | null;
  alert: string | null;
  queue_complete: boolean;
}

export interface Snapshot {
  projects: Project[];
  tasks: Task[];
  checkpoints: Record<string, Checkpoint[]>;
  validations: Record<string, ValidationResult>;
  commands: Record<string, ValidationCommand[]>;
  queue: QueueInfo;
  settings: Settings;
  claude: ClaudeStatus;
  db_path: string;
  db_error: string | null;
}

export interface TaskEvent {
  id: number;
  task_id: string | null;
  kind: string;
  message: string;
  created_at: string;
}

export interface LogLine {
  id: number;
  task_id: string;
  stream: string;
  line: string;
  created_at: string;
}

export interface TaskLog {
  events: TaskEvent[];
  logs: LogLine[];
  validations: ValidationResult[];
}

export interface NewTask {
  project_id: string;
  title: string;
  description: string;
  acceptance_criteria: string[];
  priority: number;
}

export type TaskUpdate = Omit<NewTask, "project_id">;

export type UserAction =
  | { type: "fix" }
  | { type: "adjust"; criteria: string[] }
  | { type: "ignore" }
  | { type: "stop_queue" }
  | { type: "resume" }
  | { type: "retry_validation" }
  | { type: "mark_failed" }
  | { type: "return_to_queue" }
  | { type: "cancel" };
