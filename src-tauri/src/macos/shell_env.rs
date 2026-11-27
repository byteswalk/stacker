//! An app opened from Finder or the Dock gets launchd's bare PATH (`/usr/bin:/bin:…`), not
//! the one the user's shell builds; npm, Homebrew and every agent CLI would look missing.
//! At startup Stacker asks the user's own login shell for its PATH and takes it.
//!
//! The same question answers what proxy a new terminal ends up with: another tool (Nexa, a
//! proxy client's "terminal proxy" switch) may set one in `.zshrc`, where Stacker cannot see
//! it by reading its own file.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MARK: &str = "__STACKER_PATH__";
const WAIT: Duration = Duration::from_secs(4);

/// What was printed between two marks, so whatever the profile itself prints is ignored.
/// Empty when the variable is not set; `None` when the marks never came.
fn between_marks(output: &str) -> Option<String> {
    let start = output.find(MARK)? + MARK.len();
    let end = start + output[start..].find(MARK)?;
    Some(output[start..end].trim().to_string())
}

fn login_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| s.starts_with('/'))
        .unwrap_or_else(|| "/bin/zsh".into())
}

/// One variable as a new terminal window would have it: the login shell, interactive, so
/// `.zshrc` runs too. `remove` names variables this process must not hand down, so the
/// answer comes from the shell's own files and not from Stacker's environment.
fn ask_login_shell(name: &str, remove: &[&str], wait: Duration) -> Option<String> {
    let mut command = Command::new(login_shell());
    command
        .args(["-l", "-i", "-c"])
        // printenv, not "$PATH": fish expands that to words, not a colon-separated list.
        .arg(format!(
            "printf '{MARK}'; /usr/bin/printenv {name}; printf '{MARK}'"
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for variable in remove {
        command.env_remove(variable);
    }
    let mut child = command.spawn().ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < wait => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                log::warn!("login shell did not report {name} within {wait:?}");
                return None;
            }
        }
    }
    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    between_marks(&output)
}

/// The login shell's PATH first, then whatever this process already had, each folder once.
pub fn adopt_login_path() {
    let Some(login) = ask_login_shell("PATH", &[], WAIT).filter(|path| !path.is_empty()) else {
        return;
    };
    let current = std::env::var("PATH").unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let merged: Vec<&str> = login
        .split(':')
        .chain(current.split(':'))
        .filter(|dir| !dir.is_empty() && seen.insert(*dir))
        .collect();
    std::env::set_var("PATH", merged.join(":"));
}

const PROXY_VARIABLES: [&str; 8] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
];

/// The proxy a new terminal gets (`HTTP_PROXY`, else `http_proxy`), and the start-up file line
/// it most likely comes from. `None` when the shell could not be asked or sets none. Asked
/// again at most every ten seconds: the proxy page, the tray and the home page all want it.
pub fn terminal_proxy() -> Option<(String, Option<String>)> {
    use std::sync::Mutex;
    type Answer = Option<(String, Option<String>)>;
    static LAST: Mutex<Option<(Instant, Answer)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, answer)) = last.as_ref() {
        if at.elapsed() < Duration::from_secs(10) {
            return answer.clone();
        }
    }
    let wait = Duration::from_secs(3);
    let value = ask_login_shell("HTTP_PROXY", &PROXY_VARIABLES, wait)
        .filter(|v| !v.is_empty())
        .or_else(|| {
            ask_login_shell("http_proxy", &PROXY_VARIABLES, wait).filter(|v| !v.is_empty())
        });
    let home = dirs::home_dir().unwrap_or_default();
    let answer = value.map(|value| (value, proxy_source(&home)));
    *last = Some((Instant::now(), answer.clone()));
    answer
}

/// The start-up files a terminal reads, in the order it reads them.
const START_UP_FILES: [&str; 9] = [
    ".zshenv",
    ".zprofile",
    ".zshrc",
    ".zlogin",
    ".bash_profile",
    ".bash_login",
    ".profile",
    ".bashrc",
    ".config/fish/config.fish",
];

/// `~/.zshrc:1`: the first line that sets a proxy variable, or loads a file whose name says
/// proxy. Stacker's own block is not an answer.
pub(crate) fn proxy_source(home: &std::path::Path) -> Option<String> {
    for name in START_UP_FILES {
        let Ok(text) = std::fs::read_to_string(home.join(name)) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            if sets_proxy(line) {
                return Some(format!("~/{name}:{}", index + 1));
            }
        }
    }
    None
}

fn sets_proxy(line: &str) -> bool {
    let code = line.trim();
    if code.starts_with('#') || code.contains(".config/stacker/env.sh") {
        return false;
    }
    let lower = code.to_ascii_lowercase();
    let names_variable = ["http_proxy", "https_proxy", "all_proxy"]
        .iter()
        .any(|name| lower.contains(name));
    // `. file`, `source file`: only when the file it loads is about a proxy (a mirror URL
    // such as rsproxy.cn on an export line is not).
    let loads = lower.starts_with("source ")
        || lower.starts_with(". ")
        || lower.contains("&& . ")
        || lower.contains("&& source ")
        || lower.contains("then . ")
        || lower.contains("then source ");
    names_variable || (loads && lower.contains("proxy"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_marked_path_is_taken() {
        let out = format!("Welcome!\n{MARK}/opt/homebrew/bin:/usr/bin{MARK}");
        assert_eq!(
            between_marks(&out).as_deref(),
            Some("/opt/homebrew/bin:/usr/bin")
        );
        assert_eq!(between_marks("no marks"), None);
        assert_eq!(between_marks(&format!("{MARK}{MARK}")).as_deref(), Some(""));
    }

    #[test]
    fn the_line_that_sets_the_proxy_is_found() {
        assert!(sets_proxy(
            r#"[ -f "$HOME/.config/nexa/terminal-proxy.sh" ] && . "$HOME/.config/nexa/terminal-proxy.sh"  # Nexa terminal proxy"#
        ));
        assert!(sets_proxy("export https_proxy=http://127.0.0.1:7890"));
        assert!(sets_proxy("set -gx HTTP_PROXY http://127.0.0.1:7890"));
        assert!(!sets_proxy("export RUSTUP_DIST_SERVER=https://rsproxy.cn"));
        assert!(!sets_proxy("# export http_proxy=http://old"));
        assert!(!sets_proxy(
            r#"if [ -f "$HOME/.config/stacker/env.sh" ]; then . "$HOME/.config/stacker/env.sh"; fi"#
        ));
        assert!(!sets_proxy(". ~/.cargo/env"));

        let dir = std::env::temp_dir().join(format!("stacker-proxy-source-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".zshenv"),
            "export RUSTUP_DIST_SERVER=https://rsproxy.cn\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(".zshrc"),
            "alias ll='ls -l'\nexport ALL_PROXY=socks5://h:1\n",
        )
        .unwrap();
        assert_eq!(proxy_source(&dir).as_deref(), Some("~/.zshrc:2"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
