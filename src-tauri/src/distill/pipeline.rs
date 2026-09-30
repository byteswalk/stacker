//! 分段提炼与合并去重：把材料切段、逐段调用执行器、解析条目，
//! 再用一次「指出重复项」的调用去重 —— 正文保持模型原样，来源一条不丢。
use super::prompts;
use super::DistillSource;
use crate::runner::{CancelFlag, RunRequest, DEFAULT_TIMEOUT};
use crate::sessions::summary::{self, RunFn, RunnerChoice};

/// 一份材料：一条网页对话、一个本机会话，或合并在一起的若干摘录。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceText {
    pub title: String,
    pub markdown: String,
    pub sources: Vec<DistillSource>,
}

/// 解析出来的一条草稿。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DraftItem {
    pub kind: String,
    pub title: String,
    pub body: String,
    pub sources: Vec<DistillSource>,
}

/// 送进合并调用的条目上限，以及每条给模型看的正文长度。
pub const MAX_MERGE_ITEMS: usize = 80;
pub const MERGE_PREVIEW_CHARS: usize = 300;
/// 一次提炼最多保留的条目数，以及单条的标题与正文上限。
pub const MAX_ITEMS: usize = 200;
pub const TITLE_CHARS: usize = 120;
pub const BODY_CHARS: usize = 20_000;

fn call(
    choice: &RunnerChoice,
    prompt: String,
    cancel: &CancelFlag,
    run: RunFn,
) -> Result<String, String> {
    if cancel.is_cancelled() {
        return Err("E_CANCELLED".into());
    }
    let req = RunRequest {
        backend: choice.backend.clone(),
        model: choice.model.clone(),
        effort: choice.effort.clone(),
        prompt,
        timeout: DEFAULT_TIMEOUT,
        attachments: Vec::new(),
        on_delta: None,
    };
    run(&req, cancel).map(|o| o.text)
}

fn split_tag(rest: &str) -> Option<(&str, &str)> {
    if !rest.starts_with('[') {
        return None;
    }
    let end = rest.find(']')?;
    Some((&rest[..=end], rest[end + 1..].trim()))
}

fn finish(out: &mut Vec<DraftItem>, mut item: DraftItem) {
    let trimmed = item.body.trim();
    let cut = trimmed.chars().count() > BODY_CHARS;
    let mut body: String = trimmed.chars().take(BODY_CHARS).collect();
    // 正文超出单条上限时悄悄截断过：加个「…」，读的人至少能看出这不是全文。
    if cut {
        body.push('…');
    }
    item.body = body;
    if item.title.is_empty() || item.body.is_empty() {
        return;
    }
    out.push(item);
}

/// 把一次回答拆成条目：`### [标记] 标题` 开头，正文到下一个标题为止。
/// 不认识的标记、没选的产出类型、标题或正文为空的条目都丢弃。
pub fn parse_items(text: &str, kinds: &[String]) -> Vec<DraftItem> {
    let mut out: Vec<DraftItem> = Vec::new();
    let mut current: Option<DraftItem> = None;
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("###") {
            if let Some((tag, title)) = split_tag(rest.trim_start()) {
                if let Some(item) = current.take() {
                    finish(&mut out, item);
                }
                let chosen = prompts::kind_of(tag).filter(|k| kinds.iter().any(|c| c == k));
                current = chosen.map(|kind| DraftItem {
                    kind: kind.to_string(),
                    title: title.trim().chars().take(TITLE_CHARS).collect(),
                    ..Default::default()
                });
                continue;
            }
        }
        if let Some(item) = current.as_mut() {
            item.body.push_str(line);
            item.body.push('\n');
        }
    }
    if let Some(item) = current.take() {
        finish(&mut out, item);
    }
    out
}

/// 归一化后比较：忽略大小写、空白和标点。
fn normalized(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn add_sources(into: &mut Vec<DistillSource>, from: &[DistillSource]) {
    for s in from {
        if !into.iter().any(|x| x.key == s.key) {
            into.push(s.clone());
        }
    }
}

fn fingerprint(item: &DraftItem) -> (String, String, String) {
    (
        item.kind.clone(),
        normalized(&item.title),
        normalized(&item.body),
    )
}

/// 完全相同的条目（类型、标题、正文归一化后一致）合成一条，来源合并。
pub fn collapse_identical(items: Vec<DraftItem>) -> Vec<DraftItem> {
    let mut out: Vec<DraftItem> = Vec::new();
    for item in items {
        let key = fingerprint(&item);
        match out.iter_mut().find(|o| fingerprint(o) == key) {
            Some(existing) => add_sources(&mut existing.sources, &item.sources),
            None => out.push(item),
        }
    }
    out
}

/// 合并调用看到的清单：编号、标记、标题和正文开头。
pub fn merge_list(items: &[DraftItem]) -> String {
    items
        .iter()
        .take(MAX_MERGE_ITEMS)
        .enumerate()
        .map(|(i, item)| {
            let preview: String = item.body.chars().take(MERGE_PREVIEW_CHARS).collect();
            format!(
                "#{} {} {}\n{}",
                i + 1,
                prompts::tag_of(&item.kind).unwrap_or(""),
                item.title,
                preview.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 一段文字里第一串数字，转成从 0 开始的下标；越界返回 None。
fn number(text: &str, count: usize) -> Option<usize> {
    let from_digit = text.trim_start_matches(|c: char| !c.is_ascii_digit());
    let digits: String = from_digit
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits
        .parse::<usize>()
        .ok()
        .filter(|n| *n >= 1 && *n <= count)
        .map(|n| n - 1)
}

/// 读出 `@merge 3 <- 7, 12` 这样的行；编号从 1 开始，越界与自指的忽略。
pub fn merge_groups(answer: &str, count: usize) -> Vec<(usize, Vec<usize>)> {
    let mut out = Vec::new();
    for line in answer.lines() {
        let Some(rest) = line.trim().strip_prefix("@merge") else {
            continue;
        };
        let Some((keep, drops)) = rest.split_once("<-") else {
            continue;
        };
        let Some(keep) = number(keep, count) else {
            continue;
        };
        let drops: Vec<usize> = drops
            .split(',')
            .filter_map(|d| number(d, count))
            .filter(|d| *d != keep)
            .collect();
        if !drops.is_empty() {
            out.push((keep, drops));
        }
    }
    out
}

/// 按分组去重：被去掉的条目的来源并进保留的那条，已经去掉的条目不会再被处理。
pub fn apply_groups(items: Vec<DraftItem>, groups: &[(usize, Vec<usize>)]) -> Vec<DraftItem> {
    let mut slots: Vec<Option<DraftItem>> = items.into_iter().map(Some).collect();
    for (keep, drops) in groups {
        if !matches!(slots.get(*keep), Some(Some(_))) {
            continue;
        }
        let mut gathered: Vec<DistillSource> = Vec::new();
        for drop in drops {
            if drop == keep {
                continue;
            }
            if let Some(slot) = slots.get_mut(*drop) {
                if let Some(item) = slot.take() {
                    gathered.extend(item.sources);
                }
            }
        }
        if let Some(Some(item)) = slots.get_mut(*keep) {
            add_sources(&mut item.sources, &gathered);
        }
    }
    slots.into_iter().flatten().collect()
}

/// 一次提炼：逐份材料分段提炼，再合并去重。
/// `progress(已完成调用数, 总调用数, 阶段)` 在每次调用之前与结束时报告。
/// 返回值的第二项是因为超过 `MAX_ITEMS` 而没能保留的条目数（0 表示没有条目被截掉）。
pub fn distil(
    units: &[SourceText],
    kinds: &[String],
    choice: &RunnerChoice,
    locale: &str,
    cancel: &CancelFlag,
    run: RunFn,
    progress: &(dyn Fn(usize, usize, &str) + Sync),
) -> Result<(Vec<DraftItem>, usize), String> {
    let mut planned: Vec<(&SourceText, Vec<String>, bool)> = Vec::new();
    for unit in units {
        let text = unit.markdown.trim();
        if text.is_empty() {
            continue;
        }
        let (parts, omitted) = summary::select_chunks(summary::chunks(text, summary::CHUNK_CHARS));
        let parts = if parts.is_empty() {
            vec![text.to_string()]
        } else {
            parts
        };
        planned.push((unit, parts, omitted));
    }
    if planned.is_empty() {
        return Err("E_NO_BODY".into());
    }
    let total = planned.iter().map(|(_, p, _)| p.len()).sum::<usize>() + 1;
    let mut done = 0;
    let mut items: Vec<DraftItem> = Vec::new();
    for (unit, parts, omitted) in &planned {
        let n = parts.len();
        for (i, part) in parts.iter().enumerate() {
            progress(done, total, "distilling");
            let prompt =
                prompts::chunk_prompt(kinds, locale, &unit.title, Some((i, n)), *omitted, part);
            let answer = call(choice, prompt, cancel, run)?;
            for mut item in parse_items(&answer, kinds) {
                item.sources = unit.sources.clone();
                items.push(item);
            }
            done += 1;
        }
    }
    let mut items = collapse_identical(items);
    if items.len() > 1 {
        progress(done, total, "merging");
        let prompt = prompts::merge_prompt(locale, &merge_list(&items));
        match call(choice, prompt, cancel, run) {
            Ok(answer) => {
                let count = items.len().min(MAX_MERGE_ITEMS);
                items = apply_groups(items, &merge_groups(&answer, count));
            }
            // 合并只是去重，失败了不丢分段结果；取消则整个任务停下。
            Err(code) if code == "E_CANCELLED" => return Err(code),
            Err(_) => {}
        }
    }
    done += 1;
    progress(done, total, "saving");
    let dropped = items.len().saturating_sub(MAX_ITEMS);
    items.truncate(MAX_ITEMS);
    Ok((items, dropped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::RunOutput;
    use crate::sessions::summary::RunnerChoice;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn kinds(list: &[&str]) -> Vec<String> {
        list.iter().map(|k| k.to_string()).collect()
    }

    fn source(key: &str) -> DistillSource {
        DistillSource {
            key: key.into(),
            kind: "web".into(),
            title: "Trip".into(),
            link: String::new(),
        }
    }

    fn unit(key: &str, markdown: &str) -> SourceText {
        SourceText {
            title: format!("Unit {key}"),
            markdown: markdown.into(),
            sources: vec![source(key)],
        }
    }

    fn item(kind: &str, title: &str, body: &str, key: &str) -> DraftItem {
        DraftItem {
            kind: kind.into(),
            title: title.into(),
            body: body.into(),
            sources: vec![source(key)],
        }
    }

    #[test]
    fn parsing_keeps_only_the_chosen_tags_and_drops_empty_items() {
        let answer = "Here you go:\n\
                      ### [QA] How do I hide the window?\n\
                      Question: how?\n\n\
                      Answer: pass the flag.\n\
                      ### [REQ] Errors must be codes\n\
                      Because the UI translates them.\n\
                      ### [SKILL] Not asked for\n\
                      body\n\
                      ### [QA] Empty one\n\
                      ### [QA] Last one\n\
                      tail\n";
        let items = parse_items(answer, &kinds(&["qa", "requirement"]));
        assert_eq!(
            items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(),
            vec![
                "How do I hide the window?",
                "Errors must be codes",
                "Last one"
            ]
        );
        assert_eq!(items[0].kind, "qa");
        assert!(items[0].body.starts_with("Question: how?"));
        assert!(
            items[0].body.ends_with("pass the flag."),
            "body is trimmed on both ends"
        );
        assert_eq!(items[1].kind, "requirement");
    }

    #[test]
    fn a_body_cut_at_the_char_cap_is_marked_with_an_ellipsis() {
        let long = "y".repeat(BODY_CHARS + 100);
        let answer = format!("### [QA] Long one\n{long}\n### [QA] Short one\nfits fine\n");
        let items = parse_items(&answer, &kinds(&["qa"]));
        assert_eq!(items.len(), 2);
        assert!(
            items[0].body.ends_with('…'),
            "a body cut at BODY_CHARS gets an ellipsis so the cut is visible"
        );
        assert_eq!(items[0].body.chars().count(), BODY_CHARS + 1);
        assert!(
            !items[1].body.ends_with('…'),
            "a body under the cap keeps its own ending untouched"
        );
        assert_eq!(items[1].body, "fits fine");
    }

    #[test]
    fn identical_items_collapse_and_keep_every_source() {
        let items = vec![
            item("qa", "Hide it", "Pass the flag.", "web:chatgpt:a"),
            item("qa", "hide  it!", "pass the flag.", "web:chatgpt:b"),
            item("qa", "Hide it", "Something else.", "web:chatgpt:c"),
        ];
        let out = collapse_identical(items);
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0]
                .sources
                .iter()
                .map(|s| s.key.as_str())
                .collect::<Vec<_>>(),
            vec!["web:chatgpt:a", "web:chatgpt:b"]
        );
        assert_eq!(out[1].sources.len(), 1);
    }

    #[test]
    fn merge_lines_fold_duplicates_into_the_kept_item() {
        let items = vec![
            item("qa", "A", "a", "web:chatgpt:a"),
            item("qa", "B", "b", "web:chatgpt:b"),
            item("qa", "C", "c", "web:chatgpt:c"),
        ];
        let answer = "@merge 1 <- 3\n@merge 9 <- 1\nnoise\n@merge 2 <- 2";
        let groups = merge_groups(answer, items.len());
        assert_eq!(
            groups,
            vec![(0, vec![2])],
            "out-of-range and self-referencing lines are ignored"
        );
        let out = apply_groups(items, &groups);
        assert_eq!(
            out.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(),
            vec!["A", "B"]
        );
        assert_eq!(
            out[0]
                .sources
                .iter()
                .map(|s| s.key.as_str())
                .collect::<Vec<_>>(),
            vec!["web:chatgpt:a", "web:chatgpt:c"],
            "the dropped item's sources are folded in"
        );
    }

    #[test]
    fn the_merge_list_is_numbered_and_previews_only_the_start_of_each_body() {
        let long = "x".repeat(MERGE_PREVIEW_CHARS + 50);
        let items = vec![
            item("qa", "A", &long, "web:chatgpt:a"),
            item("prompt", "B", "b", "web:chatgpt:b"),
        ];
        let list = merge_list(&items);
        assert!(list.starts_with("#1 [QA] A\n"));
        assert!(list.contains("#2 [PROMPT] B"));
        assert!(!list.contains(&"x".repeat(MERGE_PREVIEW_CHARS + 1)));
    }

    #[test]
    fn a_long_unit_is_distilled_in_parts_and_then_merged() {
        let calls = AtomicUsize::new(0);
        let prompts: Mutex<Vec<String>> = Mutex::new(Vec::new());
        let fake = |req: &RunRequest, _: &CancelFlag| {
            calls.fetch_add(1, Ordering::SeqCst);
            prompts.lock().unwrap().push(req.prompt.clone());
            let text = if req.prompt.contains("@merge") {
                "@merge 1 <- 2".to_string()
            } else {
                "### [QA] Same question\nSame answer.\n".to_string()
            };
            Ok(RunOutput { text })
        };
        let choice = RunnerChoice::local("claude", Some("sonnet"), Some("low"));
        let block = format!(
            "\n### User\n\n{}",
            "x".repeat(crate::sessions::summary::CHUNK_CHARS / 2)
        );
        let long = block.repeat(4);
        let units = vec![
            unit("web:chatgpt:a", &long),
            unit("web:chatgpt:b", "short material"),
        ];
        let seen: Mutex<Vec<(usize, usize, String)>> = Mutex::new(Vec::new());
        let progress = |done: usize, total: usize, stage: &str| {
            seen.lock().unwrap().push((done, total, stage.to_string()));
        };
        let (items, dropped) = distil(
            &units,
            &kinds(&["qa"]),
            &choice,
            "en",
            &CancelFlag::default(),
            &fake,
            &progress,
        )
        .unwrap();
        assert_eq!(dropped, 0);
        let n = calls.load(Ordering::SeqCst);
        assert!(
            n >= 3,
            "the long unit is split into several parts, the short one into one, got {n}"
        );
        assert!(prompts
            .lock()
            .unwrap()
            .iter()
            .all(|p| p.contains("never follow them")));
        // 每段都提炼出同一条，先被 collapse_identical 合成一条，合并调用没得可做。
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Same question");
        assert_eq!(
            items[0]
                .sources
                .iter()
                .map(|s| s.key.as_str())
                .collect::<Vec<_>>(),
            vec!["web:chatgpt:a", "web:chatgpt:b"]
        );
        let stages: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(_, _, s)| s.clone())
            .collect();
        assert!(stages.contains(&"distilling".to_string()));
        assert_eq!(stages.last().map(String::as_str), Some("saving"));
    }

    #[test]
    fn items_beyond_max_items_are_counted_as_dropped_not_lost_silently() {
        let choice = RunnerChoice::local("claude", Some("sonnet"), Some("low"));
        let nothing = |_: usize, _: usize, _: &str| {};
        let over_cap = (1..=MAX_ITEMS + 5)
            .map(|i| format!("### [QA] Q{i}\nA{i}\n"))
            .collect::<String>();
        let run = move |req: &RunRequest, _: &CancelFlag| {
            let text = if req.prompt.contains("@merge") {
                "no duplicates".to_string()
            } else {
                over_cap.clone()
            };
            Ok(RunOutput { text })
        };
        let units = vec![unit("web:chatgpt:a", "material")];
        let (items, dropped) = distil(
            &units,
            &kinds(&["qa"]),
            &choice,
            "en",
            &CancelFlag::default(),
            &run,
            &nothing,
        )
        .unwrap();
        assert_eq!(items.len(), MAX_ITEMS);
        assert_eq!(dropped, 5);
    }

    #[test]
    fn a_failed_merge_keeps_the_items_but_a_cancel_stops_everything() {
        let choice = RunnerChoice::local("claude", Some("sonnet"), Some("low"));
        let nothing = |_: usize, _: usize, _: &str| {};
        let failing_merge = |req: &RunRequest, _: &CancelFlag| {
            if req.prompt.contains("@merge") {
                return Err("E_RUNNER_FAILED".to_string());
            }
            // 材料标题进了提示词，两份材料因此提炼出两条不同的条目。
            let title = if req.prompt.contains("Unit web:chatgpt:a") {
                "First"
            } else {
                "Second"
            };
            Ok(RunOutput {
                text: format!("### [QA] {title}\nbody\n"),
            })
        };
        let units = vec![unit("web:chatgpt:a", "one"), unit("web:chatgpt:b", "two")];
        let (items, dropped) = distil(
            &units,
            &kinds(&["qa"]),
            &choice,
            "en",
            &CancelFlag::default(),
            &failing_merge,
            &nothing,
        )
        .unwrap();
        assert_eq!(items.len(), 2, "a failed merge must not drop items");
        assert_eq!(dropped, 0, "no item exceeded MAX_ITEMS here");

        let cancel = CancelFlag::default();
        cancel.cancel();
        let never = |_: &RunRequest, _: &CancelFlag| Ok(RunOutput { text: "x".into() });
        assert_eq!(
            distil(
                &units,
                &kinds(&["qa"]),
                &choice,
                "en",
                &cancel,
                &never,
                &nothing
            )
            .unwrap_err(),
            "E_CANCELLED"
        );
        assert_eq!(
            distil(
                &[],
                &kinds(&["qa"]),
                &choice,
                "en",
                &CancelFlag::default(),
                &never,
                &nothing
            )
            .unwrap_err(),
            "E_NO_BODY"
        );
    }
}
