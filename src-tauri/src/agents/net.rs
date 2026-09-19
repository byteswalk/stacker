//! Proxy used by Stacker's own agent downloads and the installer processes it starts.
//! It never reads or writes the user's environment variables or tool configs.

/// `manual` uses Stacker's configured address, `system` reads the current Windows proxy at
/// call time, and any other mode injects nothing so child processes inherit the system.
pub(crate) fn select_proxy(
    mode: &str,
    manual: (&str, u16),
    system: Option<(String, u16)>,
) -> Option<String> {
    match mode {
        "manual" if !manual.0.trim().is_empty() && manual.1 > 0 => {
            Some(format!("http://{}:{}", manual.0.trim(), manual.1))
        }
        // Stacker's own requests read the Windows proxy in hands-off and follow-system mode.
        "system" | "hands_off" => system.map(|(host, port)| format!("http://{host}:{port}")),
        _ => None,
    }
}

pub(crate) fn stacker_proxy() -> Option<String> {
    let (host, port) = crate::settings::proxy_addr();
    select_proxy(
        &crate::settings::proxy_mode(),
        (&host, port),
        crate::settings::detected_proxy_addr(),
    )
}

pub(crate) fn proxy_env(proxy: &str) -> [(&'static str, String); 4] {
    [
        ("HTTP_PROXY", proxy.to_string()),
        ("HTTPS_PROXY", proxy.to_string()),
        ("npm_config_proxy", proxy.to_string()),
        ("npm_config_https_proxy", proxy.to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_mode_never_injects() {
        assert_eq!(
            select_proxy("off", ("127.0.0.1", 7890), Some(("10.0.0.1".into(), 8080))),
            None
        );
    }

    #[test]
    fn manual_mode_uses_the_stacker_address() {
        assert_eq!(
            select_proxy("manual", ("127.0.0.1", 7890), None).as_deref(),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(select_proxy("manual", ("", 0), None), None);
    }

    #[test]
    fn system_mode_uses_the_current_windows_proxy_only() {
        assert_eq!(
            select_proxy("system", ("", 0), Some(("10.0.0.1".into(), 8080))).as_deref(),
            Some("http://10.0.0.1:8080")
        );
        assert_eq!(select_proxy("system", ("127.0.0.1", 7890), None), None);
    }

    #[test]
    fn env_covers_npm_and_generic_clients() {
        let keys: Vec<_> = proxy_env("http://h:1")
            .iter()
            .map(|(key, _)| *key)
            .collect();
        assert_eq!(
            keys,
            [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "npm_config_proxy",
                "npm_config_https_proxy"
            ]
        );
    }
}
