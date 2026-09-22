//! Codex invocation: ephemeral, read-only, no user config, every tool feature disabled.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Tool-bearing features turned off for every run (verified on Codex 0.155.1).
pub const TOOL_FEATURES: &[&str] = &[
    "shell_tool",
    "unified_exec",
    "code_mode_host",
    "multi_agent",
    "apps",
    "plugins",
    "remote_plugin",
    "browser_use",
    "browser_use_external",
    "in_app_browser",
    "computer_use",
    "image_generation",
    "view_image",
    "hooks",
    "goals",
    "skill_search",
    "sleep_tool",
    "tool_suggest",
    "shell_snapshot",
];

/// Feature names from `codex features list`: first column of each line.
pub fn parse_features(list: &str) -> Vec<String> {
    list.lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

/// Only features this Codex version knows; unknown names make the CLI fail.
pub fn applicable(known: &[String]) -> Vec<String> {
    TOOL_FEATURES
        .iter()
        .filter(|f| known.iter().any(|k| k == *f))
        .map(|f| f.to_string())
        .collect()
}

pub fn disabled_features() -> Vec<String> {
    static CACHE: OnceLock<Vec<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let Ok(mut cmd) = crate::sessions::codex_rpc::command() else {
                return Vec::new();
            };
            cmd.args(["features", "list"]);
            crate::sessions::codex_rpc::hidden(&mut cmd);
            match cmd.output() {
                Ok(out) if out.status.success() => {
                    applicable(&parse_features(&String::from_utf8_lossy(&out.stdout)))
                }
                // Fall back to the verified list rather than running with tools on.
                _ => TOOL_FEATURES.iter().map(|f| f.to_string()).collect(),
            }
        })
        .clone()
}

pub fn codex_args(
    tmp: &Path,
    out: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    images: &[PathBuf],
    disable: &[String],
) -> Vec<String> {
    let mut args: Vec<String> = [
        "exec",
        "-",
        "--ephemeral",
        "--skip-git-repo-check",
        "--ignore-rules",
        "--ignore-user-config",
        "-c",
        "web_search=\"disabled\"",
        "-s",
        "read-only",
        "-C",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(tmp.to_string_lossy().into_owned());
    args.push("-o".into());
    args.push(out.to_string_lossy().into_owned());
    if let Some(model) = model {
        args.push("-m".into());
        args.push(model.into());
    }
    if let Some(effort) = effort {
        args.push("-c".into());
        args.push(format!("model_reasoning_effort=\"{effort}\""));
    }
    // Attached images reach the model directly; `view_image` stays disabled.
    for image in images {
        args.push("-i".into());
        args.push(image.to_string_lossy().into_owned());
    }
    for feature in disable {
        args.push("--disable".into());
        args.push(feature.clone());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_are_stateless_and_toolless() {
        let args = codex_args(
            Path::new("T"),
            Path::new("T/out.md"),
            Some("gpt-5.6-sol"),
            Some("low"),
            &[],
            &["shell_tool".into()],
        );
        let joined = args.join(" ");
        assert!(joined.starts_with("exec - --ephemeral --skip-git-repo-check --ignore-rules --ignore-user-config -c web_search=\"disabled\" -s read-only -C T -o T/out.md"));
        assert!(joined.contains("-m gpt-5.6-sol"));
        assert!(joined.contains("-c model_reasoning_effort=\"low\""));
        assert!(joined.ends_with("--disable shell_tool"));
        let bare = codex_args(Path::new("T"), Path::new("o"), None, None, &[], &[]).join(" ");
        assert!(!bare.contains("-m ") && !bare.contains("effort") && !bare.contains("-i "));
    }

    #[test]
    fn images_are_passed_as_files() {
        let images = [
            PathBuf::from(r"T\attachment-1.png"),
            PathBuf::from(r"T\attachment-2.jpg"),
        ];
        let args = codex_args(
            Path::new("T"),
            Path::new("o"),
            None,
            None,
            &images,
            &["view_image".into()],
        );
        let joined = args.join(" ");
        assert!(joined.starts_with("exec - --ephemeral"));
        assert!(
            joined.ends_with(r"-i T\attachment-1.png -i T\attachment-2.jpg --disable view_image")
        );
    }

    #[test]
    fn only_known_features_are_disabled() {
        let list =
            "shell_tool   stable   true\nmulti_agent  stable  true\nsomething_new  stable true\n";
        assert_eq!(
            applicable(&parse_features(list)),
            vec!["shell_tool", "multi_agent"]
        );
    }
}
