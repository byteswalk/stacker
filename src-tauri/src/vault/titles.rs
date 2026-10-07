//! A web page's own title, read when the user asks for it so a saved login says what the
//! site is. Only the page is fetched: no cookies, nothing from the vault goes along. Public
//! sites go through Stacker's proxy; addresses on this network go direct, as no proxy can
//! reach them.

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Enough of a page for its `<head>`.
const READ_AT_MOST: u64 = 512 * 1024;
const WORKERS: usize = 10;
/// A page that has not answered by now is not going to: many saved logins point at sites
/// long gone, and each one waited for is a wait for the user.
const CONNECT_WAIT: Duration = Duration::from_secs(3);
const PAGE_WAIT: Duration = Duration::from_secs(6);
/// Looking up a name is not covered by the waits above, and a dead domain can take Windows
/// well over ten seconds to give up on.
const LOOKUP_WAIT: Duration = Duration::from_secs(3);
const LONGEST: usize = 120;
/// Some sites turn away anything that does not look like a browser.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

/// Whether a host is on this computer or this network, where a proxy cannot go.
fn is_local(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    if host == "localhost" || !host.contains('.') && !host.contains(':') {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00,
        Err(_) => host.ends_with(".local") || host.ends_with(".lan"),
    }
}

fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if authority.starts_with('[') {
        return authority
            .split_once(']')
            .map_or(authority, |(host, _)| host);
    }
    authority.split(':').next().unwrap_or_default()
}

/// The system's name lookup, given up on after `LOOKUP_WAIT`; the lookup thread finishes on
/// its own and its answer is dropped.
fn lookup(netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
    use std::net::ToSocketAddrs;
    let (sender, receiver) = std::sync::mpsc::channel();
    let netloc = netloc.to_string();
    std::thread::spawn(move || {
        let _ = sender.send(
            netloc
                .to_socket_addrs()
                .map(|addrs| addrs.collect::<Vec<_>>()),
        );
    });
    receiver.recv_timeout(LOOKUP_WAIT).unwrap_or_else(|_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "name lookup took too long",
        ))
    })
}

fn agent(local: bool) -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .resolver(lookup)
        .timeout_connect(CONNECT_WAIT)
        .timeout(PAGE_WAIT)
        .redirects(5);
    if !local {
        if let Some(proxy) = crate::agents::net::stacker_proxy() {
            if let Ok(proxy) = ureq::Proxy::new(&proxy) {
                builder = builder.proxy(proxy);
            }
        }
    }
    builder.build()
}

/// The page's title, or `None` when it cannot be read or has none.
pub(crate) fn fetch(url: &str) -> Option<String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let response = agent(is_local(host_of(url)))
        .get(url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "text/html,application/xhtml+xml")
        .call()
        .ok()?;
    let content_type = response
        .header("content-type")
        .unwrap_or_default()
        .to_string();
    if !content_type.is_empty() && !content_type.contains("html") {
        return None;
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(READ_AT_MOST)
        .read_to_end(&mut bytes)
        .ok()?;
    title_of(&bytes, &content_type)
}

/// Each URL's title, read several at a time; `progress` hears `(done, total)` after each.
pub(crate) fn fetch_all(
    urls: Vec<String>,
    progress: impl Fn(usize, usize) + Send + Sync,
) -> HashMap<String, String> {
    let mut unique = urls;
    unique.sort();
    unique.dedup();
    let total = unique.len();
    let queue = Mutex::new(unique.into_iter());
    let found = Mutex::new(HashMap::new());
    let done = AtomicUsize::new(0);
    let progress = Arc::new(progress);
    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(total) {
            let (queue, found, done, progress) = (&queue, &found, &done, progress.clone());
            scope.spawn(move || loop {
                let Some(url) = queue.lock().unwrap_or_else(|e| e.into_inner()).next() else {
                    break;
                };
                if let Some(title) = fetch(&url) {
                    found
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(url, title);
                }
                progress(done.fetch_add(1, Ordering::SeqCst) + 1, total);
            });
        }
    });
    found.into_inner().unwrap_or_else(|e| e.into_inner())
}

/// The charset the header names, or else the page's own `<meta>`; UTF-8 when neither does.
fn encoding_of(bytes: &[u8], content_type: &str) -> &'static encoding_rs::Encoding {
    let named = |text: &str| {
        let lower = text.to_ascii_lowercase();
        let at = lower.find("charset=")? + "charset=".len();
        let name: String = lower[at..]
            .trim_start_matches(['"', '\''])
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        encoding_rs::Encoding::for_label(name.as_bytes())
    };
    named(content_type)
        .or_else(|| named(&String::from_utf8_lossy(&bytes[..bytes.len().min(4096)])))
        .unwrap_or(encoding_rs::UTF_8)
}

fn entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub(crate) fn title_of(bytes: &[u8], content_type: &str) -> Option<String> {
    let (text, _, _) = encoding_of(bytes, content_type).decode(bytes);
    let lower = text.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let start = open + lower[open..].find('>')? + 1;
    let end = start + lower[start..].find("</title")?;
    let title = entities(&text[start..end])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() {
        return None;
    }
    Some(if title.chars().count() > LONGEST {
        title.chars().take(LONGEST - 1).collect::<String>() + "…"
    } else {
        title
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_is_read_in_the_page_own_encoding() {
        let page = b"<html><head><meta charset=\"utf-8\"><TITLE>\n  GitHub &amp; Co &#x4E2D;&#25991;  \n</TITLE></head>";
        assert_eq!(
            title_of(page, "text/html").as_deref(),
            Some("GitHub & Co 中文")
        );
        let (gbk, _, _) = encoding_rs::GBK.encode("<title>路由器管理</title>");
        let mut page =
            b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=gb2312\">".to_vec();
        page.extend_from_slice(&gbk);
        assert_eq!(title_of(&page, "text/html").as_deref(), Some("路由器管理"));
        assert_eq!(
            title_of(&gbk, "text/html; charset=GBK").as_deref(),
            Some("路由器管理")
        );
        assert_eq!(title_of(b"<title>  </title>", ""), None);
        assert_eq!(title_of(b"<p>no title</p>", ""), None);
        let long = format!("<title>{}</title>", "a".repeat(300));
        assert_eq!(
            title_of(long.as_bytes(), "").unwrap().chars().count(),
            LONGEST
        );
        assert_eq!(entities("a & b &bogus; &lt;"), "a & b &bogus; <");
    }

    #[test]
    fn addresses_on_this_network_go_direct() {
        assert!(is_local(host_of("http://192.168.2.1:8080/login")));
        assert!(is_local(host_of("https://admin:x@10.0.0.1/")));
        assert!(is_local(host_of("http://localhost/")));
        assert!(is_local(host_of("http://nas/")));
        assert!(is_local(host_of("http://[::1]:3000/")));
        assert!(is_local(host_of("http://printer.local/")));
        assert!(!is_local(host_of("https://github.com/login")));
        assert!(!is_local(host_of("http://216.152.152.238:25330/")));
    }

    #[test]
    #[ignore = "reads real pages over the network"]
    fn real_pages_have_titles() {
        let found = fetch_all(
            vec![
                "https://github.com/login".into(),
                "https://www.baidu.com/".into(),
            ],
            |_, _| {},
        );
        println!("{found:?}");
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn a_name_that_does_not_resolve_is_given_up_on_quickly() {
        let started = std::time::Instant::now();
        assert_eq!(fetch("http://no-such-host.invalid/"), None);
        assert!(
            started.elapsed() < LOOKUP_WAIT + PAGE_WAIT,
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn nothing_but_web_pages_is_fetched() {
        assert_eq!(fetch("ftp://example.com/"), None);
        assert_eq!(fetch("android://abc@com.example/"), None);
        assert!(fetch_all(Vec::new(), |_, _| {}).is_empty());
    }
}
