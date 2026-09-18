//! Production task runner: declares resources from the registry and the last scan, runs
//! the existing install flows, then re-detects the product to verify the outcome.

use super::schedule::Resource;
use super::{Action, Surface, TaskPlan, TaskRequest, TaskRunner};
use crate::agents::registry::spec_by_id;
use crate::agents::{VibeSurface, VibeTool};

/// Checks the re-detected surface after an action. An update only fails on an unchanged
/// version when an update is still being advertised, so self-updating desktop apps that
/// apply on restart are not reported as failures.
pub(crate) fn verify_outcome(
    action: Action,
    before_version: Option<&str>,
    after: &VibeSurface,
) -> Result<(), String> {
    let reason = || {
        after
            .broken_reason
            .clone()
            .unwrap_or_else(|| "未检测到可用入口".into())
    };
    match action {
        Action::Install | Action::Repair if after.health == "healthy" => Ok(()),
        Action::Install => Err(format!("安装后校验未通过：{}", reason())),
        Action::Repair => Err(format!("修复后校验未通过：{}", reason())),
        Action::Update if after.health != "healthy" => {
            Err(format!("更新后校验未通过：{}", reason()))
        }
        Action::Update
            if before_version.is_some()
                && after.version.as_deref() == before_version
                && after.update_available =>
        {
            Err("更新后校验未通过：版本未变化".into())
        }
        Action::Update => Ok(()),
        Action::Uninstall if after.health == "missing" => Ok(()),
        Action::Uninstall => Err("卸载后仍检测到入口".into()),
    }
}

fn surface_of(tool: &VibeTool, surface: Surface) -> &VibeSurface {
    match surface {
        Surface::Cli => &tool.cli,
        Surface::Desktop => &tool.desktop,
    }
}

pub(crate) struct ProductionRunner;

impl TaskRunner for ProductionRunner {
    fn plan(&self, request: &TaskRequest) -> Result<TaskPlan, String> {
        let spec = spec_by_id(&request.product_id).ok_or("未知的智能体")?;
        let method = crate::agents::cached_tool(&request.product_id)
            .and_then(|tool| surface_of(&tool, request.surface).install_method.clone());
        let mut resources = Vec::new();
        let (key, label) = match request.surface {
            Surface::Cli => {
                let key = spec.cli_id.ok_or("该智能体没有 CLI")?.to_string();
                if spec.cli.npm_package.is_some()
                    || matches!(method.as_deref(), Some("npm") | Some("conda-npm"))
                {
                    resources.push(Resource::Npm);
                }
                if spec.cli.winget_id.is_some() || method.as_deref() == Some("winget") {
                    resources.push(Resource::Installer);
                }
                (key, spec.cli.name.to_string())
            }
            Surface::Desktop => {
                if !spec.desktop_available {
                    return Err("该智能体没有桌面端".into());
                }
                resources.push(Resource::Installer);
                (spec.id.to_string(), spec.desktop.name.to_string())
            }
        };
        if request.action != Action::Uninstall {
            resources.push(Resource::Download);
        }
        resources.insert(0, Resource::Product(key.clone()));
        Ok(TaskPlan {
            key,
            product_name: spec.name.to_string(),
            surface_label: label,
            cli_id: spec.cli_id.map(Into::into),
            resources,
        })
    }

    fn run(&self, request: &TaskRequest) -> Result<String, String> {
        let target = match request.surface {
            Surface::Cli => "cli",
            Surface::Desktop => "desktop",
        };
        let action = match request.action {
            Action::Install => "install",
            Action::Update => "update",
            Action::Uninstall => "uninstall",
            Action::Repair => "repair",
        };
        let before = crate::agents::fresh_tool(&request.product_id, false)
            .and_then(|tool| surface_of(&tool, request.surface).version.clone());
        let result = crate::agents::run_tool_action(&request.product_id, target, action, None);
        crate::agents::invalidate_vibe_scan_cache();
        let after =
            crate::agents::fresh_tool(&request.product_id, true).ok_or("无法重新检测该智能体")?;
        let verified = verify_outcome(
            request.action,
            before.as_deref(),
            surface_of(&after, request.surface),
        );
        match (result, verified) {
            (Ok(message), Ok(())) => Ok(message),
            (Ok(_), Err(reason)) => Err(reason),
            (Err(error), Ok(())) if request.action != Action::Uninstall => {
                crate::installer::task_log(&format!("安装器返回错误：{error}"));
                Ok("已完成（安装器返回错误，但复检通过）".into())
            }
            (Err(error), _) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface(health: &str, version: Option<&str>, update_available: bool) -> VibeSurface {
        let mut surface = crate::agents::test_surface();
        surface.health = health.into();
        surface.version = version.map(Into::into);
        surface.update_available = update_available;
        surface
    }

    #[test]
    fn update_must_leave_a_healthy_changed_install() {
        assert!(verify_outcome(
            Action::Update,
            Some("1.0"),
            &surface("healthy", Some("1.1"), false)
        )
        .is_ok());
        assert!(verify_outcome(
            Action::Update,
            Some("1.0"),
            &surface("healthy", Some("1.0"), true)
        )
        .is_err());
        assert!(verify_outcome(
            Action::Update,
            Some("1.0"),
            &surface("healthy", Some("1.0"), false)
        )
        .is_ok());
        assert!(
            verify_outcome(Action::Update, Some("1.0"), &surface("broken", None, false)).is_err()
        );
    }

    #[test]
    fn install_repair_and_uninstall_rules() {
        assert!(verify_outcome(
            Action::Install,
            None,
            &surface("healthy", Some("1.0"), false)
        )
        .is_ok());
        assert!(verify_outcome(Action::Install, None, &surface("missing", None, false)).is_err());
        assert!(verify_outcome(Action::Repair, None, &surface("broken", None, false)).is_err());
        assert!(verify_outcome(Action::Uninstall, None, &surface("missing", None, false)).is_ok());
        assert!(verify_outcome(
            Action::Uninstall,
            None,
            &surface("healthy", Some("1.0"), false)
        )
        .is_err());
    }
}
