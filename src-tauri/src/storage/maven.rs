use quick_xml::{events::Event, Reader};
use std::ops::Range;

struct Repository {
    range: Range<usize>,
    value: String,
}

struct Settings {
    repository: Option<Repository>,
    closing: usize,
    empty_root: Option<Range<usize>>,
    root_name: String,
}

fn parse(raw: &str) -> Result<Settings, String> {
    let mut reader = Reader::from_str(raw);
    let mut depth = 0;
    let mut root_seen = false;
    let mut repository_start = None;
    let mut settings = Settings {
        repository: None,
        closing: 0,
        empty_root: None,
        root_name: String::new(),
    };
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|error| error.to_string())?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Start(ref tag) | Event::Empty(ref tag) => {
                let empty = matches!(event, Event::Empty(_));
                if depth == 0 {
                    if root_seen || tag.local_name().as_ref() != b"settings" {
                        return Err("Expected a single settings root element".into());
                    }
                    root_seen = true;
                    settings.root_name = String::from_utf8_lossy(tag.name().as_ref()).into_owned();
                    if empty {
                        settings.empty_root = Some(start..end);
                    }
                } else if repository_start.is_some() {
                    return Err("localRepository must contain a path, not nested elements".into());
                } else if depth == 1 && tag.local_name().as_ref() == b"localRepository" {
                    if settings.repository.is_some() {
                        return Err("Duplicate localRepository elements".into());
                    }
                    settings.repository = Some(Repository {
                        range: start..end,
                        value: String::new(),
                    });
                    if !empty {
                        repository_start = Some(start);
                    }
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::End(_) => {
                if depth == 0 {
                    return Err("Unexpected closing element".into());
                }
                if depth == 2 {
                    if let Some(start) = repository_start.take() {
                        settings.repository.as_mut().unwrap().range = start..end;
                    }
                }
                if depth == 1 {
                    settings.closing = start;
                }
                depth -= 1;
            }
            Event::Text(text) => {
                let text = text.decode().map_err(|error| error.to_string())?;
                if repository_start.is_some() {
                    settings.repository.as_mut().unwrap().value.push_str(&text);
                } else if depth == 0 && !text.trim().is_empty() {
                    return Err("Unexpected text outside settings".into());
                }
            }
            Event::CData(text) if repository_start.is_some() => {
                settings
                    .repository
                    .as_mut()
                    .unwrap()
                    .value
                    .push_str(&text.decode().map_err(|error| error.to_string())?);
            }
            Event::GeneralRef(entity) if repository_start.is_some() => {
                let entity = format!("&{};", entity.decode().map_err(|error| error.to_string())?);
                settings.repository.as_mut().unwrap().value.push_str(
                    &quick_xml::escape::unescape(&entity).map_err(|error| error.to_string())?,
                );
            }
            Event::DocType(_) => return Err("DOCTYPE is not supported in settings".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen || depth != 0 {
        return Err("Missing or unclosed settings root".into());
    }
    Ok(settings)
}

pub(super) fn local_repository(raw: &str) -> Result<Option<String>, String> {
    Ok(parse(raw)?
        .repository
        .map(|item| item.value.trim().to_string())
        .filter(|value| !value.is_empty()))
}

pub(super) fn update(raw: &str, value: Option<&str>) -> Result<String, String> {
    let settings = parse(raw)?;
    let prefix = settings
        .root_name
        .rsplit_once(':')
        .map(|(prefix, _)| format!("{prefix}:"))
        .unwrap_or_default();
    let tag = format!("{prefix}localRepository");
    let replacement = value
        .map(|value| format!("<{tag}>{}</{tag}>", quick_xml::escape::escape(value)))
        .unwrap_or_default();
    let mut output = raw.to_string();
    if let Some(repository) = settings.repository {
        output.replace_range(repository.range, &replacement);
    } else if value.is_some() {
        if let Some(root) = settings.empty_root {
            let opening = &raw[root.start..root.end - 2];
            output.replace_range(
                root,
                &format!("{opening}>\n  {replacement}\n</{}>", settings.root_name),
            );
        } else {
            output.insert_str(settings.closing, &format!("  {replacement}\n"));
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_commented_examples_and_preserves_unrelated_xml() {
        let raw = "<settings><!-- <localRepository>/example</localRepository> --><mirrors/><localRepository>D:/old</localRepository></settings>";
        assert_eq!(local_repository(raw).unwrap().as_deref(), Some("D:/old"));
        let changed = update(raw, Some("D:/Dev & Cache")).unwrap();
        assert!(changed.contains("<!-- <localRepository>/example</localRepository> -->"));
        assert!(changed.contains("<mirrors/>"));
        assert_eq!(
            local_repository(&changed).unwrap().as_deref(),
            Some("D:/Dev & Cache")
        );
        assert_eq!(
            local_repository(&update(&changed, None).unwrap()).unwrap(),
            None
        );
    }

    #[test]
    fn inserts_after_a_commented_example() {
        let raw = "<settings><!-- <localRepository>/example</localRepository> --></settings>";
        assert_eq!(local_repository(raw).unwrap(), None);
        assert_eq!(
            local_repository(&update(raw, Some("D:/cache")).unwrap())
                .unwrap()
                .as_deref(),
            Some("D:/cache")
        );
    }

    #[test]
    fn accepts_empty_namespaced_and_cdata_settings() {
        for raw in [
            "<settings/>",
            "<m:settings xmlns:m='http://maven.apache.org/SETTINGS/1.0.0'/>",
            "<settings><localRepository/></settings>",
        ] {
            let changed = update(raw, Some("D:/cache")).unwrap();
            assert_eq!(
                local_repository(&changed).unwrap().as_deref(),
                Some("D:/cache")
            );
        }
        assert_eq!(local_repository("<settings><localRepository><![CDATA[D:/Dev & Cache]]></localRepository></settings>").unwrap().as_deref(), Some("D:/Dev & Cache"));
    }

    #[test]
    fn refuses_malformed_or_ambiguous_settings() {
        for raw in [
            "<settings>",
            "<other/>",
            "<settings/><settings/>",
            "<settings><localRepository/><localRepository/></settings>",
            "<settings><localRepository><nested/></localRepository></settings>",
        ] {
            assert!(update(raw, Some("D:/cache")).is_err(), "{raw}");
        }
    }
}
