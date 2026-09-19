//! Claude invocation: print mode, no persistence, no tools, no MCP servers.
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn claude_args(model: Option<&str>, effort: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--no-session-persistence",
        "--tools",
        "",
        "--strict-mcp-config",
        "--output-format",
        "json",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Some(model) = model {
        args.push("--model".into());
        args.push(model.into());
    }
    if let Some(effort) = effort {
        args.push("--effort".into());
        args.push(effort.into());
    }
    args
}

/// The single JSON result object printed by `--output-format json`.
pub fn parse_claude(stdout: &str) -> Result<String, String> {
    let value: Value = stdout
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line.trim()).ok())
        .or_else(|| serde_json::from_str(stdout.trim()).ok())
        .ok_or("E_RUNNER_FAILED")?;
    if value.get("is_error").and_then(Value::as_bool) == Some(true) {
        let text = value.get("result").and_then(Value::as_str).unwrap_or("");
        let lower = text.to_lowercase();
        if lower.contains("login") || lower.contains("log in") || lower.contains("401") {
            return Err("E_RUNNER_AUTH".into());
        }
        return Err("E_RUNNER_FAILED".into());
    }
    value
        .get("result")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "E_RUNNER_EMPTY".into())
}

/// Claude names project folders by replacing every non-alphanumeric character with `-`.
pub fn claude_project_slug(dir: &Path) -> String {
    dir.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn has_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            let path = e.path();
            if path.is_dir() {
                has_files(&path)
            } else {
                true
            }
        })
    })
}

/// Removes the empty `projects\<slug>` folder print mode leaves behind.
pub fn remove_project_leftover(cwd: &Path) {
    let root = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".claude")));
    let Some(root) = root else { return };
    remove_leftover_in(&root, cwd);
}

pub fn remove_leftover_in(claude_root: &Path, cwd: &Path) {
    let mut candidates = vec![cwd.to_path_buf()];
    if let Ok(real) = std::fs::canonicalize(cwd) {
        candidates.push(real);
    }
    for dir in candidates {
        let project = claude_root.join("projects").join(claude_project_slug(&dir));
        if project.is_dir() && !has_files(&project) {
            let _ = std::fs::remove_dir_all(&project);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_disable_tools_and_persistence() {
        let args = claude_args(Some("sonnet"), Some("low"));
        assert_eq!(
            args,
            vec![
                "-p",
                "--no-session-persistence",
                "--tools",
                "",
                "--strict-mcp-config",
                "--output-format",
                "json",
                "--model",
                "sonnet",
                "--effort",
                "low"
            ]
        );
        assert_eq!(claude_args(None, None).len(), 7);
    }

    #[test]
    fn results_parse() {
        assert_eq!(
            parse_claude(r#"{"type":"result","is_error":false,"result":"5"}"#).unwrap(),
            "5"
        );
        assert_eq!(
            parse_claude(r#"{"is_error":true,"result":"Please run /login"}"#).unwrap_err(),
            "E_RUNNER_AUTH"
        );
        assert_eq!(
            parse_claude(r#"{"is_error":true,"result":"overloaded"}"#).unwrap_err(),
            "E_RUNNER_FAILED"
        );
        assert_eq!(parse_claude("garbage").unwrap_err(), "E_RUNNER_FAILED");
    }

    #[test]
    fn slug_matches_claude_naming() {
        assert_eq!(
            claude_project_slug(Path::new(
                r"C:\Users\simpl\AppData\Local\Temp\tmp.vBAHrRWx5Z"
            )),
            "C--Users-simpl-AppData-Local-Temp-tmp-vBAHrRWx5Z"
        );
    }

    #[test]
    fn only_empty_leftovers_are_removed() {
        let root = tempfile::tempdir().unwrap();
        let cwd = Path::new(r"C:\x\run.1");
        let empty = root.path().join("projects").join("C--x-run-1");
        std::fs::create_dir_all(empty.join("memory")).unwrap();
        remove_leftover_in(root.path(), cwd);
        assert!(!empty.exists());

        std::fs::create_dir_all(&empty).unwrap();
        std::fs::write(empty.join("s.jsonl"), b"{}").unwrap();
        remove_leftover_in(root.path(), cwd);
        assert!(empty.exists(), "folders with files stay");
    }
}
