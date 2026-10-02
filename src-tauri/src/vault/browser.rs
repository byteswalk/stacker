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
fn identity(host: &str, user: &str, password: &str) -> String {
    super::crypto::secret_digest(&format!("{}\n{}\n{}", host.to_lowercase(), user, password))
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
            let host = host_of(field(&entry.fields, URL_FIELD));
            let host = if host.is_empty() {
                entry.platform.to_lowercase()
            } else {
                host
            };
            identity(
                &host,
                field(&entry.fields, USER_FIELD),
                field(&entry.fields, PASSWORD_FIELD),
            )
        })
        .collect();
    let mut same = 0;
    let mut inputs = Vec::new();
    for login in logins {
        if !known.insert(identity(&login.host, &login.user, &login.password)) {
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

        let twice = format!("{CHROME}github.com,https://github.com/,me,\"p,1\"\"x\",\nother.com,https://other.com/,me,pw3,\n");
        let (inputs, same) = new_logins(&body, logins(&twice).unwrap().0);
        assert_eq!(same, 3);
        assert_eq!(
            inputs.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(),
            vec!["other.com"]
        );
    }
}
