// TEST FIXTURE ONLY. Emulates the Claude Code CLI's `-p --output-format
// stream-json` protocol so the scheduler pipeline can be integration-tested
// without network access. TaskKiln itself never ships or invokes this.
const fs = require("fs");
const path = require("path");

const args = process.argv.slice(2);
const flag = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};

if (args[0] === "--version") {
  console.log("9.9.9 (Claude Code)");
  process.exit(0);
}
if (args[0] === "--help") {
  console.log("-p, --print\n--output-format stream-json\n--session-id <uuid>\n-r, --resume [value]\n--json-schema <schema>\n--permission-prompts <target>\n--tools <tools...>\n--allowedTools, --allowed-tools");
  process.exit(0);
}
if (args[0] === "auth") {
  console.log(JSON.stringify({ loggedIn: true, authMethod: "fake" }));
  process.exit(0);
}

const emit = (obj) => process.stdout.write(JSON.stringify(obj) + "\n");
const session = flag("--session-id") || flag("--resume") || "ephemeral";
const schema = flag("--json-schema") || "";
const text = (t) => emit({ type: "assistant", message: { content: [{ type: "text", text: t }] } });
const tool = (name, input) => emit({ type: "assistant", message: { content: [{ type: "tool_use", name, input }] } });
const result = (structured) =>
  emit({ type: "result", subtype: "success", is_error: false, result: "ok", session_id: session, num_turns: 3, total_cost_usd: 0.01, structured_output: structured, permission_denials: [] });

let prompt = "";
process.stdin.on("data", (d) => (prompt += d));
process.stdin.on("end", () => {
  emit({ type: "system", subtype: "init", session_id: session, model: "fake-model" });
  const cwd = process.cwd();
  if (schema.includes('"checkpoints"')) {
    text("Planning.");
    result({ checkpoints: [{ title: "inspect", weight: 1 }, { title: "implement", weight: 2 }] });
  } else if (schema.includes('"changed_files"')) {
    const title = (prompt.match(/Title: (.*)/) || [])[1] || "task";
    text("TASKKILN_CHECKPOINT_START 1");
    tool("Read", { file_path: "README.md" });
    text("TASKKILN_CHECKPOINT_DONE 1\nTASKKILN_CHECKPOINT_START 2");
    const file = title.replace(/[^a-z0-9]+/gi, "_") + ".txt";
    tool("Write", { file_path: file });
    fs.writeFileSync(path.join(cwd, file), "done by fake claude\n");
    if (prompt.includes("VALIDATION FINDINGS")) fs.writeFileSync(path.join(cwd, "fixed.txt"), "fixed\n");
    text("TASKKILN_CHECKPOINT_DONE 2");
    result({ status: "completed", summary: "implemented " + title, changed_files: [file], tests_run: [], tests_passed: true, remaining_issues: [], checkpoint_results: [{ index: 1, status: "completed" }, { index: 2, status: "completed" }] });
  } else if (schema.includes('"overall"')) {
    const mustFail = prompt.includes("MUST_FAIL") && !fs.existsSync(path.join(cwd, "fixed.txt"));
    result(mustFail
      ? { overall: "fail", summary: "criterion missing", criteria: [{ criterion: "MUST_FAIL", verdict: "not_met", evidence: "fixed.txt absent" }] }
      : { overall: "pass", summary: "all good", criteria: [{ criterion: "works", verdict: "met", evidence: "file written" }] });
  } else {
    result({ criteria: ["it works"] });
  }
});
