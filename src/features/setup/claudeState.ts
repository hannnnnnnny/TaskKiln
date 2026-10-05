import type { Tone } from "../../lib/format";
import type { ClaudeStatus } from "../../types";

export type ClaudeState = "missing" | "unsupported" | "signed_out" | "connected";

export function claudeState(c: ClaudeStatus): ClaudeState {
  if (!c.found) return "missing";
  const caps = c.capabilities;
  if (!(caps.print_mode && caps.stream_json && caps.session_id && caps.resume)) return "unsupported";
  if (c.logged_in === false) return "signed_out";
  return "connected";
}

export function claudeLabel(c: ClaudeStatus): string {
  switch (claudeState(c)) {
    case "missing":
      return "CLAUDE CLI: NOT FOUND";
    case "unsupported":
      return "CLAUDE CLI: UNSUPPORTED VERSION";
    case "signed_out":
      return "CLAUDE CLI: NOT SIGNED IN";
    default:
      return "CLAUDE CLI: CONNECTED";
  }
}

export function claudeTone(c: ClaudeStatus): Tone {
  switch (claudeState(c)) {
    case "connected":
      return "ok";
    case "signed_out":
      return "warn";
    default:
      return "error";
  }
}
