use crate::agents::{process::*, *};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::hash::{Hash, Hasher};
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

pub(crate) fn scan_agent_environment() -> Result<AgentEnvironmentSnapshot, String> {
    #[cfg(windows)]
    {
        let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$names = @(
  foreach ($scope in @('User', 'Machine')) {
    [Environment]::GetEnvironmentVariables($scope).Keys | ForEach-Object { [string]$_ }
  }
)
$names | Sort-Object -Unique | ConvertTo-Json -Compress
"#;
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ]);
        let output = command_output_timeout_named(
            command,
            "agent environment scan",
            Duration::from_secs(8),
        )?;
        if !output.status.success() {
            return Err(output_all_text(&output));
        }
        let text = output_text(&output);
        let value: Value = if text.is_empty() {
            Value::Array(Vec::new())
        } else {
            serde_json::from_str(&text)
                .map_err(|e| format!("failed to parse environment scan result: {e}"))?
        };
        let mut names = match value {
            Value::Array(items) => items
                .into_iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect::<Vec<_>>(),
            Value::String(name) => vec![name],
            _ => Vec::new(),
        };
        names.sort_unstable();
        names.dedup();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        names.hash(&mut hasher);
        Ok(AgentEnvironmentSnapshot {
            variable_count: names.len() as u32,
            fingerprint: format!("{:016x}", hasher.finish()),
        })
    }

    #[cfg(not(windows))]
    {
        Ok(AgentEnvironmentSnapshot {
            variable_count: 0,
            fingerprint: String::new(),
        })
    }
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
$patterns = @(
  @{ Id = 'claude'; Name = 'Claude Code'; Pattern = '(?i)(^|[\\/\s"])claude([\\/\s".]|$)|@anthropic-ai[\\/]claude-code' },
  @{ Id = 'codex'; Name = 'Codex'; Pattern = '(?i)(^|[\\/\s"])codex([\\/\s".]|$)|@openai[\\/]codex' },
  @{ Id = 'antigravity'; Name = 'Antigravity'; Pattern = '(?i)antigravity|(^|[\\/])agy(\\.cmd|\\.exe)?' },
  @{ Id = 'opencode'; Name = 'OpenCode'; Pattern = '(?i)opencode' },
  @{ Id = 'zcode'; Name = 'ZCode'; Pattern = '(?i)zcode|z\.ai' },
  @{ Id = 'kimi'; Name = 'Kimi Code'; Pattern = '(?i)(^|[\\/\s"])kimi(-cli|-code)?([\\/\s".]|$)' },
  @{ Id = 'workbuddy'; Name = 'WorkBuddy'; Pattern = '(?i)workbuddy' },
  @{ Id = 'qoder'; Name = 'Qoder'; Pattern = '(?i)(^|[\\/\s"])qoder([\\/\s".]|$)' },
  @{ Id = 'trae-work'; Name = 'TRAE Work'; Pattern = '(?i)(^|[\\/\s"])trae([\\/\s".]|$)' },
  @{ Id = 'deepseek-harness'; Name = 'DeepSeek Harness'; Pattern = '(?i)deepseek-harness|@deepseek-ai[\\/]dsh|(^|[\\/])dsh(\.cmd|\.exe)?' },
  @{ Id = 'openclaw'; Name = 'OpenClaw'; Pattern = '(?i)openclaw' },
  @{ Id = 'hermes'; Name = 'Hermes'; Pattern = '(?i)(^|[\\/\s"])hermes(-agent)?([\\/\s".]|$)' }
)
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
            script,
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

pub(crate) fn desktop_agent_processes(id: &str) -> Result<Vec<AgentProcess>, String> {
    managed_agent(id)?;
    let family = activity_family(id);
    Ok(scan_agent_activity()?
        .processes
        .into_iter()
        .filter(|process| process.agent_id == family && is_probable_desktop_process(process))
        .collect())
}

/// The process scan reports one id per vendor; regional product variants share it.
pub(crate) fn activity_family(id: &str) -> &str {
    match id {
        "qoder-cn" => "qoder",
        "trae-global" => "trae-work",
        other => other,
    }
}

pub(crate) fn is_probable_desktop_process(process: &AgentProcess) -> bool {
    let name = process
        .process_name
        .trim_end_matches(".exe")
        .to_ascii_lowercase();
    !matches!(
        name.as_str(),
        "node"
            | "cmd"
            | "powershell"
            | "pwsh"
            | "conhost"
            | "windowsterminal"
            | "wt"
            | "bash"
            | "sh"
            | "python"
            | "pythonw"
    )
}
