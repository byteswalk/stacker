use super::model::Roots;
use std::path::PathBuf;

fn env(name: &str) -> Option<String> {
    crate::winenv::get_user_raw(name)
        .or_else(|| std::env::var(name).ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn pick(override_value: &str, env_name: Option<&str>, fallback: PathBuf) -> String {
    if !override_value.trim().is_empty() {
        return override_value.trim().to_string();
    }
    env_name
        .and_then(env)
        .unwrap_or_else(|| fallback.to_string_lossy().into_owned())
}

/// Custom setting, then the agent's own environment variable, then its default folder.
pub fn resolve(overrides: &Roots) -> Roots {
    let home = dirs::home_dir().unwrap_or_default();
    let roaming = dirs::data_dir().unwrap_or_default();
    Roots {
        codex: pick(&overrides.codex, Some("CODEX_HOME"), home.join(".codex")),
        claude: pick(
            &overrides.claude,
            Some("CLAUDE_CONFIG_DIR"),
            home.join(".claude"),
        ),
        claude_desktop_index: pick(
            &overrides.claude_desktop_index,
            None,
            roaming.join("Claude").join("claude-code-sessions"),
        ),
        codebuddy: pick(&overrides.codebuddy, None, home.join(".codebuddy")),
        mimo: pick(
            &overrides.mimo,
            None,
            home.join(".local").join("share").join("mimocode"),
        ),
        kimi: pick(&overrides.kimi, None, home.join(".kimi-code")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_win() {
        let roots = resolve(&Roots {
            codex: " D:/codex ".into(),
            ..Default::default()
        });
        assert_eq!(roots.codex, "D:/codex");
        assert!(roots.claude_desktop_index.ends_with("claude-code-sessions"));
    }
}
