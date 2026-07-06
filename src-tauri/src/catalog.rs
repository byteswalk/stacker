use crate::{binary, custom, sources, update};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone)]
pub struct SourceCatalogRow {
    pub row_id: String,
    pub tool_id: String,
    pub tool_name: String,
    pub category: String,
    pub category_label: String,
    pub source: String,
    pub source_label: String,
    pub mirror_id: String,
    pub name: String,
    pub url: String,
    pub host: String,
    pub description: String,
    pub current: bool,
    pub mutable: bool,
    pub has_auth: bool,
    pub duplicate: bool,
}

#[derive(Serialize)]
pub struct SourceCatalogStatus {
    pub server_url: String,
    pub server_version: Option<u32>,
    pub builtin_count: usize,
    pub local_count: usize,
    pub binary_count: usize,
    pub rows: Vec<SourceCatalogRow>,
}

#[derive(Serialize, Deserialize)]
struct SourceCatalogFile {
    format: String,
    version: u32,
    exported_at: String,
    #[serde(default)]
    catalog_rows: Vec<SourceCatalogRow>,
    #[serde(default)]
    builtin_sources: Option<update::RemoteList>,
    #[serde(default)]
    local_sources: Vec<custom::CustomDTO>,
    #[serde(default)]
    server_sources: Option<update::RemoteList>,
}

#[derive(Serialize)]
pub struct SourceCatalogImportResult {
    pub local_added: usize,
    pub local_skipped: usize,
    pub server_imported: bool,
    pub builtin_tools: usize,
    pub builtin_mirrors: usize,
}

fn category(tool_id: &str, handler: &str) -> (&'static str, &'static str) {
    match tool_id {
        "python-runtime" | "node-runtime" => ("runtime", "运行时下载"),
        "maven" | "gradle" => ("build", "构建工具"),
        _ if handler == "runtime_download" => ("runtime", "运行时下载"),
        _ => ("package", "包仓库"),
    }
}

fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    rest.split('/')
        .next()
        .unwrap_or("")
        .split('@')
        .last()
        .unwrap_or("")
        .to_string()
}

fn norm_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_ascii_lowercase()
}

fn tool_meta() -> HashMap<String, sources::Tool> {
    sources::hardcoded()
        .into_iter()
        .map(|t| (t.id.clone(), t))
        .collect()
}

fn current_map() -> HashMap<String, Option<String>> {
    sources::list_sources()
        .into_iter()
        .map(|t| (t.id, t.current))
        .collect()
}

fn mirror_row(
    layer: &str,
    layer_label: &str,
    tool: &sources::Tool,
    mirror: &sources::Mirror,
    current: bool,
    mutable: bool,
    has_auth: bool,
) -> SourceCatalogRow {
    let (cat, cat_label) = category(&tool.id, &tool.handler);
    SourceCatalogRow {
        row_id: format!("{layer}:{}:{}", tool.id, mirror.id),
        tool_id: tool.id.clone(),
        tool_name: tool.name.clone(),
        category: cat.into(),
        category_label: cat_label.into(),
        source: layer.into(),
        source_label: layer_label.into(),
        mirror_id: mirror.id.clone(),
        name: mirror.name.clone(),
        url: mirror.url.clone(),
        host: if mirror.host.trim().is_empty() {
            host_of(&mirror.url)
        } else {
            mirror.host.clone()
        },
        description: String::new(),
        current,
        mutable,
        has_auth,
        duplicate: false,
    }
}

fn mark_duplicates(rows: &mut [SourceCatalogRow]) {
    let mut counts: HashMap<(String, String), usize> = HashMap::new();
    for row in rows.iter() {
        let key = (row.tool_id.clone(), norm_url(&row.url));
        if !key.1.is_empty() {
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    for row in rows.iter_mut() {
        row.duplicate = counts
            .get(&(row.tool_id.clone(), norm_url(&row.url)))
            .copied()
            .unwrap_or(0)
            > 1;
    }
}

fn effective_builtin_snapshot() -> update::RemoteList {
    let mut tools = sources::hardcoded();
    update::overlay(&mut tools);
    let version = update::remote_snapshot().map(|r| r.version).unwrap_or(0);
    update::RemoteList {
        version,
        tools: tools
            .into_iter()
            .map(|t| update::RemoteTool {
                id: t.id,
                mirrors: t.mirrors,
            })
            .collect(),
    }
}

#[tauri::command]
pub fn source_catalog_status() -> SourceCatalogStatus {
    let meta = tool_meta();
    let current = current_map();
    let remote = update::remote_snapshot();
    let server_url = update::mirrors_status().url;
    let mut rows = Vec::new();

    let mut builtin_tools = sources::hardcoded();
    update::overlay(&mut builtin_tools);
    let mut builtin_count = 0usize;
    for tool in &builtin_tools {
        for mirror in &tool.mirrors {
            builtin_count += 1;
            let is_current = current
                .get(&tool.id)
                .and_then(|id| id.as_deref())
                .map(|id| id == mirror.id)
                .unwrap_or(false);
            rows.push(mirror_row(
                "builtin", "内置", tool, mirror, is_current, false, false,
            ));
        }
    }

    let locals = custom::export_all();
    let mut local_count = 0usize;
    for c in &locals {
        let Some(base) = meta.get(&c.tool) else {
            continue;
        };
        local_count += 1;
        let mirror = sources::Mirror {
            id: c.id.clone(),
            name: c.name.clone(),
            url: c.url.clone(),
            host: host_of(&c.url),
        };
        let is_current = current
            .get(&base.id)
            .and_then(|id| id.as_deref())
            .map(|id| id == c.id)
            .unwrap_or(false);
        rows.push(mirror_row(
            "local",
            "本地",
            base,
            &mirror,
            is_current,
            true,
            c.has_password || !c.username.trim().is_empty(),
        ));
    }

    let mut binary_count = 0usize;
    for item in binary::catalog_entries() {
        binary_count += 1;
        rows.push(SourceCatalogRow {
            row_id: format!("binary:{}", item.id),
            tool_id: format!("binary-{}", item.id),
            tool_name: item.name,
            category: "binary".into(),
            category_label: "大文件下载".into(),
            source: "builtin".into(),
            source_label: "内置".into(),
            mirror_id: item.id,
            name: "国内镜像".into(),
            url: item.url,
            host: item.host,
            description: item.description,
            current: false,
            mutable: false,
            has_auth: false,
            duplicate: false,
        });
    }

    mark_duplicates(&mut rows);
    SourceCatalogStatus {
        server_url,
        server_version: remote.as_ref().map(|r| r.version),
        builtin_count,
        local_count,
        binary_count,
        rows,
    }
}

#[tauri::command]
pub fn source_catalog_export(path: String, include_server: bool) -> Result<(), String> {
    let status = source_catalog_status();
    let file = SourceCatalogFile {
        format: "stacker-source-catalog".into(),
        version: 1,
        exported_at: chrono::Local::now().to_rfc3339(),
        catalog_rows: status.rows,
        builtin_sources: Some(effective_builtin_snapshot()),
        local_sources: custom::export_all(),
        server_sources: include_server.then(update::remote_snapshot).flatten(),
    };
    std::fs::write(
        path,
        serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn source_catalog_import(
    path: String,
    import_server: bool,
) -> Result<SourceCatalogImportResult, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let file: SourceCatalogFile =
        serde_json::from_str(&text).map_err(|e| format!("源配置文件格式错误：{e}"))?;
    if file.format != "stacker-source-catalog" {
        return Err("不是 Stacker 源管理导出的配置文件".into());
    }
    let local = custom::import_preserve(file.local_sources)?;
    let mut server_imported = false;
    let mut builtin_tools = 0usize;
    let mut builtin_mirrors = 0usize;
    if import_server {
        let snapshot = file.builtin_sources.or(file.server_sources);
        if let Some(server) = snapshot {
            builtin_tools = server.tools.len();
            builtin_mirrors = server.tools.iter().map(|t| t.mirrors.len()).sum();
            if builtin_tools > 0 {
                update::save_remote_snapshot(&server)?;
                server_imported = true;
            }
        }
    }
    Ok(SourceCatalogImportResult {
        local_added: local.added,
        local_skipped: local.skipped,
        server_imported,
        builtin_tools,
        builtin_mirrors,
    })
}
