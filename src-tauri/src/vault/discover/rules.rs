//! 判断一个“名称 = 值”是不是明文密钥：名称按段整段匹配，值要够长、不是占位符、不是路径、
//! 熵够高；已知前缀直接命中并推测平台。

use crate::vault::model::Kind;

const NAME_SEGMENTS: &[&str] = &[
    "KEY",
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "PWD",
    "CREDENTIAL",
    "CREDENTIALS",
    "AUTH",
];
const NAME_CONTAINS: &[&str] = &["APIKEY", "ACCESSKEY"];
const MIN_VALUE_CHARS: usize = 12;
const MIN_PREFIXED_CHARS: usize = 16;
const MIN_ENTROPY: f64 = 3.0;

/// Specific prefixes come before the generic `sk-`.
const PREFIXES: &[(&str, &str, Kind)] = &[
    ("sk-ant-", "Anthropic", Kind::ApiKey),
    ("github_pat_", "GitHub", Kind::Token),
    ("ghp_", "GitHub", Kind::Token),
    ("gho_", "GitHub", Kind::Token),
    ("glpat-", "GitLab", Kind::Token),
    ("xox", "Slack", Kind::Token),
    ("AIza", "Google", Kind::ApiKey),
    ("hf_", "Hugging Face", Kind::Token),
    ("npm_", "npm", Kind::Token),
    ("sk-", "", Kind::ApiKey),
];

pub(crate) fn name_matches(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    if upper
        .split(['_', '-', '.'])
        .any(|segment| NAME_SEGMENTS.contains(&segment))
    {
        return true;
    }
    let squashed: String = upper
        .chars()
        .filter(|c| !matches!(c, '_' | '-' | '.'))
        .collect();
    NAME_CONTAINS.iter().any(|needle| squashed.contains(needle))
}

pub(crate) fn is_placeholder(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    lower.contains("your")
        || lower.starts_with("xxx")
        || [
            "changeme",
            "placeholder",
            "example",
            "replace_me",
            "replace-me",
        ]
        .iter()
        .any(|word| lower.contains(word))
        || (value.starts_with('<') && value.ends_with('>'))
        || value.contains("${")
        || (value.len() > 1 && value.starts_with('%') && value.ends_with('%'))
}

fn looks_like_path(value: &str) -> bool {
    value.contains(":\\")
        || value.starts_with('/')
        || ["\\\\", "~/", "~\\", "./", ".\\", "../", "..\\"]
            .iter()
            .any(|start| value.starts_with(start))
}

fn url_has_credentials(value: &str) -> bool {
    value
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .is_some_and(|authority| authority.contains('@') && authority.contains(':'))
}

/// Shannon entropy in bits per character.
pub(crate) fn entropy(value: &str) -> f64 {
    let chars: Vec<char> = value.chars().collect();
    if chars.is_empty() {
        return 0.0;
    }
    let mut counts = std::collections::HashMap::new();
    for c in &chars {
        *counts.entry(*c).or_insert(0usize) += 1;
    }
    let total = chars.len() as f64;
    counts
        .values()
        .map(|n| {
            let p = *n as f64 / total;
            -p * p.log2()
        })
        .sum()
}

pub(crate) fn value_ok(value: &str) -> bool {
    let value = value.trim();
    if value.chars().count() < MIN_VALUE_CHARS || is_placeholder(value) || looks_like_path(value) {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    if (lower.starts_with("http://") || lower.starts_with("https://"))
        && !url_has_credentials(value)
    {
        return false;
    }
    entropy(value) >= MIN_ENTROPY
}

pub(crate) fn known_prefix(value: &str) -> Option<(&'static str, Kind)> {
    let value = value.trim();
    if value.chars().count() < MIN_PREFIXED_CHARS
        || value.chars().any(char::is_whitespace)
        || !value_ok(value)
    {
        return None;
    }
    PREFIXES
        .iter()
        .find(|(prefix, _, _)| value.starts_with(prefix))
        .map(|(_, platform, kind)| (*platform, *kind))
}

pub(crate) fn classify(name: &str, value: &str) -> Option<(String, Kind)> {
    if let Some((platform, kind)) = known_prefix(value) {
        return Some((platform.to_string(), kind));
    }
    if name_matches(name) && value_ok(value) {
        let is_token = name
            .to_ascii_uppercase()
            .split(['_', '-', '.'])
            .any(|segment| segment == "TOKEN");
        let kind = if is_token { Kind::Token } else { Kind::ApiKey };
        return Some((String::new(), kind));
    }
    None
}

pub(crate) fn primary_field(kind: Kind) -> &'static str {
    if kind == Kind::Token {
        "Token"
    } else {
        "Key"
    }
}

/// `NAME=value` lines: `export ` prefixes, quotes and trailing ` #` comments are understood.
pub(crate) fn parse_env(text: &str) -> Vec<(String, String)> {
    text.strip_prefix('\u{feff}')
        .unwrap_or(text)
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (name, value) = line.split_once('=')?;
            let name = name.trim();
            if name.is_empty() || name.contains(char::is_whitespace) {
                return None;
            }
            let value = value.trim();
            let value = match value.chars().next() {
                Some(quote @ ('"' | '\'')) => value[1..].split(quote).next().unwrap_or(""),
                _ => value.split(" #").next().unwrap_or(value).trim(),
            };
            Some((name.to_string(), value.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Token-shaped samples are assembled at run time so the source holds no real-looking secret.
    fn sample(prefix: &[&str]) -> String {
        let mut parts: Vec<&str> = prefix.to_vec();
        parts.extend(["Zq81", "vKp3", "Lm0X", "w7Rt", "2YbN", "c4Hd"]);
        parts.concat()
    }

    #[test]
    fn names_match_whole_segments() {
        for name in [
            "OPENAI_API_KEY",
            "github-token",
            "db.password",
            "OPENAIAPIKEY",
            "AWS_SECRET_ACCESS_KEY",
            "Auth",
        ] {
            assert!(name_matches(name), "{name}");
        }
        for name in ["MONKEY", "KEYBOARD_LAYOUT", "JAVA_HOME", "TOKENIZER_PATH_X"] {
            assert!(!name_matches(name), "{name}");
        }
    }

    #[test]
    fn values_must_look_like_secrets() {
        assert!(value_ok(&sample(&[])));
        assert!(value_ok(&format!(
            "https://alice:{}@git.host.io",
            sample(&[])
        )));
        for value in [
            "short",
            "your_api_key_here_please",
            "<paste-your-token>",
            "${OPENAI_API_KEY_VALUE}",
            "%OPENAI_API_KEY%",
            "C:\\Users\\me\\keys\\id.pem",
            "/home/me/.ssh/id_ed25519",
            "https://api.service.io/v1/endpoint",
            "aaaaaaaaaaaaaaaaaaaa",
            "changeme-changeme-123",
            ".\\config\\secrets.json",
            "..\\config\\secrets.json",
            "../config/secrets.json",
            "~\\keys\\id_ed25519.pem",
        ] {
            assert!(!value_ok(value), "{value}");
        }
    }

    #[test]
    fn known_prefixes_name_the_platform() {
        assert_eq!(
            known_prefix(&sample(&["gh", "p_"])),
            Some(("GitHub", Kind::Token))
        );
        assert_eq!(
            known_prefix(&sample(&["sk-", "ant-"])),
            Some(("Anthropic", Kind::ApiKey))
        );
        assert_eq!(known_prefix(&sample(&["sk-"])), Some(("", Kind::ApiKey)));
        assert_eq!(known_prefix("sk-your-key-goes-here"), None);
        assert_eq!(known_prefix(&["gh", "p_x"].concat()), None);
    }

    #[test]
    fn a_prefix_hit_must_still_look_like_a_secret() {
        let flat = ["sk-", &"a".repeat(20)].concat();
        assert_eq!(known_prefix(&flat), None);
        assert_eq!(classify("WHATEVER", &flat), None);
        assert_eq!(classify("SERVICE_KEY", &flat), None);
    }

    #[test]
    fn every_listed_prefix_is_recognised() {
        for (prefix, platform, kind) in PREFIXES {
            assert_eq!(
                known_prefix(&sample(&[prefix])),
                Some((*platform, *kind)),
                "{prefix}"
            );
        }
        assert_eq!(
            known_prefix(&sample(&["xox", "b-"])),
            Some(("Slack", Kind::Token))
        );
    }

    #[test]
    fn cloud_key_ids_are_not_prefix_hits() {
        for prefix in [["AK", "IA"], ["AK", "LT"], ["LT", "AI"]] {
            assert_eq!(known_prefix(&sample(&prefix)), None, "{}", prefix.concat());
        }
    }

    #[test]
    fn token_kind_needs_a_whole_token_segment() {
        assert_eq!(
            classify("TOKENIZER_API_KEY", &sample(&[])),
            Some((String::new(), Kind::ApiKey))
        );
        assert_eq!(
            classify("GH_TOKEN", &sample(&[])),
            Some((String::new(), Kind::Token))
        );
    }

    #[test]
    fn a_leading_bom_is_ignored() {
        assert_eq!(
            parse_env("\u{feff}A_TOKEN=one\nB_KEY=two\n"),
            vec![
                ("A_TOKEN".into(), "one".into()),
                ("B_KEY".into(), "two".into())
            ]
        );
    }

    #[test]
    fn classify_combines_prefix_and_name_rules() {
        assert_eq!(
            classify("WHATEVER", &sample(&["gl", "pat-"])),
            Some(("GitLab".into(), Kind::Token))
        );
        assert_eq!(
            classify("SERVICE_TOKEN", &sample(&[])),
            Some((String::new(), Kind::Token))
        );
        assert_eq!(
            classify("SERVICE_KEY", &sample(&[])),
            Some((String::new(), Kind::ApiKey))
        );
        assert_eq!(classify("NODE_ENV", "development"), None);
        assert_eq!(classify("PLAIN_NAME", &sample(&[])), None);
    }

    #[test]
    fn env_files_are_parsed_like_dotenv() {
        let text = "# comment\nexport A_KEY=one\nB_TOKEN = \"two words\" \nC_SECRET='three' # tail\nD_PASSWORD=four # note\nnot a line\n=nothing\n";
        assert_eq!(
            parse_env(text),
            vec![
                ("A_KEY".into(), "one".into()),
                ("B_TOKEN".into(), "two words".into()),
                ("C_SECRET".into(), "three".into()),
                ("D_PASSWORD".into(), "four".into()),
            ]
        );
    }
}
