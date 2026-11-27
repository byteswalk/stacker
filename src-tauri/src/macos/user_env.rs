//! User-level environment variables on a Mac.
//!
//! Windows keeps them in the registry and hands them to every program started afterwards.
//! A Mac has nothing like it, so Stacker keeps its own file, `~/.config/stacker/env.sh`, and
//! each shell reads it as it starts:
//!
//! - zsh (the default shell) through a marked block in `~/.zshenv`, read by every zsh,
//!   interactive or not, so a tool's `zsh -c` sees the same variables as the terminal;
//! - bash through the login file it actually reads (`.bash_profile`, `.bash_login` or
//!   `.profile`, the first one present), and `.bashrc` when there is one;
//! - fish through a file of Stacker's own in `~/.config/fish/conf.d`, so `config.fish` is
//!   never edited.
//!
//! The block only reads the file when it exists, so deleting the file by hand breaks nothing.
//! Programs started from the Dock or Finder do not run a shell; `launchctl setenv` gives them
//! the variables for the current login session, and Stacker repeats it each time it starts.
//! When the last variable goes, the file, the fish file and every block go with it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const BEGIN: &str = "# >>> Stacker environment >>>";
const END: &str = "# <<< Stacker environment <<<";
const SOURCE_LINE: &str =
    r#"if [ -f "$HOME/.config/stacker/env.sh" ]; then . "$HOME/.config/stacker/env.sh"; fi"#;
const HEADER: &str = "# Variables Stacker sets for you (terminal proxy and others).\n# Change them in Stacker: this file is rewritten on every change.\n";

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn env_file(home: &Path) -> PathBuf {
    home.join(".config").join("stacker").join("env.sh")
}

fn fish_file(home: &Path) -> PathBuf {
    home.join(".config")
        .join("fish")
        .join("conf.d")
        .join("stacker.fish")
}

pub fn get_user_raw(name: &str) -> Option<String> {
    read_all(&home()).remove(name)
}

pub fn set_user(name: &str, value: &str) -> Result<(), String> {
    if !valid_name(name) {
        return Err(format!("环境变量名不合法：{name}"));
    }
    let home = home();
    let mut vars = read_all(&home);
    if vars.get(name).map(String::as_str) == Some(value) {
        return Ok(());
    }
    vars.insert(name.to_string(), value.to_string());
    store(&home, &vars, &login_shell())?;
    launchctl(&["setenv", name, value]);
    Ok(())
}

pub fn remove_user(name: &str) -> Result<(), String> {
    let home = home();
    let mut vars = read_all(&home);
    if vars.remove(name).is_none() {
        return Ok(());
    }
    store(&home, &vars, &login_shell())?;
    launchctl(&["unsetenv", name]);
    Ok(())
}

/// Gives programs started from the Dock the variables again: what `launchctl setenv` set is
/// gone after the user logs out.
pub fn publish_to_session() {
    for (name, value) in read_all(&home()) {
        launchctl(&["setenv", &name, &value]);
    }
}

fn launchctl(args: &[&str]) {
    let _ = std::process::Command::new("/bin/launchctl")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

fn login_shell() -> String {
    std::env::var("SHELL").unwrap_or_default()
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn read_all(home: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(env_file(home))
        .map(|text| parse_sh(&text))
        .unwrap_or_default()
}

/// Writes the variables and puts every hook in place, or takes it all away when none are left.
fn store(home: &Path, vars: &BTreeMap<String, String>, shell: &str) -> Result<(), String> {
    let file = env_file(home);
    let fish = fish_file(home);
    if vars.is_empty() {
        remove_if_present(&file)?;
        // The folder goes too when nothing else of Stacker's is in it.
        if let Some(folder) = file.parent() {
            let _ = std::fs::remove_dir(folder);
        }
        remove_if_present(&fish)?;
        for rc in hook_files(home, shell) {
            unhook(&rc)?;
        }
        return Ok(());
    }
    write_atomic(&file, &render_sh(vars))?;
    if fish
        .parent()
        .and_then(Path::parent)
        .is_some_and(Path::is_dir)
        || shell.ends_with("/fish")
    {
        write_atomic(&fish, &render_fish(vars))?;
    }
    for rc in hook_files(home, shell) {
        hook(&rc)?;
    }
    Ok(())
}

/// The shell start-up files the block goes into, for the shells this user has.
fn hook_files(home: &Path, shell: &str) -> Vec<PathBuf> {
    let mut files = vec![home.join(".zshenv")];
    let login = [".bash_profile", ".bash_login", ".profile"]
        .iter()
        .map(|name| home.join(name))
        .find(|path| path.exists());
    match login {
        Some(path) => files.push(path),
        // Creating `.bash_profile` when `.profile` exists would hide `.profile` from bash, so
        // a new file is made only when bash reads none at all, and only for a bash user.
        None if shell.ends_with("/bash") => files.push(home.join(".bash_profile")),
        None => {}
    }
    let bashrc = home.join(".bashrc");
    if bashrc.exists() {
        files.push(bashrc);
    }
    files
}

fn hook(rc: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(rc).unwrap_or_default();
    let next = add_block(&text);
    if next == text {
        return Ok(());
    }
    crate::backup::backup_file(rc);
    write_through(rc, &next)
}

fn unhook(rc: &Path) -> Result<(), String> {
    let Ok(text) = std::fs::read_to_string(rc) else {
        return Ok(());
    };
    let next = remove_block(&text);
    if next == text {
        return Ok(());
    }
    crate::backup::backup_file(rc);
    if next.trim().is_empty() {
        return remove_if_present(rc);
    }
    write_through(rc, &next)
}

/// Writes in place, so a start-up file that is a link into a dotfiles folder stays a link.
fn write_through(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("写入 {} 失败：{e}", path.display()))
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let error = |e: std::io::Error| format!("写入 {} 失败：{e}", path.display());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(error)?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, text).map_err(error)?;
    std::fs::rename(&temp, path).map_err(error)
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("删除 {} 失败：{e}", path.display()))
        }
        _ => Ok(()),
    }
}

pub(crate) fn add_block(text: &str) -> String {
    if text.contains(BEGIN) {
        return text.to_string();
    }
    let mut out = text.to_string();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(&format!("{BEGIN}\n{SOURCE_LINE}\n{END}\n"));
    out
}

pub(crate) fn remove_block(text: &str) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let Some(start) = lines.iter().position(|line| line.trim_end() == BEGIN) else {
        return text.to_string();
    };
    let Some(end) = lines[start..]
        .iter()
        .position(|line| line.trim_end() == END)
        .map(|at| start + at)
    else {
        return text.to_string();
    };
    // The blank line `add_block` put in front of the block goes with it.
    let from = if start > 0 && lines[start - 1].trim().is_empty() {
        start - 1
    } else {
        start
    };
    let mut out: String = lines[..from].concat();
    out.push_str(&lines[end + 1..].concat());
    out
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn fish_quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', r"\\").replace('\'', r"\'"))
}

pub(crate) fn render_sh(vars: &BTreeMap<String, String>) -> String {
    let mut out = HEADER.to_string();
    for (name, value) in vars {
        out.push_str(&format!("export {name}={}\n", sh_quote(value)));
    }
    out
}

pub(crate) fn render_fish(vars: &BTreeMap<String, String>) -> String {
    let mut out = HEADER.to_string();
    for (name, value) in vars {
        out.push_str(&format!("set -gx {name} {}\n", fish_quote(value)));
    }
    out
}

/// Reads back what `render_sh` wrote: `export NAME='value'`, where a quote inside the value
/// is written `'\''`.
pub(crate) fn parse_sh(text: &str) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    for line in text.lines() {
        let Some((name, mut rest)) = line
            .trim()
            .strip_prefix("export ")
            .and_then(|rest| rest.split_once('='))
        else {
            continue;
        };
        if !valid_name(name) {
            continue;
        }
        let mut value = String::new();
        loop {
            if let Some(quoted) = rest.strip_prefix('\'') {
                let Some(close) = quoted.find('\'') else {
                    break;
                };
                value.push_str(&quoted[..close]);
                rest = &quoted[close + 1..];
            } else if let Some(after) = rest.strip_prefix(r"\'") {
                value.push('\'');
                rest = after;
            } else {
                break;
            }
        }
        vars.insert(name.to_string(), value);
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_come_back_as_they_were_written() {
        let vars = BTreeMap::from([
            (
                "HTTP_PROXY".to_string(),
                "http://127.0.0.1:7890".to_string(),
            ),
            ("ODD".to_string(), "it's a $HOME `x` \\ test".to_string()),
            ("EMPTY".to_string(), String::new()),
        ]);
        assert_eq!(parse_sh(&render_sh(&vars)), vars);
        assert!(render_fish(&vars).contains(r"set -gx ODD 'it\'s a $HOME `x` \\ test'"));
    }

    #[test]
    fn the_block_goes_in_once_and_comes_out_without_a_trace() {
        let original = "export PATH=\"$HOME/bin:$PATH\"\nalias ll='ls -l'";
        let hooked = add_block(original);
        assert!(hooked.contains(SOURCE_LINE));
        assert_eq!(add_block(&hooked), hooked);
        assert_eq!(remove_block(&hooked), format!("{original}\n"));
        assert_eq!(remove_block(original), original);
        // An empty file gets the block alone, and is empty again after.
        assert_eq!(remove_block(&add_block("")), "");
        // What the user put after the block stays.
        let later = format!("{hooked}export EDITOR=vim\n");
        assert_eq!(
            remove_block(&later),
            format!("{original}\nexport EDITOR=vim\n")
        );
    }

    #[test]
    fn bash_gets_the_file_it_reads() {
        let dir = std::env::temp_dir().join(format!("stacker-user-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(hook_files(&dir, "/bin/zsh"), [dir.join(".zshenv")]);
        assert_eq!(
            hook_files(&dir, "/bin/bash"),
            [dir.join(".zshenv"), dir.join(".bash_profile")]
        );
        std::fs::write(dir.join(".profile"), "").unwrap();
        std::fs::write(dir.join(".bashrc"), "").unwrap();
        assert_eq!(
            hook_files(&dir, "/bin/bash"),
            [
                dir.join(".zshenv"),
                dir.join(".profile"),
                dir.join(".bashrc")
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_last_variable_takes_everything_with_it() {
        let dir = std::env::temp_dir().join(format!("stacker-user-env-all-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".config/fish")).unwrap();
        std::fs::write(dir.join(".zshenv"), "export A=1\n").unwrap();
        let vars = BTreeMap::from([("HTTP_PROXY".to_string(), "http://h:1".to_string())]);
        crate::backup::with_test_root(&dir.join("backups"), || store(&dir, &vars, "/bin/zsh"))
            .unwrap();
        assert_eq!(read_all(&dir), vars);
        assert!(fish_file(&dir).is_file());
        assert!(std::fs::read_to_string(dir.join(".zshenv"))
            .unwrap()
            .contains(BEGIN));

        crate::backup::with_test_root(&dir.join("backups"), || {
            store(&dir, &BTreeMap::new(), "/bin/zsh")
        })
        .unwrap();
        assert!(!env_file(&dir).exists() && !fish_file(&dir).exists());
        assert!(!dir.join(".config/stacker").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join(".zshenv")).unwrap(),
            "export A=1\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The real shells read what Stacker wrote: `cargo test --lib user_env -- --ignored`.
    #[test]
    #[ignore]
    fn zsh_and_bash_see_the_variables() {
        let dir = std::env::temp_dir().join(format!("stacker-user-env-sh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let vars = BTreeMap::from([("STACKER_PROBE".to_string(), "a'b c".to_string())]);
        crate::backup::with_test_root(&dir.join("backups"), || store(&dir, &vars, "/bin/bash"))
            .unwrap();
        for (shell, args) in [("/bin/zsh", vec!["-c"]), ("/bin/bash", vec!["-lc"])] {
            let out = std::process::Command::new(shell)
                .args(&args)
                .arg("printf %s \"$STACKER_PROBE\"")
                .env("HOME", &dir)
                .env("ZDOTDIR", &dir)
                .output()
                .unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), "a'b c", "{shell}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
