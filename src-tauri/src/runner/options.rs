//! Model and reasoning-effort choices for each runner.
use serde::Serialize;
use serde_json::Value;

pub const CLAUDE_MODELS: &[&str] = &["haiku", "sonnet", "opus", "fable"];
pub const CLAUDE_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
/// `ultra` delegates to sub-agents automatically; summaries never need it.
const EXCLUDED_EFFORTS: &[&str] = &["ultra"];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub id: String,
    pub label: String,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

/// Visible models from Codex's `models_cache.json`.
pub fn codex_models(cache_json: &str) -> Vec<ModelOption> {
    let Ok(value) = serde_json::from_str::<Value>(cache_json) else {
        return Vec::new();
    };
    value
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
                .filter_map(|m| {
                    let id = m.get("slug")?.as_str()?.to_string();
                    let label = m
                        .get("display_name")
                        .and_then(Value::as_str)
                        .unwrap_or(&id)
                        .to_string();
                    let efforts = m
                        .get("supported_reasoning_levels")
                        .and_then(Value::as_array)
                        .map(|levels| {
                            levels
                                .iter()
                                .filter_map(|l| l.get("effort")?.as_str())
                                .filter(|e| !EXCLUDED_EFFORTS.contains(e))
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    let default_effort = m
                        .get("default_reasoning_level")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    Some(ModelOption {
                        id,
                        label,
                        efforts,
                        default_effort,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn union_efforts(models: &[ModelOption]) -> Vec<String> {
    let order = ["minimal", "low", "medium", "high", "xhigh", "max"];
    order
        .iter()
        .filter(|e| models.iter().any(|m| m.efforts.iter().any(|x| x == *e)))
        .map(|e| e.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_cache_lists_visible_models_without_ultra() {
        let cache = r#"{"models":[
            {"slug":"a","display_name":"A","visibility":"list","default_reasoning_level":"medium",
             "supported_reasoning_levels":[{"effort":"low"},{"effort":"high"},{"effort":"ultra"}]},
            {"slug":"hidden","visibility":"hide","supported_reasoning_levels":[]}
        ]}"#;
        let models = codex_models(cache);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].label, "A");
        assert_eq!(models[0].efforts, vec!["low", "high"]);
        assert_eq!(models[0].default_effort.as_deref(), Some("medium"));
        assert_eq!(union_efforts(&models), vec!["low", "high"]);
        assert!(codex_models("not json").is_empty());
    }
}
