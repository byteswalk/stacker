mod codex;
mod model;
mod reader;
mod store;
mod summary;

use model::*;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};
use std::time::Duration;

fn err(e: impl std::fmt::Display) -> String {
    log::warn!(target:"stacker::conversations","storage/platform operation failed: {}",e);
    "E_STORAGE".into()
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn uid() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}
fn sources(db: &rusqlite::Connection) -> Result<Vec<Source>, String> {
    Ok(store::meta(db, "sources")?.unwrap_or_else(reader::default_sources))
}
fn source_for(db: &rusqlite::Connection, c: &Conversation) -> Result<Source, String> {
    sources(db)?
        .into_iter()
        .find(|s| s.id == c.source_id && s.enabled)
        .ok_or_else(|| "E_SOURCE_MISSING".into())
}

fn local_descendants(all: &[Conversation], root: &Conversation) -> Vec<Conversation> {
    let mut ids = HashSet::from([root.native_id.clone()]);
    let mut result = Vec::new();
    loop {
        let before = ids.len();
        for c in all {
            if c.source_id == root.source_id
                && !c.parent_id.is_empty()
                && ids.contains(&c.parent_id)
                && !ids.contains(&c.native_id)
            {
                ids.insert(c.native_id.clone());
                result.push(c.clone());
            }
        }
        if ids.len() == before {
            break;
        }
    }
    result
}

static CANCEL: AtomicBool = AtomicBool::new(false);
static JOB: OnceLock<Mutex<Job>> = OnceLock::new();
fn state() -> &'static Mutex<Job> {
    JOB.get_or_init(|| Mutex::new(Job::default()))
}
fn publish(job: &Job) {
    if let Ok(mut lock) = state().lock() {
        *lock = job.clone();
    }
    if let Ok(db) = store::connect() {
        if store::set_meta(&db, "job", job).is_err() {
            log::warn!(target:"stacker::conversations","job checkpoint failed");
        }
    }
}

fn scan(job: &mut Job) -> Result<(), String> {
    let db = store::connect()?;
    let existing = store::all(&db)?;
    let parser_current = store::meta::<u32>(&db, "parser_version")? == Some(3);
    let previous: BTreeMap<_, _> = existing.iter().map(|c| (c.path.clone(), c)).collect();
    let mut warnings = Vec::new();
    let mut found = HashSet::new();
    for source in sources(&db)?.into_iter().filter(|s| s.enabled) {
        let titles = reader::native_titles(&source);
        let (files, notes) = reader::files(&source, &CANCEL)?;
        let source_complete = notes.is_empty();
        warnings.extend(notes);
        job.total += files.len();
        publish(job);
        for path in files {
            if CANCEL.load(Ordering::Relaxed) {
                return Err("E_CANCELLED".into());
            }
            let display = reader::display(&path);
            let cached = previous.get(&display).filter(|c| {
                parser_current
                    && reader::quick_fingerprint(&path).ok().as_ref() == Some(&c.fingerprint)
            });
            let result = if let Some(c) = cached {
                Ok((*c).clone())
            } else {
                reader::read(&source, &path, &CANCEL).map(|v| v.0)
            };
            match result {
                Ok(mut c) => {
                    if let Some(title) = titles.get(&c.native_id) {
                        c.title = title.clone();
                    }
                    if found.contains(&c.id) {
                        c.id = format!(
                            "{}:{}",
                            c.id,
                            reader::digest(&path)?.chars().take(16).collect::<String>()
                        );
                        c.complete = false;
                        c.warning = "E_DUPLICATE".into();
                    }
                    found.insert(c.id.clone());
                    store::upsert(&db, &c)?;
                    if source.kind == Kind::Import && !c.summary.is_empty() {
                        db.execute("INSERT OR IGNORE INTO annotations(id,summary,summary_fingerprint) VALUES(?1,?2,?3)",rusqlite::params![c.id,c.summary,c.summary_fingerprint]).map_err(err)?;
                    }
                }
                Err(code) => {
                    warnings.push(format!("{display}: {code}"));
                    if let Some(c) = previous.get(&display) {
                        found.insert(c.id.clone());
                    }
                }
            }
            job.done += 1;
            if job.done % 20 == 0 {
                publish(job);
            }
        }
        // An inaccessible root must not make its existing indexed sessions disappear.
        if source_complete {
            for c in existing
                .iter()
                .filter(|c| c.source_id == source.id && !found.contains(&c.id))
            {
                db.execute("UPDATE conversations SET present=0 WHERE id=?", [&c.id])
                    .map_err(err)?;
            }
        }
    }
    store::set_meta(&db, "warnings", &warnings)?;
    store::set_meta(&db, "parser_version", &3u32)?;
    store::set_meta(&db, "synced_at", &chrono::Utc::now().to_rfc3339())?;
    job.output = format!("{}", found.len());
    Ok(())
}

fn read_current(
    db: &rusqlite::Connection,
    c: &Conversation,
) -> Result<(Conversation, Vec<Message>), String> {
    let source = source_for(db, c)?;
    let (mut fresh, messages) = reader::read(&source, Path::new(&c.path), &AtomicBool::new(false))?;
    fresh.favorite = c.favorite;
    fresh.title = c.title.clone();
    fresh.hidden = c.hidden;
    fresh.group_name = c.group_name.clone();
    fresh.summary = c.summary.clone();
    fresh.summary_fingerprint = c.summary_fingerprint.clone();
    Ok((fresh, messages))
}

fn filtered(
    db: &rusqlite::Connection,
    query: &Query,
) -> Result<(Vec<Conversation>, Vec<String>, usize), String> {
    let enabled: HashSet<_> = sources(db)?
        .into_iter()
        .filter(|s| s.enabled)
        .map(|s| s.id)
        .collect();
    let all: Vec<_> = store::all(db)?
        .into_iter()
        .filter(|c| enabled.contains(&c.source_id))
        .collect();
    let indexed = all.len();
    let mut projects: Vec<_> = all
        .iter()
        .map(|c| {
            if c.group_name.is_empty() {
                c.project.clone()
            } else {
                c.group_name.clone()
            }
        })
        .filter(|s| !s.is_empty())
        .collect();
    projects.sort();
    projects.dedup();
    let needle = query.search.to_lowercase();
    let mut rows = Vec::new();
    for c in all {
        let project = if c.group_name.is_empty() {
            &c.project
        } else {
            &c.group_name
        };
        if !query.source.is_empty() && query.source != c.source_id {
            continue;
        }
        if !query.project.is_empty() && query.project != *project {
            continue;
        }
        if query.before > 0 && c.modified >= query.before {
            continue;
        }
        if query.state != "hidden" && c.hidden {
            continue;
        }
        if match query.state.as_str() {
            "hidden" => !c.hidden,
            "favorite" => !c.favorite,
            "archived" => !c.archived,
            "active" => c.archived,
            "unsummarized" => !c.summary.is_empty() && c.summary_fingerprint == c.fingerprint,
            _ => false,
        } {
            continue;
        }
        if !needle.is_empty()
            && !format!("{} {} {}", c.title, project, c.summary)
                .to_lowercase()
                .contains(&needle)
        {
            if !query.full_text {
                continue;
            }
            let Ok((_, messages)) = read_current(db, &c) else {
                continue;
            };
            if !messages
                .iter()
                .any(|m| m.text.to_lowercase().contains(&needle))
            {
                continue;
            }
        }
        rows.push(c);
    }
    Ok((rows, projects, indexed))
}

#[tauri::command]
pub async fn conversations_list(query: Query) -> Result<Listing, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = store::connect()?;
        let (rows, projects, indexed) = filtered(&db, &query)?;
        Ok(Listing {
            total: rows.len(),
            ids: rows.iter().map(|c| c.id.clone()).collect(),
            items: rows.into_iter().skip(query.offset).take(40).collect(),
            projects,
            indexed,
            synced_at: store::meta(&db, "synced_at")?.unwrap_or_default(),
            warnings: store::meta(&db, "warnings")?.unwrap_or_default(),
        })
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub async fn conversations_read(id: String, offset: usize) -> Result<Detail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = store::connect()?;
        let c = store::get(&db, &id)?;
        let (conversation, messages) = read_current(&db, &c)?;
        Ok(Detail {
            conversation,
            total: messages.len(),
            messages: messages.into_iter().skip(offset).take(60).collect(),
        })
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub fn conversations_sources() -> Result<Value, String> {
    let db = store::connect()?;
    let model: ModelSettings = store::meta(&db, "model")?.unwrap_or_default();
    Ok(
        json!({"contract_version":1,"sources":sources(&db)?,"storage":reader::display(&store::root()),"model":{"endpoint":model.endpoint,"model":model.model,"has_key":!model.key_cipher.is_empty()}}),
    )
}

#[tauri::command]
pub fn conversations_save_settings(
    items: Vec<Source>,
    endpoint: String,
    model: String,
    key: Option<String>,
) -> Result<(), String> {
    let guard = state().lock().map_err(err)?;
    if guard.state == "running" {
        return Err("E_BUSY".into());
    }
    let mut ids = HashSet::new();
    let mut roots = HashSet::new();
    for s in &items {
        if s.id.is_empty()
            || s.id.len() > 100
            || !ids.insert(s.id.clone())
            || s.name.trim().is_empty()
            || !Path::new(&s.root).is_absolute()
        {
            return Err("E_REQUEST".into());
        }
        if !roots.insert((s.root.to_lowercase(), format!("{:?}", s.kind))) {
            return Err("E_DUPLICATE".into());
        }
        if Path::new(&s.root).exists() {
            reader::checked_path(Path::new(&s.root), Path::new(&s.root))?;
        }
    }
    let db = store::connect()?;
    let old = store::meta(&db, "model")?.unwrap_or_default();
    let model = prepare_model(old, endpoint, model, key)?;
    let tx = db.unchecked_transaction().map_err(err)?;
    store::set_meta(&tx, "sources", &items)?;
    store::set_meta(&tx, "model", &model)?;
    tx.commit().map_err(err)?;
    drop(guard);
    Ok(())
}

fn prepare_model(
    old: ModelSettings,
    endpoint: String,
    model: String,
    key: Option<String>,
) -> Result<ModelSettings, String> {
    if endpoint.is_empty() && model.is_empty() && key.as_ref().map_or(true, |k| k.is_empty()) {
        return Ok(ModelSettings::default());
    }
    if model.trim().is_empty() {
        return Err("E_MODEL_MISSING".into());
    }
    let url = url::Url::parse(&endpoint).map_err(|_| "E_MODEL_URL")?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || (local && url.scheme() == "http"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("E_MODEL_URL".into());
    }
    let cipher = match key {
        Some(k) if k.is_empty() => vec![],
        Some(k) => {
            #[cfg(not(windows))]
            {
                let _ = k;
                return Err("E_SECRET_STORE".into());
            }
            #[cfg(windows)]
            {
                crate::dpapi::encrypt(&k)?
            }
        }
        None => {
            if old.endpoint == endpoint {
                old.key_cipher
            } else {
                vec![]
            }
        }
    };
    Ok(ModelSettings {
        endpoint,
        model,
        key_cipher: cipher,
    })
}

#[tauri::command]
pub fn conversations_annotate(
    ids: Vec<String>,
    field: String,
    value: String,
) -> Result<(), String> {
    store::annotate(&store::connect()?, &ids, &field, &value)
}

#[tauri::command]
pub fn conversations_job() -> Result<Job, String> {
    let current = state().lock().map_err(err)?.clone();
    if !current.id.is_empty() {
        return Ok(current);
    }
    let mut saved: Job = store::meta(&store::connect()?, "job")?.unwrap_or_default();
    if saved.state == "running" {
        saved.state = "interrupted".into();
        saved.error = "E_INTERRUPTED".into();
    }
    Ok(saved)
}

#[tauri::command]
pub fn conversations_cancel() -> Result<(), String> {
    CANCEL.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn conversations_prepare_summary(
    ids: Vec<String>,
    locale: String,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move||{
        if ids.is_empty()||ids.len()>20{return Err("E_SUMMARY_LIMIT".into());}
        let db=store::connect()?;let settings:ModelSettings=store::meta(&db,"model")?.unwrap_or_default();
        if settings.endpoint.is_empty()||settings.model.trim().is_empty(){return Err("E_MODEL_MISSING".into());}
        let mut consent=SummaryConsent{token:uid(),ids:vec![],fingerprints:BTreeMap::new(),endpoint:settings.endpoint,model:settings.model,locale,created:now()};
        let mut items=Vec::new();
        for id in ids.into_iter().collect::<std::collections::BTreeSet<_>>(){
            let c=store::get(&db,&id)?;let(fresh,messages)=read_current(&db,&c)?;
            if !fresh.complete{return Err("E_PARTIAL".into());}
            let messages=summary::redact(&messages);
            let chars:usize=messages.iter().map(|m|m.text.chars().count()).sum();
            if chars>summary::MAX_SUMMARY_CHARS{return Err("E_SUMMARY_LIMIT".into());}
            consent.fingerprints.insert(id.clone(),summary::payload_digest(&messages)?);
            consent.ids.push(id.clone());
            items.push(json!({"id":id,"title":c.title,"chars":chars,"text":serde_json::to_string_pretty(&messages).map_err(err)?}));
        }
        store::prune_tokens(&db,"consent:",now().saturating_sub(24*3600))?;
        store::set_meta(&db,&format!("consent:{}",consent.token),&consent)?;
        Ok(json!({"token":consent.token,"endpoint":consent.endpoint,"model":consent.model,"items":items}))
    }).await.map_err(err)?
}

#[tauri::command]
pub fn conversations_start(
    action: String,
    ids: Vec<String>,
    destination: Option<String>,
    locale: String,
    approval: Option<String>,
) -> Result<String, String> {
    if !["scan", "export", "summarize", "handoff"].contains(&action.as_str()) {
        return Err("E_REQUEST".into());
    }
    if action != "scan" && (ids.is_empty() || ids.len() > 10_000) {
        return Err("E_REQUEST".into());
    }
    let consent = if action == "summarize" {
        let db = store::connect()?;
        let token = approval.ok_or("E_APPROVAL")?;
        let consent: SummaryConsent =
            store::meta(&db, &format!("consent:{token}"))?.ok_or("E_APPROVAL")?;
        let settings: ModelSettings = store::meta(&db, "model")?.unwrap_or_default();
        let selected: HashSet<_> = ids.iter().collect();
        let approved: HashSet<_> = consent.ids.iter().collect();
        if selected != approved
            || consent.endpoint != settings.endpoint
            || consent.model != settings.model
            || consent.locale != locale
            || now().saturating_sub(consent.created) > 600
        {
            return Err("E_APPROVAL".into());
        }
        Some(consent)
    } else {
        None
    };
    let job = Job {
        id: uid(),
        action: action.clone(),
        state: "running".into(),
        total: if action == "scan" { 0 } else { ids.len() },
        ..Default::default()
    };
    {
        let mut lock = state().lock().map_err(err)?;
        if lock.state == "running" {
            return Err("E_BUSY".into());
        }
        *lock = job.clone();
    }
    CANCEL.store(false, Ordering::Relaxed);
    publish(&job);
    let id = job.id.clone();
    std::thread::spawn(move || {
        let mut job = job;
        let result = if action == "scan" {
            scan(&mut job)
        } else {
            run_read_job(&mut job, ids, destination, locale, consent)
        };
        finish(&mut job, result);
    });
    Ok(id)
}

fn finish(job: &mut Job, result: Result<(), String>) {
    job.state = match &result {
        Err(e) if e == "E_CANCELLED" => "cancelled",
        Err(_) => "failed",
        Ok(_) if job.items.iter().any(|i| i.status == "failed") => "partial",
        _ => "completed",
    }
    .into();
    if let Err(e) = result {
        job.error = e;
    }
    log::info!(target:"stacker::conversations","operation={} action={} state={} done={} total={}",job.id,job.action,job.state,job.done,job.total);
    publish(job);
}

fn run_read_job(
    job: &mut Job,
    ids: Vec<String>,
    destination: Option<String>,
    locale: String,
    consent: Option<SummaryConsent>,
) -> Result<(), String> {
    let db = store::connect()?;
    let output = if job.action != "summarize" {
        let parent = PathBuf::from(destination.ok_or("E_PATH")?);
        reader::checked_path(&parent, &parent)?;
        let path = parent.join(format!("stacker-conversations-{}", job.id));
        fs::create_dir(&path).map_err(err)?;
        job.output = reader::display(&path);
        Some(path)
    } else {
        None
    };
    let settings: ModelSettings = store::meta(&db, "model")?.unwrap_or_default();
    let mut handoff = String::from(
        "# Project handoff\n\nConversation claims are not verification of the current code.\n\n",
    );
    for id in ids.into_iter().collect::<std::collections::BTreeSet<_>>() {
        if CANCEL.load(Ordering::Relaxed) {
            return Err("E_CANCELLED".into());
        }
        let c = store::get(&db, &id)?;
        let result = (|| {
            let (fresh, messages) = read_current(&db, &c)?;
            if job.action == "summarize" {
                if !fresh.complete {
                    return Err("E_PARTIAL".into());
                }
                let consent = consent.as_ref().ok_or("E_APPROVAL")?;
                if consent.endpoint != settings.endpoint
                    || consent.model != settings.model
                    || consent.locale != locale
                {
                    return Err("E_APPROVAL".into());
                }
                let redacted = summary::redact(&messages);
                if consent.fingerprints.get(&c.id) != Some(&summary::payload_digest(&redacted)?) {
                    return Err("E_APPROVAL".into());
                }
                let summary = summary::generate(&settings, &redacted, &locale)?;
                if CANCEL.load(Ordering::Relaxed) {
                    return Err("E_CANCELLED".into());
                }
                if reader::quick_fingerprint(Path::new(&c.path))? != fresh.fingerprint {
                    return Err("E_CHANGED".into());
                }
                store::summary(&db, &fresh, &summary)?;
            } else if job.action == "handoff" {
                handoff.push_str(&format!(
                    "## {}\n\nSource: {} / {}\nProject: {}\n\n{}\n\n",
                    c.title,
                    c.client,
                    c.native_id,
                    c.project,
                    if c.summary.is_empty() {
                        "No summary. See the original transcript."
                    } else {
                        &c.summary
                    }
                ));
                if c.summary_fingerprint != fresh.fingerprint {
                    handoff.push_str("Summary may be outdated.\n\n");
                }
            } else {
                let name = format!("{:04}-{}", job.done, c.native_id);
                let root = output.as_ref().ok_or("E_PATH")?;
                let source = source_for(&db, &c)?;
                if source.kind != Kind::Import {
                    reader::checked_path(Path::new(&source.root), Path::new(&c.path))?;
                    fs::copy(&c.path, root.join(format!("{name}.jsonl"))).map_err(err)?;
                    if reader::quick_fingerprint(Path::new(&c.path))? != fresh.fingerprint {
                        return Err("E_CHANGED".into());
                    }
                }
                store::atomic_json(
                    &root.join(format!("{name}.json")),
                    &Bundle {
                        schema_version: 1,
                        conversation: fresh.clone(),
                        messages: messages.clone(),
                    },
                )?;
                let mut text = format!(
                    "# {}\n\n{} / {}\n\n",
                    fresh.title, fresh.client, fresh.project
                );
                for message in messages {
                    text.push_str(&format!(
                        "## L{} {}\n\n{}\n\n",
                        message.line, message.role, message.text
                    ));
                }
                fs::write(root.join(format!("{name}.md")), text).map_err(err)?;
            }
            Ok(())
        })();
        if result.as_ref().is_err_and(|e| e == "E_CANCELLED") {
            return result;
        }
        job.items.push(JobItem {
            id,
            title: c.title,
            status: if result.is_ok() {
                "completed"
            } else {
                "failed"
            }
            .into(),
            detail: result.err().unwrap_or_default(),
        });
        job.done += 1;
        publish(job);
    }
    if job.action == "handoff" {
        fs::write(output.ok_or("E_PATH")?.join("HANDOFF.md"), handoff).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn conversations_preview(ids: Vec<String>, action: String) -> Result<Preview, String> {
    tauri::async_runtime::spawn_blocking(move || preview(ids, action))
        .await
        .map_err(err)?
}

fn preview(ids: Vec<String>, action: String) -> Result<Preview, String> {
    if !["archive", "unarchive", "delete"].contains(&action.as_str())
        || ids.is_empty()
        || ids.len() > 500
    {
        return Err("E_REQUEST".into());
    }
    let db = store::connect()?;
    let all = store::all(&db)?;
    let mut plan = Preview {
        token: uid(),
        action: action.clone(),
        selected: vec![],
        affected: vec![],
        blocked: vec![],
        bytes: 0,
        created: now(),
        proofs: BTreeMap::new(),
    };
    let ready = codex::require_closed().and_then(|_| codex::capabilities());
    let mut seen = HashSet::new();
    let mut clients = std::collections::HashMap::new();
    for id in ids {
        let c = store::get(&db, &id)?;
        let result = (|| {
            let source = source_for(&db, &c)?;
            if source.kind != Kind::Codex {
                return Err("E_READ_ONLY".into());
            }
            ready.clone()?;
            if !clients.contains_key(&source.id) {
                clients.insert(source.id.clone(), codex::Rpc::start(&source)?);
            }
            let rpc = clients.get_mut(&source.id).ok_or("E_RPC")?;
            let mut affected = vec![c.clone()];
            if action != "unarchive" {
                affected.extend(local_descendants(&all, &c));
            }
            if action != "unarchive" {
                for child in rpc.descendants(&c.native_id)? {
                    let native = child["id"].as_str().ok_or("E_RPC")?;
                    let local = all
                        .iter()
                        .find(|x| x.source_id == c.source_id && x.native_id == native)
                        .ok_or("E_INCOMPLETE_SCOPE")?;
                    affected.push(local.clone());
                }
            }
            for target in &affected {
                let (fresh, _) = read_current(&db, target)?;
                if !fresh.complete || !target.complete || now().saturating_sub(fresh.modified) < 120
                {
                    return Err("E_CHANGED".into());
                }
                let native = rpc.call(
                    "thread/read",
                    json!({"threadId":target.native_id,"includeTurns":false}),
                )?;
                let path = native["thread"]["path"]
                    .as_str()
                    .ok_or("E_INCOMPLETE_SCOPE")?;
                if reader::checked_path(Path::new(&source.root), Path::new(path))?
                    != reader::checked_path(Path::new(&source.root), Path::new(&target.path))?
                {
                    return Err("E_INCOMPLETE_SCOPE".into());
                }
            }
            Ok(affected)
        })();
        match result {
            Ok(affected) => {
                plan.selected.push(id);
                for c in affected {
                    if seen.insert(c.id.clone()) {
                        plan.proofs
                            .insert(c.id.clone(), reader::digest(Path::new(&c.path))?);
                        plan.bytes += c.bytes;
                        plan.affected.push(c);
                    }
                }
            }
            Err(code) => plan.blocked.push(JobItem {
                id,
                title: c.title,
                status: "blocked".into(),
                detail: code,
            }),
        }
    }
    // Previews expire after ten minutes; keep an hour of slack for clock changes.
    store::prune_tokens(&db, "preview:", now().saturating_sub(3600))?;
    store::set_meta(&db, &format!("preview:{}", plan.token), &plan)?;
    Ok(plan)
}

#[tauri::command]
pub fn conversations_execute(token: String) -> Result<String, String> {
    let db = store::connect()?;
    let plan: Preview = store::meta(&db, &format!("preview:{token}"))?.ok_or("E_PREVIEW")?;
    if now().saturating_sub(plan.created) > 600 || plan.selected.is_empty() {
        return Err("E_PREVIEW".into());
    }
    let job = Job {
        id: plan.token.clone(),
        action: plan.action.clone(),
        state: "running".into(),
        total: plan.affected.len(),
        ..Default::default()
    };
    {
        let mut lock = state().lock().map_err(err)?;
        if lock.state == "running" {
            return Err("E_BUSY".into());
        }
        let changed = db
            .execute("DELETE FROM meta WHERE key=?", [format!("preview:{token}")])
            .map_err(err)?;
        if changed != 1 {
            return Err("E_PREVIEW".into());
        }
        *lock = job.clone();
    }
    CANCEL.store(false, Ordering::Relaxed);
    publish(&job);
    let id = job.id.clone();
    std::thread::spawn(move || {
        let mut job = job;
        let result = execute(&mut job, &plan);
        finish(&mut job, result);
    });
    Ok(id)
}

fn execute(job: &mut Job, plan: &Preview) -> Result<(), String> {
    codex::require_closed()?;
    let fresh = preview(plan.selected.clone(), plan.action.clone())?;
    let expected: HashSet<_> = plan.affected.iter().map(|c| &c.id).collect();
    let actual: HashSet<_> = fresh.affected.iter().map(|c| &c.id).collect();
    if !fresh.blocked.is_empty() || expected != actual || plan.proofs != fresh.proofs {
        return Err("E_CHANGED".into());
    }
    let db = store::connect()?;
    let backup = store::root().join("backups").join(&job.id);
    fs::create_dir_all(&backup).map_err(err)?;
    job.output = reader::display(&backup);
    // Back up every affected rollout before the first source mutation. Originals stay untouched on failure.
    for (n, c) in plan.affected.iter().enumerate() {
        if CANCEL.load(Ordering::Relaxed) {
            return Err("E_CANCELLED".into());
        }
        let (fresh, messages) = read_current(&db, c)?;
        let copy = backup.join(format!("{n:04}-{}.jsonl", c.native_id));
        fs::copy(&c.path, &copy).map_err(err)?;
        if reader::digest(&copy)? != *plan.proofs.get(&c.id).ok_or("E_PREVIEW")? {
            return Err("E_CHANGED".into());
        }
        store::atomic_json(
            &backup.join(format!("{n:04}-{}.json", c.native_id)),
            &Bundle {
                schema_version: 1,
                conversation: fresh,
                messages,
            },
        )?;
    }
    store::atomic_json(&backup.join("manifest.json"), plan)?;
    let mut handled = HashSet::new();
    // Old rollouts can contain parent relationships absent from the native DB.
    // Process deepest children first, then the parent, covering both relationship sources.
    let mut selected: Vec<_> = plan.affected.iter().collect();
    selected.sort_by_key(|c| local_descendants(&plan.affected, c).len());
    for c in selected {
        if handled.contains(&c.id) {
            continue;
        }
        if CANCEL.load(Ordering::Relaxed) {
            return Err("E_CANCELLED".into());
        }
        codex::require_closed()?;
        let source = source_for(&db, c)?;
        let mut rpc = codex::Rpc::start(&source)?;
        let children = if plan.action == "unarchive" {
            vec![]
        } else {
            rpc.descendants(&c.native_id)?
        };
        let mut group = vec![c];
        for child in children {
            let native = child["id"].as_str().ok_or("E_RPC")?;
            let target = plan
                .affected
                .iter()
                .find(|x| x.source_id == c.source_id && x.native_id == native)
                .ok_or("E_CHANGED")?;
            if !handled.contains(&target.id) {
                group.push(target);
            }
        }
        for target in &group {
            if reader::digest(Path::new(&target.path))?
                != *plan.proofs.get(&target.id).ok_or("E_PREVIEW")?
            {
                return Err("E_CHANGED".into());
            }
        }
        let result = rpc.call(
            &format!("thread/{}", plan.action),
            json!({"threadId":c.native_id}),
        );
        for target in group {
            handled.insert(target.id.clone());
            let verification = if result.is_err() {
                Err(result.as_ref().err().unwrap().clone())
            } else if plan.action == "delete" {
                if Path::new(&target.path).exists() {
                    Err("E_VERIFY".into())
                } else {
                    db.execute(
                        "UPDATE conversations SET present=0 WHERE id=?",
                        [&target.id],
                    )
                    .map_err(err)?;
                    Ok(())
                }
            } else {
                match rpc.call(
                    "thread/read",
                    json!({"threadId":target.native_id,"includeTurns":false}),
                ) {
                    Ok(v) => {
                        let path = v["thread"]["path"].as_str().ok_or("E_VERIFY")?;
                        let (mut updated, _) = reader::read(&source, Path::new(path), &CANCEL)?;
                        if updated.archived != (plan.action == "archive") {
                            Err("E_VERIFY".into())
                        } else {
                            updated.id = target.id.clone();
                            store::upsert(&db, &updated)?;
                            Ok(())
                        }
                    }
                    Err(e) => Err(e),
                }
            };
            job.items.push(JobItem {
                id: target.id.clone(),
                title: target.title.clone(),
                status: if verification.is_ok() {
                    "completed"
                } else {
                    "failed"
                }
                .into(),
                detail: verification.err().unwrap_or_default(),
            });
            job.done += 1;
            publish(job);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn conversations_open(id: String, target: String) -> Result<(), String> {
    let db = store::connect()?;
    let c = store::get(&db, &id)?;
    let source = source_for(&db, &c)?;
    if target == "native" {
        if source.kind != Kind::Codex {
            return Err("E_UNSUPPORTED".into());
        }
        if !c
            .native_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err("E_REQUEST".into());
        }
        let mut cmd = std::process::Command::new("explorer.exe");
        cmd.arg(format!("codex://threads/{}", c.native_id));
        codex::hidden(&mut cmd);
        cmd.spawn().map_err(err)?;
        return Ok(());
    }
    let path = if target == "project" {
        PathBuf::from(&c.project)
    } else {
        Path::new(&c.path).parent().ok_or("E_PATH")?.to_path_buf()
    };
    if !path.is_absolute() || !path.is_dir() {
        return Err("E_PATH".into());
    }
    let mut cmd = std::process::Command::new("explorer.exe");
    cmd.arg(path);
    codex::hidden(&mut cmd);
    cmd.spawn().map_err(err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_annotations_survive_refresh() {
        let temp = tempfile::tempdir().unwrap();
        let db = store::connect_at(temp.path()).unwrap();
        let mut c = Conversation {
            id: "s:a".into(),
            title: "hello".into(),
            fingerprint: "v1".into(),
            ..Default::default()
        };
        store::upsert(&db, &c).unwrap();
        store::annotate(&db, &[c.id.clone()], "favorite", "1").unwrap();
        store::summary(&db, &c, "decision [L2]").unwrap();
        c.fingerprint = "v2".into();
        store::upsert(&db, &c).unwrap();
        let row = store::get(&db, &c.id).unwrap();
        assert!(row.favorite);
        assert_eq!(row.summary_fingerprint, "v1");
        assert_eq!(row.fingerprint, "v2");
    }
    #[test]
    fn future_schema_is_not_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let db = store::connect_at(temp.path()).unwrap();
        db.execute_batch("PRAGMA user_version=99;").unwrap();
        drop(db);
        assert_eq!(store::connect_at(temp.path()).err().unwrap(), "E_SCHEMA");
    }
    #[test]
    fn annotation_field_allowlist() {
        let temp = tempfile::tempdir().unwrap();
        let db = store::connect_at(temp.path()).unwrap();
        assert!(store::annotate(&db, &[], "id", "x").is_err());
    }
}
