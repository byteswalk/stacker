use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};

pub fn command() -> Result<Command, String> {
    if let Some(path) = crate::env::resolve_fresh("codex.exe") {
        return Ok(Command::new(path));
    }
    // Run the npm entry point directly; no shell interpolation and no reading credentials.
    for dir in crate::env::fresh_path_dirs() {
        let entry = dir.join("node_modules/@openai/codex/bin/codex.js");
        if entry.is_file() {
            let node = crate::env::resolve_fresh("node.exe").ok_or("E_CODEX_MISSING")?;
            let mut cmd = Command::new(node);
            cmd.arg(entry);
            return Ok(cmd);
        }
    }
    Err("E_CODEX_MISSING".into())
}

pub fn hidden(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
}

pub fn capabilities() -> Result<(), String> {
    let temp = tempfile::tempdir().map_err(err)?;
    let mut cmd = command()?;
    cmd.args([
        "app-server",
        "generate-json-schema",
        "--experimental",
        "--out",
    ])
    .arg(temp.path())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    hidden(&mut cmd);
    let mut child = cmd.spawn().map_err(|_| "E_CODEX_MISSING")?;
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(err)? {
            if !status.success() {
                return Err("E_CODEX_VERSION".into());
            }
            break;
        }
        if started.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("E_TIMEOUT".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let schema: Value = serde_json::from_slice(
        &fs::read(temp.path().join("v2/ThreadListParams.json")).map_err(|_| "E_CODEX_VERSION")?,
    )
    .map_err(err)?;
    if schema.pointer("/properties/ancestorThreadId").is_none()
        || !temp.path().join("v2/ThreadDeleteParams.json").exists()
    {
        return Err("E_CODEX_VERSION".into());
    }
    Ok(())
}

pub fn require_closed() -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("tasklist.exe");
        cmd.args(["/FO", "CSV", "/NH"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        hidden(&mut cmd);
        let output = cmd.output().map_err(|_| "E_PROCESS_CHECK")?;
        if !output.status.success() {
            return Err("E_PROCESS_CHECK".into());
        }
        let text = String::from_utf8_lossy(&output.stdout).to_lowercase();
        if text.lines().any(|l| {
            l.starts_with("\"codex.exe\"")
                || l.starts_with("\"chatgpt (beta).exe\"")
                || l.starts_with("\"codex-code-mode-host.exe\"")
        }) {
            return Err("E_CLOSE_CODEX".into());
        }
    }
    Ok(())
}

pub struct Rpc {
    child: Child,
    input: ChildStdin,
    rx: Receiver<Value>,
    next: u64,
}
impl Drop for Rpc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Rpc {
    pub fn start(source: &Source) -> Result<Self, String> {
        if source.kind != Kind::Codex {
            return Err("E_UNSUPPORTED".into());
        }
        let mut cmd = command()?;
        cmd.args(["app-server", "--stdio"])
            .env("CODEX_HOME", &source.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        hidden(&mut cmd);
        let mut child = cmd.spawn().map_err(|_| "E_CODEX_MISSING")?;
        let input = child.stdin.take().ok_or("E_RPC")?;
        let output = child.stdout.take().ok_or("E_RPC")?;
        let (tx, rx) = mpsc::sync_channel(32);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut line = String::new();
                match std::io::Read::by_ref(&mut reader)
                    .take(32 * 1024 * 1024)
                    .read_line(&mut line)
                {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if v.get("id").is_some() && tx.send(v).is_err() {
                        break;
                    }
                }
            }
        });
        let mut rpc = Self {
            child,
            input,
            rx,
            next: 0,
        };
        rpc.call("initialize",json!({"clientInfo":{"name":"stacker_session_manager","version":"0.3.3"},"capabilities":{"experimentalApi":true}}))?;
        writeln!(rpc.input, "{}", json!({"method":"initialized"})).map_err(err)?;
        Ok(rpc)
    }
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        writeln!(
            self.input,
            "{}",
            json!({"id":id,"method":method,"params":params})
        )
        .map_err(|_| "E_RPC")?;
        self.input.flush().map_err(|_| "E_RPC")?;
        let deadline = std::time::Instant::now() + Duration::from_secs(25);
        loop {
            let v = self
                .rx
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .map_err(|_| "E_TIMEOUT")?;
            if v["id"] != id {
                continue;
            }
            if let Some(e) = v.get("error") {
                log::warn!(target:"stacker::conversations","codex method={} rpc_code={}",method,e["code"]);
                return Err(if e["code"] == -32601 {
                    "E_CODEX_VERSION"
                } else {
                    "E_RPC"
                }
                .into());
            }
            return v.get("result").cloned().ok_or_else(|| "E_RPC".into());
        }
    }
    pub fn descendants(&mut self, id: &str) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        for archived in [false, true] {
            let mut cursor = Value::Null;
            let mut seen = std::collections::HashSet::new();
            loop {
                let result=self.call("thread/list",json!({"ancestorThreadId":id,"archived":archived,"cursor":cursor,"limit":100,"useStateDbOnly":false,
                "modelProviders":[],"sourceKinds":["cli","vscode","exec","appServer","subAgent","subAgentReview","subAgentCompact","subAgentThreadSpawn","subAgentOther","unknown"]}))?;
                let rows = result["data"].as_array().ok_or("E_RPC")?;
                out.extend(rows.iter().cloned());
                cursor = result["nextCursor"].clone();
                if cursor.is_null() {
                    break;
                }
                if out.len() > 10_000 || !seen.insert(cursor.to_string()) {
                    return Err("E_SCAN_LIMIT".into());
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Uses the installed Codex CLI, with an isolated temporary CODEX_HOME only"]
    fn isolated_codex_archive_delete_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let source = Source {
            id: "fixture".into(),
            name: "Fixture".into(),
            kind: Kind::Codex,
            root: reader::display(temp.path()),
            enabled: true,
        };
        let id = "11111111-2222-4333-8444-555555555555";
        let folder = temp.path().join("sessions/2026/01/01");
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join(format!("rollout-2026-01-01T00-00-00-{id}.jsonl"));
        let meta = json!({"timestamp":"2026-01-01T00:00:00Z","type":"session_meta","payload":{"id":id,"timestamp":"2026-01-01T00:00:00Z","cwd":reader::display(temp.path()),"originator":"codex_cli_rs","cli_version":"0.146.0","source":"cli","model_provider":"openai"}});
        let message = json!({"timestamp":"2026-01-01T00:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Synthetic test transcript. No user data."}]}});
        fs::write(&path, format!("{meta}\n{message}\n")).unwrap();
        let child_id = "22222222-2222-4333-8444-555555555555";
        let child_path = folder.join(format!("rollout-2026-01-01T00-00-02-{child_id}.jsonl"));
        let mut child_meta = meta.clone();
        child_meta["payload"]["id"] = json!(child_id);
        child_meta["payload"]["source"] = json!({"subagent":{"thread_spawn":{"parent_thread_id":id,"depth":1,"agent_nickname":"Fixture","agent_role":"explorer"}}});
        fs::write(&child_path, format!("{child_meta}\n{message}\n")).unwrap();
        let mut rpc = Rpc::start(&source).unwrap();
        let initial = rpc
            .call("thread/read", json!({"threadId":id,"includeTurns":false}))
            .unwrap();
        assert_eq!(initial["thread"]["id"], id);
        // A recovered rollout does not necessarily recreate the native spawn ledger.
        // The local relationship parser must therefore contribute to the impact scope.
        let (parent, _) = reader::read(&source, &path, &AtomicBool::new(false)).unwrap();
        let (child, _) = reader::read(&source, &child_path, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            super::super::local_descendants(&[parent.clone(), child], &parent).len(),
            1
        );
        rpc.call("thread/archive", json!({"threadId":child_id}))
            .unwrap();
        rpc.call("thread/archive", json!({"threadId":id})).unwrap();
        assert!(!path.exists());
        assert!(!child_path.exists());
        let archived = rpc
            .call("thread/read", json!({"threadId":id,"includeTurns":false}))
            .unwrap();
        assert!(archived["thread"]["path"]
            .as_str()
            .unwrap()
            .contains("archived_sessions"));
        rpc.call("thread/unarchive", json!({"threadId":id}))
            .unwrap();
        let restored = rpc
            .call("thread/read", json!({"threadId":id,"includeTurns":false}))
            .unwrap();
        let restored_path = PathBuf::from(restored["thread"]["path"].as_str().unwrap());
        assert!(restored_path.exists());
        rpc.call("thread/delete", json!({"threadId":child_id}))
            .unwrap();
        rpc.call("thread/delete", json!({"threadId":id})).unwrap();
        assert!(!restored_path.exists());
        assert!(rpc
            .call(
                "thread/read",
                json!({"threadId":child_id,"includeTurns":false})
            )
            .is_err());
    }
}
