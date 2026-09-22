//! One-click update plan: which advertised updates can run in the background, which need
//! the user, and which installs could not be checked at all, each with the reason. A shared
//! CLI appears once.

use super::Surface;
use crate::agents::{VibeSurface, VibeTool};
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub product_id: String,
    pub product_name: String,
    pub surface: Surface,
    pub surface_label: String,
    pub current: Option<String>,
    pub latest: Option<String>,
    pub reason: Option<String>,
    /// The latest-version lookup failed (rather than there being no public source): worth
    /// asking again.
    pub lookup_failed: bool,
}

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePlan {
    pub auto: Vec<PlanItem>,
    pub manual: Vec<PlanItem>,
    /// Installed, but whether an update exists is not known: no public version source, or
    /// the lookup failed. Listed so "everything is up to date" is never claimed for them.
    pub unknown: Vec<PlanItem>,
}

fn item(tool: &VibeTool, surface: Surface, state: &VibeSurface, reason: Option<&str>) -> PlanItem {
    PlanItem {
        product_id: tool.id.clone(),
        product_name: tool.name.clone(),
        surface,
        surface_label: state.label.clone(),
        current: state.version.clone(),
        latest: state.latest.clone(),
        reason: reason.map(Into::into),
        lookup_failed: state.latest_error.is_some(),
    }
}

/// A desktop app that is open cannot be updated in place, and Stacker never closes it for the
/// user: such items move from the background group to the manual one, saying so, instead of
/// failing once started. `is_running` gets the executable's file name.
pub(crate) fn hold_running_apps(
    plan: &mut UpdatePlan,
    tools: &[VibeTool],
    is_running: impl Fn(&str) -> bool,
) {
    let (held, keep): (Vec<_>, Vec<_>) =
        std::mem::take(&mut plan.auto)
            .into_iter()
            .partition(|item| {
                item.surface == Surface::Desktop
                    && tools
                        .iter()
                        .find(|tool| tool.id == item.product_id)
                        .and_then(|tool| tool.desktop.path.as_deref())
                        .and_then(|path| std::path::Path::new(path).file_name()?.to_str())
                        .filter(|name| name.to_ascii_lowercase().ends_with(".exe"))
                        .is_some_and(&is_running)
            });
    plan.auto = keep;
    plan.manual.extend(held.into_iter().map(|mut item| {
        item.reason = Some(format!(
            "{} 正在运行，请先退出应用再更新",
            item.surface_label
        ));
        item
    }));
}

pub(crate) fn build_update_plan(tools: &[VibeTool]) -> UpdatePlan {
    let mut plan = UpdatePlan::default();
    let mut seen_cli = std::collections::HashSet::new();
    for tool in tools {
        for (surface, state) in [(Surface::Cli, &tool.cli), (Surface::Desktop, &tool.desktop)] {
            let unknown = state.installed && state.latest_checked && state.latest.is_none();
            if !state.update_available && !unknown {
                continue;
            }
            if surface == Surface::Cli {
                if let Some(cli_id) = &tool.cli_id {
                    if !seen_cli.insert(cli_id.clone()) {
                        continue;
                    }
                }
            }
            if unknown {
                let reason = match &state.latest_error {
                    Some(error) => format!("查询最新版本失败：{error}"),
                    None => "没有公开的版本号渠道，由它自己检查更新".to_string(),
                };
                plan.unknown.push(item(tool, surface, state, Some(&reason)));
                continue;
            }
            if state.health == "broken" {
                plan.manual
                    .push(item(tool, surface, state, Some("安装已损坏，请先修复")));
            } else if !state.can_update {
                plan.manual
                    .push(item(tool, surface, state, Some("需要在官方下载页手动更新")));
            } else {
                plan.auto.push(item(tool, surface, state, None));
            }
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(id: &str, cli_id: Option<&str>, cli: VibeSurface, desktop: VibeSurface) -> VibeTool {
        let mut tool = crate::agents::test_tool(id);
        tool.cli_id = cli_id.map(Into::into);
        tool.cli = cli;
        tool.desktop = desktop;
        tool
    }

    fn updatable(can_update: bool, health: &str) -> VibeSurface {
        let mut surface = crate::agents::test_surface();
        surface.update_available = true;
        surface.can_update = can_update;
        surface.health = health.into();
        surface.version = Some("1.0".into());
        surface.latest = Some("2.0".into());
        surface
    }

    fn unchecked(error: Option<&str>) -> VibeSurface {
        let mut surface = crate::agents::test_surface();
        surface.installed = true;
        surface.latest_checked = true;
        surface.version = Some("3.12.3".into());
        surface.latest_error = error.map(Into::into);
        surface
    }

    #[test]
    fn an_open_desktop_app_waits_for_the_user_instead_of_failing() {
        let idle = crate::agents::test_surface();
        let mut kimi = updatable(true, "healthy");
        kimi.label = "Kimi Work 桌面端".into();
        kimi.path = Some(r"C:\Users\me\AppData\Local\Programs\Kimi\Kimi.exe".into());
        let mut zcode = updatable(true, "healthy");
        zcode.path = Some(r"D:\AITools\ZCode\ZCode.exe".into());
        let tools = vec![
            tool("kimi", None, idle.clone(), kimi),
            tool("zcode", None, idle, zcode),
        ];
        let mut plan = build_update_plan(&tools);
        hold_running_apps(&mut plan, &tools, |image| image == "Kimi.exe");
        assert_eq!(
            plan.auto
                .iter()
                .map(|i| i.product_id.as_str())
                .collect::<Vec<_>>(),
            ["zcode"]
        );
        assert_eq!(plan.manual.len(), 1);
        assert_eq!(
            plan.manual[0].reason.as_deref(),
            Some("Kimi Work 桌面端 正在运行，请先退出应用再更新")
        );
    }

    #[test]
    fn installs_whose_latest_is_unknown_are_listed_not_passed_as_up_to_date() {
        let idle = crate::agents::test_surface();
        let tools = vec![
            tool("zcode", None, idle.clone(), unchecked(None)),
            tool(
                "codex",
                Some("codex"),
                unchecked(Some("network down")),
                idle.clone(),
            ),
            // Not refreshed yet: nothing is known, and nothing is claimed either way.
            tool("qoder", None, idle.clone(), {
                let mut s = unchecked(None);
                s.latest_checked = false;
                s
            }),
        ];
        let plan = build_update_plan(&tools);
        assert!(plan.auto.is_empty() && plan.manual.is_empty());
        let reasons: Vec<_> = plan
            .unknown
            .iter()
            .map(|item| (item.product_id.as_str(), item.reason.clone().unwrap()))
            .collect();
        assert_eq!(
            reasons,
            vec![
                (
                    "zcode",
                    "没有公开的版本号渠道，由它自己检查更新".to_string()
                ),
                ("codex", "查询最新版本失败：network down".to_string()),
            ]
        );
    }

    #[test]
    fn splits_auto_and_manual_and_dedupes_shared_cli() {
        let idle = crate::agents::test_surface();
        let tools = vec![
            tool(
                "workbuddy-cn",
                Some("codebuddy"),
                updatable(true, "healthy"),
                updatable(false, "healthy"),
            ),
            tool(
                "workbuddy-global",
                Some("codebuddy"),
                updatable(true, "healthy"),
                idle.clone(),
            ),
            tool("claude", Some("claude"), updatable(true, "broken"), idle),
        ];
        let plan = build_update_plan(&tools);
        assert_eq!(plan.auto.len(), 1);
        assert_eq!(plan.auto[0].product_id, "workbuddy-cn");
        assert_eq!(plan.manual.len(), 2);
        assert!(plan
            .manual
            .iter()
            .any(|item| item.surface == Surface::Desktop
                && item.reason.as_deref() == Some("需要在官方下载页手动更新")));
        assert!(plan
            .manual
            .iter()
            .any(|item| item.product_id == "claude"
                && item.reason.as_deref().unwrap().contains("损坏")));
    }
}
