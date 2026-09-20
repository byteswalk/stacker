//! skill 草稿：只在 Stacker 数据目录下写文件夹，永远不安装到任何智能体目录。
use super::pipeline::DraftItem;
use crate::sessions::export::safe;
use std::path::{Path, PathBuf};

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 每个来源在 `excerpts.md` 里最多写多少字的原文。
pub const EXCERPT_CHARS: usize = 4_000;

/// 文件夹名：沿用会话导出的字符清洗（保留中文），再避开 Windows 设备名。
pub fn folder_name(title: &str) -> String {
    if title.trim().is_empty() {
        return "skill".into();
    }
    let base = safe(title, 60);
    let stem = base.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        format!("_{base}")
    } else {
        base
    }
}

fn unique_dir(skills: &Path, name: &str) -> Result<PathBuf, String> {
    for n in 1..100 {
        let candidate = skills.join(if n == 1 {
            name.to_string()
        } else {
            format!("{name} ({n})")
        });
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("E_STORAGE".into())
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// YAML 里的双引号字符串：转义反斜杠与双引号，任何标题（包含冒号、方括号等会破坏
/// YAML 语法的字符）都能安全放进 `name:`/`description:` 字段。
fn yaml_quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn zh(locale: &str) -> bool {
    locale.starts_with("zh")
}

struct SkillTexts {
    sources_heading: &'static str,
    open_paren: &'static str,
    close_paren: &'static str,
    draft_note: &'static str,
    supporting_material: &'static str,
    source_label: &'static str,
    link_label: &'static str,
    no_original: &'static str,
    truncated: &'static str,
}

fn texts(locale: &str) -> SkillTexts {
    if zh(locale) {
        SkillTexts {
            sources_heading: "来源",
            open_paren: "（",
            close_paren: "）",
            draft_note:
                "这是草稿：Stacker 只生成了文件，没有安装到任何智能体；依据原文见 excerpts.md。",
            supporting_material: "依据原文",
            source_label: "来源：",
            link_label: "链接：",
            no_original: "（没有保存原文。）",
            truncated: "\n…（已截断）",
        }
    } else {
        SkillTexts {
            sources_heading: "Sources",
            open_paren: "(",
            close_paren: ")",
            draft_note: "This is a draft: Stacker only wrote the files and installed nothing into any agent; the supporting material is in excerpts.md.",
            supporting_material: "Supporting material",
            source_label: "Source: ",
            link_label: "Link: ",
            no_original: "(No original text was kept.)",
            truncated: "\n…(truncated)",
        }
    }
}

fn skill_markdown(name: &str, item: &DraftItem, locale: &str) -> String {
    let t = texts(locale);
    let mut out = format!(
        "---\nname: {}\ndescription: {}\n---\n\n# {}\n\n{}\n\n## {}\n\n",
        yaml_quote(name),
        yaml_quote(&one_line(&item.title)),
        item.title,
        item.body,
        t.sources_heading,
    );
    for s in &item.sources {
        out.push_str(&format!(
            "- {} {}{}{}\n",
            s.title, t.open_paren, s.key, t.close_paren
        ));
    }
    out.push_str(&format!("\n{}\n", t.draft_note));
    out
}

fn excerpts_markdown(item: &DraftItem, evidence: &[(Vec<String>, String)], locale: &str) -> String {
    let t = texts(locale);
    let mut out = format!("# {} · {}\n\n", item.title, t.supporting_material);
    for s in &item.sources {
        out.push_str(&format!(
            "## {}\n\n- {}{}\n",
            s.title, t.source_label, s.key
        ));
        if !s.link.is_empty() {
            out.push_str(&format!("- {}{}\n", t.link_label, s.link));
        }
        out.push('\n');
        match evidence
            .iter()
            .find(|(keys, _)| keys.iter().any(|k| k == &s.key))
        {
            Some((_, text)) => {
                let cut: String = text.chars().take(EXCERPT_CHARS).collect();
                out.push_str(&cut);
                if text.chars().count() > EXCERPT_CHARS {
                    out.push_str(t.truncated);
                }
                out.push_str("\n\n");
            }
            None => out.push_str(&format!("{}\n\n", t.no_original)),
        }
    }
    out
}

/// 写 `<skills>/<名称>/SKILL.md` 与 `excerpts.md`，返回实际用的文件夹名。
/// `evidence` 是 `(那份材料涉及的来源键列表, 那份材料的原文)`，一份材料只克隆一次原文，
/// 用来写 `excerpts.md`。
pub fn write_draft(
    skills: &Path,
    item: &DraftItem,
    evidence: &[(Vec<String>, String)],
    locale: &str,
) -> Result<String, String> {
    let storage = |_| "E_STORAGE".to_string();
    std::fs::create_dir_all(skills).map_err(storage)?;
    let dir = unique_dir(skills, &folder_name(&item.title))?;
    std::fs::create_dir_all(&dir).map_err(storage)?;
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or("E_STORAGE")?;
    std::fs::write(dir.join("SKILL.md"), skill_markdown(&name, item, locale)).map_err(storage)?;
    std::fs::write(
        dir.join("excerpts.md"),
        excerpts_markdown(item, evidence, locale),
    )
    .map_err(storage)?;
    Ok(name)
}

/// 在资源管理器里打开一个草稿文件夹；只允许 `<skills>` 之下已存在的一级目录。
pub fn open_folder(skills: &Path, name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.contains("..") {
        return Err("E_REQUEST".into());
    }
    let dir = skills.join(name);
    if !dir.is_dir() {
        return Err("E_NOT_FOUND".into());
    }
    crate::sessions::commands::explorer(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distill::DistillSource;

    fn item(title: &str) -> DraftItem {
        DraftItem {
            kind: "skill".into(),
            title: title.into(),
            body: "When to use it: always.\n\nSteps:\n1. Do the thing.".into(),
            sources: vec![
                DistillSource {
                    key: "web:chatgpt:a".into(),
                    kind: "web".into(),
                    title: "Trip plan".into(),
                    link: String::new(),
                },
                DistillSource {
                    key: "excerpt:e1".into(),
                    kind: "excerpt".into(),
                    title: "Budget".into(),
                    link: "https://chatgpt.com/c/a".into(),
                },
            ],
        }
    }

    #[test]
    fn folder_names_are_safe_and_never_a_device_name() {
        assert_eq!(folder_name("Plan a trip"), "Plan a trip");
        assert_eq!(folder_name("a/b:c*d"), "a_b_c_d");
        assert_eq!(folder_name("CON"), "_CON");
        assert_eq!(folder_name("con.txt"), "_con.txt");
        assert_eq!(folder_name("   "), "skill");
        assert!(folder_name(&"x".repeat(200)).chars().count() <= 60);
    }

    #[test]
    fn a_draft_writes_both_files_and_never_installs_anything() {
        let dir = tempfile::tempdir().unwrap();
        let skills = dir.path().join("skills");
        let evidence = vec![(
            vec!["web:chatgpt:a".to_string()],
            format!(
                "# Trip plan\n\n### User\n\n{}",
                "y".repeat(EXCERPT_CHARS + 100)
            ),
        )];
        let name = write_draft(&skills, &item("Plan a trip"), &evidence, "zh-CN").unwrap();
        assert_eq!(name, "Plan a trip");
        let folder = skills.join(&name);
        let skill = std::fs::read_to_string(folder.join("SKILL.md")).unwrap();
        assert!(skill.starts_with("---\nname: \"Plan a trip\"\n"));
        assert!(skill.contains("Do the thing."));
        assert!(skill.contains("web:chatgpt:a") && skill.contains("excerpt:e1"));
        let excerpts = std::fs::read_to_string(folder.join("excerpts.md")).unwrap();
        assert!(excerpts.contains("https://chatgpt.com/c/a"));
        assert!(excerpts.contains(&"y".repeat(100)));
        assert!(
            !excerpts.contains(&"y".repeat(EXCERPT_CHARS + 1)),
            "the excerpt has a length cap"
        );
        // 第二份同名草稿不覆盖第一份。
        let again = write_draft(&skills, &item("Plan a trip"), &evidence, "zh-CN").unwrap();
        assert_eq!(again, "Plan a trip (2)");
        assert!(skills.join("Plan a trip (2)").join("SKILL.md").is_file());
    }

    #[test]
    fn a_title_with_yaml_breaking_characters_stays_valid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let skills = dir.path().join("skills");
        let draft = item("Trip: the \"best\" plan");
        let name = write_draft(&skills, &draft, &[], "en").unwrap();
        let skill = std::fs::read_to_string(skills.join(&name).join("SKILL.md")).unwrap();
        // The folder name is filesystem-safe (colon and quote were stripped by `safe()`), so
        // `name:` never needed escaping here, but the title itself, spliced raw into
        // `description:`, must have its backslash and quote escaped so the front matter parses.
        assert!(
            skill.contains("description: \"Trip: the \\\"best\\\" plan\"\n"),
            "{skill}"
        );
        // An unescaped `"` right after the colon would have made this an invalid YAML scalar;
        // the front matter's `name:` line must be a properly quoted string too.
        let lines: Vec<&str> = skill.lines().collect();
        assert_eq!(lines[0], "---");
        assert!(lines[1].starts_with("name: \"") && lines[1].ends_with('"'));
        assert_eq!(lines[3], "---");
    }

    #[test]
    fn write_draft_uses_english_headings_for_non_chinese_locales() {
        let dir = tempfile::tempdir().unwrap();
        let skills = dir.path().join("skills");
        let evidence = vec![(
            vec!["web:chatgpt:a".to_string(), "excerpt:e1".to_string()],
            "the original text".to_string(),
        )];
        let name = write_draft(&skills, &item("Plan a trip"), &evidence, "en").unwrap();
        let folder = skills.join(&name);
        let skill = std::fs::read_to_string(folder.join("SKILL.md")).unwrap();
        assert!(skill.contains("\n## Sources\n\n"));
        assert!(skill.contains("Trip plan (web:chatgpt:a)"));
        assert!(skill.contains("This is a draft: Stacker only wrote the files"));
        assert!(!skill.contains("来源") && !skill.contains("草稿"));
        let excerpts = std::fs::read_to_string(folder.join("excerpts.md")).unwrap();
        assert!(excerpts.contains("· Supporting material"));
        assert!(excerpts.contains("- Source: web:chatgpt:a"));
        assert!(excerpts.contains("- Link: https://chatgpt.com/c/a"));
        assert!(
            excerpts.contains("the original text"),
            "shared evidence is found by either source key"
        );
        assert!(!excerpts.contains("来源") && !excerpts.contains("依据原文"));
    }

    #[test]
    fn a_missing_evidence_entry_says_so_in_the_right_locale() {
        let dir = tempfile::tempdir().unwrap();
        let skills = dir.path().join("skills");
        let name = write_draft(&skills, &item("Plan a trip"), &[], "en").unwrap();
        let excerpts = std::fs::read_to_string(skills.join(name).join("excerpts.md")).unwrap();
        assert!(excerpts.contains("(No original text was kept.)"));
    }

    #[test]
    fn opening_a_folder_stays_inside_the_skills_folder() {
        let dir = tempfile::tempdir().unwrap();
        let skills = dir.path().join("skills");
        write_draft(&skills, &item("Plan a trip"), &[], "zh-CN").unwrap();
        for bad in ["..", "../x", "a/b", "a\\b", "C:/Windows", ""] {
            assert_eq!(open_folder(&skills, bad).unwrap_err(), "E_REQUEST", "{bad}");
        }
        assert_eq!(open_folder(&skills, "missing").unwrap_err(), "E_NOT_FOUND");
    }
}
