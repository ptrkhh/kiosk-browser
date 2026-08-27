//! WebKit content-rule compilation for the Linux egress belt.

use serde_json::json;
use url::Url;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterOutput {
    pub json: String,
    pub refused: Vec<String>,
}

pub fn compile_filter(allow: &super::allowlist::Allowlist) -> FilterOutput {
    let mut refused = allow.invalid_patterns().to_vec();
    let mut sources = Vec::new();

    for pattern in allow.accepted_patterns() {
        match origin_source_from_pattern(pattern) {
            Some(source) => sources.push(source),
            None => refused.push(pattern.clone()),
        }
    }
    if let Some(home) = allow.home_url() {
        if let Some(source) = origin_source_from_url(home) {
            sources.push(source);
        }
    }

    sources.sort();
    sources.dedup();

    // WebKit's content-rule regex dialect does not support alternation.
    let mut rules = vec![
        json!({
            "trigger": { "url-filter": "^https?://" },
            "action": { "type": "block" }
        }),
        json!({
            "trigger": { "url-filter": "^wss?://" },
            "action": { "type": "block" }
        }),
    ];
    for source in sources {
        rules.push(json!({
            "trigger": { "url-filter": source },
            "action": { "type": "ignore-previous-rules" }
        }));
    }

    FilterOutput {
        json: serde_json::to_string(&rules).expect("content filter rules are serializable"),
        refused,
    }
}

/// Convert an accepted URLPattern spelling to a WebKit URL-filter regex. The filter is
/// intentionally origin-only: URLPattern path/query semantics remain the navigation
/// authority, while SEC-10 closes host-level subresource egress.
pub(crate) fn origin_source_from_pattern(pattern: &str) -> Option<String> {
    let (scheme, authority) = split_authority(pattern)?;
    let (host, port) = split_host_port(authority)?;
    host_source(scheme, host, port)
}

fn origin_source_from_url(url: &Url) -> Option<String> {
    if !is_network_scheme(url.scheme()) {
        return None;
    }
    let host = url.host_str()?;
    if host.contains(':') {
        let port_regex = match url.port() {
            Some(port) => format!(":{port}"),
            None => format!("(:{})?", default_port(url.scheme())?),
        };
        return Some(format!(
            "^{}://\\[{}\\]{}",
            url.scheme(),
            escape_literal(host),
            port_regex
        ));
    }
    host_source(url.scheme(), host, url.port())
}

pub(crate) fn origin_from_pattern(pattern: &str) -> Option<String> {
    let (scheme, authority) = split_authority(pattern)?;
    let (host, port) = split_host_port(authority)?;
    let wildcard = host.starts_with("*.");
    let host = host.strip_prefix("*.").unwrap_or(host);
    if host.is_empty() || host.contains('*') || !valid_host(host) {
        return None;
    }
    let authority = format!("{host}{}", port_suffix(port));
    let parsed = Url::parse(&format!("{scheme}://{authority}/")).ok()?;
    let canonical_host = parsed.host_str()?;
    Some(format!(
        "{}://{}{}",
        parsed.scheme(),
        if wildcard { "*." } else { "" },
        canonical_host_with_port(canonical_host, parsed.port())
    ))
}

pub(crate) fn origin_from_url(url: &Url) -> Option<String> {
    if !is_network_scheme(url.scheme()) {
        return None;
    }
    let host = url.host_str()?;
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    Some(format!(
        "{}://{}{}",
        url.scheme(),
        host,
        url.port().map(|p| format!(":{p}")).unwrap_or_default()
    ))
}

fn split_authority(pattern: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = pattern.split_once("://")?;
    if !is_network_scheme(scheme) || scheme.contains('*') {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    (!authority.is_empty()).then_some((scheme, authority))
}

fn split_host_port(authority: &str) -> Option<(&str, Option<u16>)> {
    if authority.contains('@') || authority.starts_with('[') || authority.matches(':').count() > 1 {
        return None;
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() || port.is_empty() {
            return None;
        }
        return Some((host, Some(port.parse().ok()?)));
    }
    Some((authority, None))
}

fn host_source(scheme: &str, host: &str, port: Option<u16>) -> Option<String> {
    let wildcard = host.strip_prefix("*.");
    let literal_host = wildcard.unwrap_or(host);
    if literal_host.is_empty()
        || literal_host.contains('*')
        || !valid_host(literal_host)
        || (wildcard.is_some() && literal_host.starts_with('.'))
    {
        return None;
    }

    let host_regex = if wildcard.is_some() {
        format!(
            r"[a-z0-9-]+(\.[a-z0-9-]+)*\.{}",
            escape_literal(&literal_host.to_ascii_lowercase())
        )
    } else {
        escape_literal(&literal_host.to_ascii_lowercase())
    };
    let port_regex = match port {
        Some(port) => format!(":{port}"),
        None => format!("(:{})?", default_port(scheme)?),
    };
    Some(format!(
        "^{}://{}{}",
        scheme.to_ascii_lowercase(),
        host_regex,
        port_regex
    ))
}

fn valid_host(host: &str) -> bool {
    host.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

fn escape_literal(value: &str) -> String {
    value
        .chars()
        .flat_map(|c| {
            if matches!(
                c,
                '.' | '+' | '?' | '*' | '[' | ']' | '(' | ')' | '\\' | '^' | '$' | '|' | '{' | '}'
            ) {
                vec!['\\', c]
            } else {
                vec![c]
            }
        })
        .collect()
}

fn port_suffix(port: Option<u16>) -> String {
    port.map(|p| format!(":{p}")).unwrap_or_default()
}

fn canonical_host_with_port(host: &str, port: Option<u16>) -> String {
    match port {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

fn is_network_scheme(scheme: &str) -> bool {
    matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "ws" | "wss"
    )
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav::allowlist::Allowlist;

    fn compile(patterns: &[&str], home: &str) -> FilterOutput {
        let owned: Vec<String> = patterns.iter().map(|s| s.to_string()).collect();
        compile_filter(&Allowlist::new(&owned, home))
    }

    #[test]
    fn the_block_rules_are_narrowed_to_network_schemes() {
        let out = compile(&["https://app.example.com/*"], "https://app.example.com/");
        assert!(out.json.contains(r#""url-filter":"^https?://"#));
        assert!(out.json.contains(r#""url-filter":"^wss?://"#));
        assert!(!out.json.contains(r#""url-filter":".*"#));
    }

    #[test]
    fn a_literal_host_emits_an_exact_default_port() {
        let out = compile(&["https://app.example.com/*"], "https://app.example.com/");
        assert!(out.json.contains(r"app\\.example\\.com(:443)?"));
        assert!(!out.json.contains("[0-9]+"));
    }

    #[test]
    fn an_explicit_port_is_emitted_exactly() {
        let out = compile(
            &["https://app.example.com:8443/*"],
            "https://app.example.com:8443/",
        );
        assert!(out.json.contains(":8443"));
    }

    #[test]
    fn a_leading_label_wildcard_expands_to_a_label_class() {
        let out = compile(&["https://*.example.com/*"], "https://app.example.com/");
        assert!(out
            .json
            .contains(r"[a-z0-9-]+(\\.[a-z0-9-]+)*\\.example\\.com"));
    }

    #[test]
    fn inexpressible_shapes_are_refused() {
        for p in [
            "https://api-*.example.com/*",
            "*://example.com/*",
            "https://:sub.example.com/*",
        ] {
            let out = compile(&[p], "https://example.com/");
            assert!(out.refused.contains(&p.to_string()), "{p} should compile");
        }
    }

    #[test]
    fn the_home_origin_is_emitted_even_with_a_populated_allowlist() {
        let out = compile(&["https://cdn.example.com/*"], "https://home.test/app");
        assert!(out.json.contains(r"home\\.test"));
    }

    #[test]
    fn an_empty_allowlist_emits_only_the_home_origin() {
        let out = compile(&[], "https://home.test/app");
        assert!(out.json.contains(r"home\\.test"));
        assert!(!out.json.contains("example"));
    }

    #[test]
    fn all_four_schemes_are_accepted() {
        for p in [
            "http://a.test/*",
            "https://a.test/*",
            "ws://a.test/*",
            "wss://a.test/*",
        ] {
            let out = compile(&[p], "https://a.test/");
            assert!(out.refused.is_empty(), "{p} should compile");
        }
    }
}
