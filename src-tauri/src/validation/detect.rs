//! Detect build/test/lint commands from project metadata. Only commands that
//! demonstrably exist (a declared script, a manifest) are proposed.

use std::path::Path;

use serde_json::Value;

use crate::models::ValidationCommand;

fn cmd(kind: &str, program: &str, args: &[&str], source: &str) -> ValidationCommand {
    ValidationCommand {
        id: String::new(),
        project_id: String::new(),
        kind: kind.into(),
        program: program.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        source: source.into(),
        approved: false,
        enabled: true,
    }
}

/// npm's placeholder test script, which always fails.
const NPM_PLACEHOLDER_TEST: &str = "no test specified";

pub fn node_package_manager(root: &Path) -> &'static str {
    if root.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if root.join("yarn.lock").exists() {
        "yarn"
    } else if root.join("bun.lockb").exists() || root.join("bun.lock").exists() {
        "bun"
    } else {
        "npm"
    }
}

fn detect_node(root: &Path, out: &mut Vec<ValidationCommand>) {
    let Ok(raw) = std::fs::read_to_string(root.join("package.json")) else { return };
    let Ok(pkg) = serde_json::from_str::<Value>(&raw) else { return };
    let Some(scripts) = pkg.get("scripts").and_then(Value::as_object) else { return };
    let pm = node_package_manager(root);
    for (kind, names) in [("build", &["build"][..]), ("test", &["test"][..]), ("lint", &["lint", "typecheck"][..])] {
        let found = names.iter().find(|n| {
            scripts.get(**n).and_then(Value::as_str).is_some_and(|s| !s.contains(NPM_PLACEHOLDER_TEST))
        });
        if let Some(name) = found {
            out.push(cmd(kind, pm, &["run", name], &format!("package.json#scripts.{name}")));
        }
    }
}

fn detect_rust(root: &Path, out: &mut Vec<ValidationCommand>) {
    if !root.join("Cargo.toml").is_file() {
        return;
    }
    out.push(cmd("build", "cargo", &["build"], "Cargo.toml"));
    out.push(cmd("test", "cargo", &["test"], "Cargo.toml"));
    out.push(cmd("lint", "cargo", &["clippy", "--quiet"], "Cargo.toml"));
}

fn file_contains(path: &Path, needle: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|s| s.contains(needle))
}

fn detect_python(root: &Path, out: &mut Vec<ValidationCommand>) {
    let pyproject = root.join("pyproject.toml");
    let manifests = ["requirements.txt", "requirements-dev.txt", "setup.py", "setup.cfg", "pyproject.toml"];
    if !manifests.iter().any(|m| root.join(m).is_file()) {
        return;
    }
    let uses_pytest = file_contains(&pyproject, "pytest")
        || root.join("pytest.ini").is_file()
        || file_contains(&root.join("setup.cfg"), "[tool:pytest]")
        || file_contains(&root.join("requirements.txt"), "pytest")
        || file_contains(&root.join("requirements-dev.txt"), "pytest");
    if uses_pytest {
        out.push(cmd("test", "python", &["-m", "pytest", "-q"], "pytest configuration"));
    }
    if file_contains(&pyproject, "[tool.ruff") || root.join("ruff.toml").is_file() {
        out.push(cmd("lint", "ruff", &["check", "."], "ruff configuration"));
    }
}

fn detect_go(root: &Path, out: &mut Vec<ValidationCommand>) {
    if root.join("go.mod").is_file() {
        out.push(cmd("build", "go", &["build", "./..."], "go.mod"));
        out.push(cmd("test", "go", &["test", "./..."], "go.mod"));
        out.push(cmd("lint", "go", &["vet", "./..."], "go.mod"));
    }
}

pub fn detect_commands(root: &Path) -> Vec<ValidationCommand> {
    let mut out = vec![];
    detect_node(root, &mut out);
    detect_rust(root, &mut out);
    detect_python(root, &mut out);
    detect_go(root, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn detects_node_scripts_with_package_manager() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "package.json", r#"{"scripts":{"build":"vite build","test":"vitest run","typecheck":"tsc"}}"#);
        write(d.path(), "pnpm-lock.yaml", "");
        let cmds = detect_commands(d.path());
        let rendered: Vec<String> = cmds.iter().map(|c| format!("{} {} {}", c.kind, c.program, c.args.join(" "))).collect();
        assert_eq!(rendered, ["build pnpm run build", "test pnpm run test", "lint pnpm run typecheck"]);
        assert!(cmds.iter().all(|c| !c.approved), "detected commands need user approval");
    }

    #[test]
    fn skips_npm_placeholder_and_missing_scripts() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "package.json", r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#);
        assert!(detect_commands(d.path()).is_empty());
        write(d.path(), "package.json", "not json");
        assert!(detect_commands(d.path()).is_empty());
    }

    #[test]
    fn detects_cargo_and_python() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "Cargo.toml", "[package]");
        write(d.path(), "pyproject.toml", "[tool.pytest.ini_options]\n[tool.ruff]\n");
        let kinds: Vec<String> = detect_commands(d.path()).iter().map(|c| format!("{}:{}", c.kind, c.program)).collect();
        assert_eq!(kinds, ["build:cargo", "test:cargo", "lint:cargo", "test:python", "lint:ruff"]);
    }

    #[test]
    fn python_without_pytest_gets_no_test_command() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "requirements.txt", "flask\n");
        assert!(detect_commands(d.path()).is_empty());
    }

    #[test]
    fn empty_directory_has_no_commands() {
        let d = tempfile::tempdir().unwrap();
        assert!(detect_commands(d.path()).is_empty());
    }
}
