use crate::agents::{process::*, *};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Command;
use std::time::Duration;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AgentProcess {
    pub agent_id: String,
    pub agent: String,
    pub pid: u32,
    pub parent_pid: u32,
    pub process_name: String,
}

#[derive(Serialize, Clone)]
pub struct AgentActivitySnapshot {
    pub scanned_at: String,
    pub processes: Vec<AgentProcess>,
    pub note: String,
}

pub(crate) fn scan_agent_activity() -> Result<AgentActivitySnapshot, String> {
    #[cfg(windows)]
    {
        // ponytail: process names and command lines are used only for classification;
        // raw command lines never cross the IPC boundary because they may contain secrets.
        let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
# Short vendor names must be a whole path segment or executable name, so workspace
# paths such as D:\hermes-demo or .codex data folders are not attributed to an agent.
"#
        .to_string()
            + &patterns_powershell(&process_patterns())
            + r#"
$rows = @(
  foreach ($p in (Get-CimInstance Win32_Process)) {
    $text = "{0} {1}" -f $p.Name, $p.CommandLine
    foreach ($pattern in $patterns) {
      if ($text -match $pattern.Pattern) {
        [pscustomobject]@{
          agent_id = $pattern.Id
          agent = $pattern.Name
          pid = [uint32]$p.ProcessId
          parent_pid = [uint32]$p.ParentProcessId
          process_name = [string]$p.Name
        }
        break
      }
    }
  }
)
$rows | ConvertTo-Json -Compress
"#;

        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ]);
        let output =
            command_output_timeout_named(command, "工作智能体进程扫描", Duration::from_secs(8))?;
        if !output.status.success() {
            return Err(output_all_text(&output));
        }
        let text = output_text(&output);
        let processes = if text.is_empty() {
            Vec::new()
        } else {
            let value: Value =
                serde_json::from_str(&text).map_err(|e| format!("解析进程扫描结果失败：{e}"))?;
            let values = match value {
                Value::Array(items) => items,
                item => vec![item],
            };
            values
                .into_iter()
                .filter_map(|item| serde_json::from_value::<AgentProcess>(item).ok())
                .collect()
        };
        Ok(AgentActivitySnapshot {
            scanned_at: chrono::Local::now().to_rfc3339(),
            processes,
            note: "仅根据进程名称和启动参数识别工作智能体；原始命令行、令牌和文件内容不会返回。"
                .to_string(),
        })
    }

    #[cfg(not(windows))]
    {
        Ok(AgentActivitySnapshot {
            scanned_at: chrono::Local::now().to_rfc3339(),
            processes: Vec::new(),
            note: "当前版本仅支持 Windows 工作智能体进程扫描。".to_string(),
        })
    }
}

/// One `(family, display name, pattern)` row per agent family, in catalog order.
/// Regional editions share their family's process signature.
pub(crate) fn process_patterns() -> Vec<(String, String, String)> {
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for product in PRODUCTS {
        if rows.iter().any(|row| row.0 == product.family) {
            continue;
        }
        let name = product
            .name
            .trim_end_matches(product.edition_label)
            .trim()
            .to_string();
        rows.push((product.family.into(), name, product.process_pattern.into()));
    }
    rows
}

fn patterns_powershell(rows: &[(String, String, String)]) -> String {
    let quote = |value: &str| value.replace('\'', "''");
    let body = rows
        .iter()
        .map(|(id, name, pattern)| {
            format!(
                "  @{{ Id = '{}'; Name = '{}'; Pattern = '{}' }}",
                quote(id),
                quote(name),
                quote(pattern)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!("$patterns = @(\n{body}\n)\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_pattern_per_family() {
        let patterns = process_patterns();
        let families: std::collections::HashSet<_> =
            patterns.iter().map(|row| row.0.clone()).collect();
        assert_eq!(families.len(), patterns.len());
        assert!(families.contains("workbuddy"));
        assert!(families.contains("trae"));
        assert!(patterns.iter().any(|row| row.1 == "Claude Code"));
        assert!(patterns.iter().any(|row| row.1 == "WorkBuddy"));
    }

    #[test]
    fn powershell_rows_escape_single_quotes() {
        let script = patterns_powershell(&[("x".into(), "X's".into(), "(?i)a'b".into())]);
        assert!(script.contains("Name = 'X''s'"));
        assert!(script.contains("Pattern = '(?i)a''b'"));
    }
}
