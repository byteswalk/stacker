//! Passwords a browser exported as CSV (Chrome, Edge, Firefox): each login becomes a general
//! credential. The browser's own store is never read: since Chrome 127 its passwords are
//! bound to Chrome itself, and getting round that is what password stealers do.

use super::errors::{CORRUPT, IO};
use super::model::{Body, EntryInput, FieldInput, Kind};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use zeroize::Zeroizing;

const LARGEST_FILE: u64 = 20 * 1024 * 1024;
pub(crate) const URL_FIELD: &str = "网址";
pub(crate) const USER_FIELD: &str = "账号";
pub(crate) const PASSWORD_FIELD: &str = "密码";

/// One login from the file.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Login {
    pub title: String,
    pub url: String,
    pub host: String,
    pub user: String,
    pub password: Zeroizing<String>,
    pub note: String,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserStats {
    /// Logins not yet in the vault.
    pub added: usize,
    /// Logins the vault already holds (same site, account and password).
    pub same: usize,
    /// Rows without a password.
    pub empty: usize,
}

/// RFC 4180: fields split by commas, quoted fields may hold commas, line breaks and `""`.
pub(crate) fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.retain(|row| row.iter().any(|cell| !cell.trim().is_empty()));
    rows
}

fn host_of(url: &str) -> String {
    url::Url::parse(url.trim())
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.trim_start_matches("www.").to_string())
        })
        .unwrap_or_default()
}

/// Which system a login belongs to: scheme, host, port and path. Another port, another path, even
/// another page on the same host may be another system (a router's page, a NAS on :1188,
/// two apps under one address), so only all of them together are taken for one system;
/// http and https are two. What does not tell systems apart is left out: the case of the
/// host, a leading `www.`, the scheme's own port written or not, a trailing slash, the query and
/// the part after `#`. Without a parseable address, the text as it is.
pub(crate) fn system_of(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url.trim()) else {
        return url.trim().to_string();
    };
    let Some(host) = parsed.host_str() else {
        return url.trim().to_string();
    };
    // `port()` is empty for the scheme's own port, written or not.
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let path = parsed.path().trim_end_matches('/');
    let host = host.trim_start_matches("www.");
    format!("{}://{host}{port}{path}", parsed.scheme())
}

/// The browsers a password file is written for; each imports its own export format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Browser {
    Chrome,
    Edge,
    Firefox,
}

/// The logins worth handing to a browser: live entries with an address and a password.
fn exportable<'a>(
    body: &'a Body,
    ids: &'a [String],
) -> impl Iterator<Item = &'a super::model::Entry> + 'a {
    body.entries.iter().filter(move |entry| {
        entry.deleted_at.is_none()
            && entry.kind != Kind::SshKey
            && (ids.is_empty() || ids.contains(&entry.id))
            && !field(&entry.fields, URL_FIELD).trim().is_empty()
            && !field(&entry.fields, PASSWORD_FIELD).is_empty()
    })
}

/// How many of these entries a browser could take (all entries when `ids` is empty).
pub(crate) fn exportable_count(body: &Body, ids: &[String], browser: Browser) -> usize {
    exportable(body, ids)
        .filter(|entry| {
            browser != Browser::Firefox || origin(field(&entry.fields, URL_FIELD)).is_some()
        })
        .count()
}

/// `scheme://host[:port]`: all Firefox keeps of an address. Only web addresses have one.
fn origin(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    matches!(parsed.scheme(), "http" | "https").then(|| parsed.origin().ascii_serialization())
}

fn csv_cell(value: &str, always: bool) -> String {
    if always
        || value.contains([',', '"', '\r', '\n'])
        || value.starts_with(' ')
        || value.ends_with(' ')
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// The password file `browser` imports, as that browser writes its own export: Chrome and
/// Edge `name,url,username,password,note`; Firefox every cell quoted, with the origin and
/// times it keeps. Returns the text and how many logins it holds.
pub(crate) fn export_csv(
    body: &Body,
    ids: &[String],
    browser: Browser,
) -> (Zeroizing<String>, usize) {
    let mut out = Zeroizing::new(String::new());
    let mut count = 0;
    match browser {
        Browser::Chrome | Browser::Edge => {
            out.push_str("name,url,username,password,note\r\n");
            for entry in exportable(body, ids) {
                let row = [
                    entry.title.as_str(),
                    field(&entry.fields, URL_FIELD).trim(),
                    field(&entry.fields, USER_FIELD),
                    field(&entry.fields, PASSWORD_FIELD),
                    entry.note.as_str(),
                ]
                .map(|cell| csv_cell(cell, false));
                out.push_str(&row.join(","));
                out.push_str("\r\n");
                count += 1;
            }
        }
        Browser::Firefox => {
            out.push_str("\"url\",\"username\",\"password\",\"httpRealm\",\"formActionOrigin\",\"guid\",\"timeCreated\",\"timeLastUsed\",\"timePasswordChanged\"\r\n");
            for entry in exportable(body, ids) {
                let Some(origin) = origin(field(&entry.fields, URL_FIELD)) else {
                    continue;
                };
                let (created, changed) =
                    (entry.created_at.to_string(), entry.updated_at.to_string());
                let row = [
                    origin.as_str(),
                    field(&entry.fields, USER_FIELD),
                    field(&entry.fields, PASSWORD_FIELD),
                    "",
                    origin.as_str(),
                    "",
                    created.as_str(),
                    changed.as_str(),
                    changed.as_str(),
                ]
                .map(|cell| csv_cell(cell, true));
                out.push_str(&row.join(","));
                out.push_str("\r\n");
                count += 1;
            }
        }
    }
    (out, count)
}

/// The logins in a browser's export, by the columns its header names. Chrome and Edge write
/// `name,url,username,password[,note]`; Firefox writes `url,username,password,…` without a
/// name. A file without a `password` column is not one of these.
pub(crate) fn logins(text: &str) -> Result<(Vec<Login>, usize), String> {
    let mut rows = parse_csv(text).into_iter();
    let header: Vec<String> = rows
        .next()
        .ok_or(CORRUPT)?
        .into_iter()
        .map(|cell| cell.trim().to_ascii_lowercase())
        .collect();
    let column = |names: &[&str]| {
        header
            .iter()
            .position(|cell| names.contains(&cell.as_str()))
    };
    let password = column(&["password"]).ok_or(CORRUPT)?;
    let url = column(&["url", "origin", "login_uri"]);
    let user = column(&["username", "login_username"]);
    let name = column(&["name", "title"]);
    let note = column(&["note", "notes"]);
    let cell = |row: &[String], at: Option<usize>| {
        at.and_then(|i| row.get(i))
            .map(|v| v.trim().to_string())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    let mut empty = 0;
    for row in rows {
        let secret = row.get(password).cloned().unwrap_or_default();
        if secret.is_empty() {
            empty += 1;
            continue;
        }
        let url = cell(&row, url);
        let host = host_of(&url);
        let user = cell(&row, user);
        let named = cell(&row, name);
        let title = if !named.is_empty() {
            named
        } else if !host.is_empty() {
            host.clone()
        } else if !url.is_empty() {
            url.clone()
        } else {
            user.clone()
        };
        out.push(Login {
            title: if title.is_empty() {
                "登录".into()
            } else {
                title
            },
            url,
            host,
            user,
            password: Zeroizing::new(secret),
            note: cell(&row, note),
        });
    }
    Ok((out, empty))
}

/// The login's identity: site, account and password, so the same login is not saved twice.
/// One login: its system (see `system_of`), account and password.
fn identity(system: &str, user: &str, password: &str) -> String {
    super::crypto::secret_digest(&format!(
        "{}\n{}\n{}",
        system.to_lowercase(),
        user,
        password
    ))
}

fn field<'a>(fields: &'a [super::model::Field], name: &str) -> &'a str {
    fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.as_str())
        .unwrap_or_default()
}

/// The logins worth adding: not already in the vault, not twice in the file.
pub(crate) fn new_logins(body: &Body, logins: Vec<Login>) -> (Vec<EntryInput>, usize) {
    let mut known: HashSet<String> = body
        .entries
        .iter()
        .filter(|entry| entry.deleted_at.is_none())
        .map(|entry| {
            let url = field(&entry.fields, URL_FIELD);
            let system = if url.trim().is_empty() {
                entry.platform.to_lowercase()
            } else {
                system_of(url)
            };
            identity(
                &system,
                field(&entry.fields, USER_FIELD),
                field(&entry.fields, PASSWORD_FIELD),
            )
        })
        .collect();
    let mut same = 0;
    let mut inputs = Vec::new();
    for login in logins {
        let system = if login.url.trim().is_empty() {
            login.host.to_lowercase()
        } else {
            system_of(&login.url)
        };
        if !known.insert(identity(&system, &login.user, &login.password)) {
            same += 1;
            continue;
        }
        let mut fields = Vec::new();
        if !login.url.is_empty() {
            fields.push(FieldInput {
                name: URL_FIELD.into(),
                previous_name: None,
                value: Some(login.url.clone()),
                secret: false,
            });
        }
        if !login.user.is_empty() {
            fields.push(FieldInput {
                name: USER_FIELD.into(),
                previous_name: None,
                value: Some(login.user.clone()),
                secret: false,
            });
        }
        fields.push(FieldInput {
            name: PASSWORD_FIELD.into(),
            previous_name: None,
            value: Some(login.password.to_string()),
            secret: true,
        });
        inputs.push(EntryInput {
            id: None,
            title: login.title,
            platform: login.host,
            kind: Kind::Other,
            fields,
            expires_at: None,
            tags: vec!["浏览器".into()],
            note: login.note,
            favorite: false,
        });
    }
    (inputs, same)
}

pub(crate) fn read(path: &Path) -> Result<Zeroizing<String>, String> {
    let meta = std::fs::metadata(path).map_err(|_| IO.to_string())?;
    if meta.len() > LARGEST_FILE {
        return Err(CORRUPT.into());
    }
    let bytes = Zeroizing::new(std::fs::read(path).map_err(|_| IO.to_string())?);
    Ok(Zeroizing::new(String::from_utf8_lossy(&bytes).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::model::{apply_input, Body};

    const CHROME: &str = "\u{feff}name,url,username,password,note\r\n\
        github.com,https://github.com/login,me,\"p,1\"\"x\",\"two\nlines\"\r\n\
        ,https://www.example.com/,ann,pw2,\r\n\
        nopass.com,https://nopass.com/,x,,\r\n";

    #[test]
    fn quoted_commas_quotes_and_line_breaks_survive() {
        let rows = parse_csv(CHROME);
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows[1],
            vec![
                "github.com",
                "https://github.com/login",
                "me",
                "p,1\"x",
                "two\nlines"
            ]
        );
        assert_eq!(rows[0][0], "name", "the byte order mark is dropped");
    }

    #[test]
    fn a_chrome_export_becomes_logins_and_rows_without_a_password_are_counted() {
        let (found, empty) = logins(CHROME).unwrap();
        assert_eq!(empty, 1);
        assert_eq!(found.len(), 2);
        assert_eq!(
            (
                found[0].title.as_str(),
                found[0].host.as_str(),
                found[0].user.as_str()
            ),
            ("github.com", "github.com", "me")
        );
        assert_eq!(found[0].password.as_str(), "p,1\"x");
        assert_eq!(found[0].note, "two\nlines");
        // No name: the site stands in for it.
        assert_eq!(found[1].title, "example.com");
    }

    #[test]
    fn a_firefox_export_is_read_by_its_own_header_and_anything_else_is_refused() {
        let firefox = "\"url\",\"username\",\"password\",\"httpRealm\",\"formActionOrigin\",\"guid\"\n\
            \"https://mail.example.org\",\"bob\",\"hunter2\",,\"https://mail.example.org\",\"{1}\"\n";
        let (found, _) = logins(firefox).unwrap();
        assert_eq!(
            (
                found[0].title.as_str(),
                found[0].user.as_str(),
                found[0].password.as_str()
            ),
            ("mail.example.org", "bob", "hunter2")
        );
        assert_eq!(logins("a,b\n1,2\n").err().unwrap(), CORRUPT);
    }

    #[test]
    fn logins_already_in_the_vault_or_twice_in_the_file_are_added_once() {
        let mut body = Body::default();
        let (found, _) = logins(CHROME).unwrap();
        let (inputs, same) = new_logins(&body, found);
        assert_eq!((inputs.len(), same), (2, 0));
        assert_eq!(
            inputs[0]
                .fields
                .iter()
                .map(|f| (f.name.as_str(), f.secret))
                .collect::<Vec<_>>(),
            vec![
                (URL_FIELD, false),
                (USER_FIELD, false),
                (PASSWORD_FIELD, true)
            ]
        );
        for input in inputs {
            apply_input(&mut body, input, 1).unwrap();
        }
        // New entries stay out of Windows Credential Manager.
        assert!(body
            .entries
            .iter()
            .all(|entry| entry.windows == Some(false)));

        let twice = format!("{CHROME}github.com,https://github.com/login,me,\"p,1\"\"x\",\nother.com,https://other.com/,me,pw3,\n");
        let (inputs, same) = new_logins(&body, logins(&twice).unwrap().0);
        assert_eq!(same, 3);
        assert_eq!(
            inputs.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(),
            vec!["other.com"]
        );
    }

    #[test]
    fn a_file_each_browser_imports_and_this_one_reads_back() {
        let mut body = Body::default();
        let (found, _) = logins(CHROME).unwrap();
        for input in new_logins(&body, found).0 {
            apply_input(&mut body, input, 7).unwrap();
        }
        // An entry with no address, and an app login Firefox cannot keep.
        let csv = "name,url,username,password\nnone,,u,p\napp,android://hash@com.example/,u,p2\n";
        for input in new_logins(&body, logins(csv).unwrap().0).0 {
            apply_input(&mut body, input, 8).unwrap();
        }

        let (chrome, count) = export_csv(&body, &[], Browser::Chrome);
        assert_eq!(count, 3);
        assert!(chrome.starts_with("name,url,username,password,note\r\n"));
        assert!(
            chrome.contains("github.com,https://github.com/login,me,\"p,1\"\"x\",\"two\nlines\"")
        );
        assert_eq!(exportable_count(&body, &[], Browser::Edge), 3);
        // Read back by the importer, every login comes out as it went in.
        let (again, _) = logins(&chrome).unwrap();
        assert_eq!(new_logins(&body, again).1, 3, "all already in the vault");

        let (firefox, count) = export_csv(&body, &[], Browser::Firefox);
        assert_eq!(
            logins(&firefox).unwrap().0.len(),
            2,
            "this importer reads Firefox's format too"
        );
        assert_eq!(count, 2, "the app login has no web origin");
        assert_eq!(exportable_count(&body, &[], Browser::Firefox), 2);
        assert!(firefox.starts_with("\"url\",\"username\",\"password\",\"httpRealm\""));
        assert!(firefox.contains("\"https://github.com\",\"me\",\"p,1\"\"x\",\"\",\"https://github.com\",\"\",\"7\",\"7\",\"7\""));

        let one = body.entries[0].id.clone();
        assert_eq!(export_csv(&body, &[one], Browser::Chrome).1, 1);
    }

    #[test]
    fn only_the_same_address_is_the_same_system() {
        assert_eq!(
            system_of("HTTPS://192.168.2.1:443/#top"),
            system_of("https://192.168.2.1")
        );
        assert_eq!(
            system_of("http://Router.lan:80/admin/"),
            system_of("http://router.lan/admin")
        );
        assert_ne!(
            system_of("https://192.168.2.1/userLogin.asp"),
            system_of("https://192.168.2.1/")
        );
        assert_eq!(
            system_of("https://www.github.com/login"),
            system_of("https://github.com/login")
        );
        assert_eq!(
            system_of("https://host/login?app=1"),
            system_of("https://host/login?next=/")
        );
        assert_ne!(
            system_of("https://192.168.2.1/userLogin.asp"),
            system_of("http://192.168.2.1:1188/")
        );
        assert_ne!(
            system_of("http://192.168.2.1/"),
            system_of("https://192.168.2.1/")
        );
        assert_ne!(
            system_of("https://192.168.2.1/"),
            system_of("https://192.168.2.1:8443/")
        );
        assert_ne!(
            system_of("https://host/app1/login"),
            system_of("https://host/app2/login")
        );

        // The same account and password at three addresses are three logins; the same
        // address twice is one.
        let csv = "name,url,username,password\nr,https://192.168.2.1/userLogin.asp,admin,pw\nn,http://192.168.2.1:1188/,admin,pw\nr,https://192.168.2.1/,admin,pw\nr,https://192.168.2.1,admin,pw\n";
        let (inputs, same) = new_logins(&Body::default(), logins(csv).unwrap().0);
        assert_eq!((inputs.len(), same), (3, 1));
    }
}
