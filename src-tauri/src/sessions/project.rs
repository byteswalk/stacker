use super::model::ProjectRef;

/// Stable key for grouping: lowercase, `\` separators, no `\\?\` prefix or trailing slash.
pub fn project_key(path: &str) -> String {
    path.trim()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// Claude worktree sessions (`<repo>\.claude\worktrees\<name>`) belong to the repository.
pub fn repository_root(path: &str) -> String {
    let normalized = path.trim().trim_start_matches(r"\\?\").replace('/', "\\");
    match normalized.to_lowercase().find(r"\.claude\worktrees\") {
        Some(index) => normalized[..index].to_string(),
        None => normalized.trim_end_matches('\\').to_string(),
    }
}

pub fn display_name(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("未知项目")
        .to_string()
}

pub fn project_ref(path: &str, name: Option<&str>) -> ProjectRef {
    let root = repository_root(path);
    ProjectRef {
        key: project_key(&root),
        name: name
            .map(str::to_string)
            .unwrap_or_else(|| display_name(&root)),
        exists: !root.is_empty() && std::path::Path::new(&root).is_dir(),
        path: root,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_ignore_case_separators_and_prefix() {
        assert_eq!(
            project_key(r"\\?\D:\Projects\App\"),
            project_key("d:/projects/app")
        );
        assert_eq!(project_key(""), "");
    }

    #[test]
    fn claude_worktrees_fold_into_their_repository() {
        assert_eq!(
            repository_root(r"E:\VibeCoding\repo\.claude\worktrees\nexa-android-planb3"),
            r"E:\VibeCoding\repo"
        );
        assert_eq!(
            repository_root(r"E:\VibeCoding\repo\src"),
            r"E:\VibeCoding\repo\src"
        );
    }

    #[test]
    fn display_names_use_the_last_segment() {
        assert_eq!(display_name(r"D:\Projects\rust\envswitch"), "envswitch");
        assert_eq!(display_name(""), "未知项目");
    }
}
