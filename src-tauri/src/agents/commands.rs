use crate::agents::{registry::*, *};

#[tauri::command]
pub async fn vibe_tools() -> Vec<VibeTool> {
    tauri::async_runtime::spawn_blocking(scan_vibe_tools_cached)
        .await
        .unwrap_or_default()
}

/// Explicit user refreshes must not reuse the short-lived startup cache. This
/// matters for desktop agents that update themselves outside of Stacker.
#[tauri::command]
pub async fn vibe_tools_refresh() -> Vec<VibeTool> {
    tauri::async_runtime::spawn_blocking(|| {
        invalidate_vibe_scan_cache();
        scan_vibe_tools_cached()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub fn vibe_catalog() -> Vec<VibeTool> {
    tool_specs().into_iter().map(vibe_catalog_tool).collect()
}

#[tauri::command]
pub async fn vibe_tool(id: String) -> Result<VibeTool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let tool = scan_vibe_tool(&id, true).ok_or_else(|| "未知的工作智能体工具".to_string())?;
        cache_vibe_tool(&tool);
        Ok(tool)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn vibe_environment_prompt() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(build_environment_prompt)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn vibe_tool_action(
    window: tauri::Window,
    id: String,
    target: String,
    action: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let result = run_tool_action(&id, &target, &action, Some(window));
        if result.is_ok() {
            invalidate_vibe_scan_cache();
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn vibe_open_desktop(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || open_desktop_tool(&id))
        .await
        .map_err(|e| e.to_string())?
}

use super::tasks::{AgentTask, AgentTaskManager, TaskRequest};

#[tauri::command]
pub fn agent_task_start(
    request: TaskRequest,
    manager: tauri::State<'_, AgentTaskManager>,
) -> Result<AgentTask, String> {
    manager.start(request)
}

#[tauri::command]
pub fn agent_task_cancel(
    id: String,
    manager: tauri::State<'_, AgentTaskManager>,
) -> Result<(), String> {
    manager.cancel(&id)
}

#[tauri::command]
pub fn agent_task_retry(
    id: String,
    manager: tauri::State<'_, AgentTaskManager>,
) -> Result<AgentTask, String> {
    manager.retry(&id)
}

#[tauri::command]
pub fn agent_tasks(manager: tauri::State<'_, AgentTaskManager>) -> Vec<AgentTask> {
    manager.list()
}

/// Drops every finished task; returns the tasks still open.
#[tauri::command]
pub fn agent_tasks_clear(manager: tauri::State<'_, AgentTaskManager>) -> Vec<AgentTask> {
    manager.clear_finished()
}

#[tauri::command]
pub fn agent_task_dismiss(
    id: String,
    manager: tauri::State<'_, AgentTaskManager>,
) -> Result<(), String> {
    manager.dismiss(&id)
}

#[tauri::command]
pub fn agent_task_log(
    id: String,
    manager: tauri::State<'_, AgentTaskManager>,
) -> Result<Vec<String>, String> {
    manager.log(&id)
}

#[tauri::command]
pub async fn agent_update_plan() -> super::tasks::plan::UpdatePlan {
    tauri::async_runtime::spawn_blocking(|| {
        let tools = super::last_scan_or_scan();
        let mut plan = super::tasks::plan::build_update_plan(&tools);
        super::tasks::plan::hold_running_apps(&mut plan, &tools, super::install::image_is_running);
        plan
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub async fn agent_update_all(app: tauri::AppHandle) -> Result<Vec<AgentTask>, String> {
    use tauri::Manager;
    let plan = agent_update_plan().await;
    let manager = app.state::<AgentTaskManager>();
    plan.auto
        .into_iter()
        .map(|item| {
            manager.start(TaskRequest {
                product_id: item.product_id,
                surface: item.surface,
                action: super::tasks::Action::Update,
            })
        })
        .collect()
}
