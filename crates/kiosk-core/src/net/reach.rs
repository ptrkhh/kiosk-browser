//! Reachability scope for the connectivity prober (spec §3.3, arch-13).
//!
//! The prober measures reachability of *`content.url`'s network path*, not "the
//! internet" in general. On an intranet/air-gapped deployment the public default probe
//! (`https://www.gstatic.com/generate_204`) reads **permanently offline** while the
//! content host is perfectly reachable, so the kiosk would show the offline video
//! forever on a site that is up. [`resolve_probe_url`] is the fix: it swaps the probe
//! target to the home origin when (and only when) the operator never chose a probe URL
//! of their own *and* the home is not publicly reachable anyway. [`is_private_host`] is
//! the heuristic that answers "not publicly reachable anyway".
//!
//! Layering (spec §4): no Tauri, no per-OS API. `std::net::{IpAddr, Ipv6Addr}` is pure
//! std and is the only "network" type here — there is no DNS resolution, no socket, no
//! HTTP. Both functions are pure string/parse logic.

use std::net::{IpAddr, Ipv6Addr};

use url::Url;

/// The schema default for `network.connectivity_check_url`
/// ([`crate::config::schema::Network`]). The field is never literally absent — serde
/// fills this in when the operator's config omits it — so "unset" is defined as "still
/// exactly equal to this constant". See [`resolve_probe_url`].
pub const DEFAULT_CONNECTIVITY_CHECK_URL: &str = "https://www.gstatic.com/generate_204";

/// True when `host` can only be reached over a private/internal network path, so probing
/// the public default (`https://www.gstatic.com/generate_204`) would never observe it.
///
/// Rules, in the order applied:
///
/// 1. **`host` parses as an IP literal** — classified by [`is_private_ip`]: RFC 1918
///    IPv4 (`10/8`, `172.16/12`, `192.168/16`), IPv4/IPv6 loopback (`127/8`, `::1`),
///    IPv4 link-local (`169.254/16`), IPv6 link-local (`fe80::/10`), and IPv6 ULA
///    (`fc00::/7`).
/// 2. **The literal `localhost`**, case-insensitively.
/// 3. **A `.local` suffix** (mDNS, RFC 6762), case-insensitively.
/// 4. **A single-label hostname** — no dot at all (e.g. `kiosk-server`). A name with no
///    dot cannot be resolved by public DNS; only an internal resolver (split-horizon
///    DNS, `/etc/hosts`, mDNS with the suffix omitted) can ever answer it.
///
/// Anything else — an ordinary `host.example.com` or a public IP — is **not** private.
///
/// # Bracketed IPv6
///
/// `host` is expected in the form [`url::Url::host_str`] returns: an IPv6 literal comes
/// back **without** brackets (`::1`, not `[::1]`), and that is the form
/// [`resolve_probe_url`] passes. As a defensive fallback for a caller that instead
/// passes the URL-syntax bracketed form (RFC 3986 §3.2.2), a single matching pair of
/// surrounding `[`/`]` is stripped before parsing. A host that is not a bracket-wrapped
/// IP literal is never affected by this — brackets cannot legally appear in a hostname.
pub fn is_private_host(host: &str) -> bool {
    let host = strip_brackets(host);

    // Rule 1.
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_private_ip(ip);
    }

    // Rule 2.
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    // Rule 3. `to_ascii_lowercase` is UTF-8-safe (it only touches ASCII bytes and never
    // changes the byte length), unlike slicing the last 6 *bytes* directly, which would
    // panic on a host with a multi-byte (IDN) label if the cut landed mid-character.
    if host.to_ascii_lowercase().ends_with(".local") {
        return true;
    }

    // Rule 4. No dot anywhere. Checked last, after the more specific named rules above —
    // though `localhost` would also satisfy this (it too has no dot), the explicit rule
    // 2 check makes that case self-documenting rather than an incidental side effect.
    if !host.contains('.') {
        return true;
    }

    false
}

/// Choose the URL the prober should actually GET (spec §3.3, arch-13).
///
/// - If `configured` is still the **schema default** ([`DEFAULT_CONNECTIVITY_CHECK_URL`])
///   **and** `home_url`'s host [`is_private_host`], probe **`home_url`'s origin**
///   (`scheme://host[:port]`, no path/query/fragment) instead — the public default would
///   read permanently offline against a private/intranet host.
/// - Otherwise return `configured` verbatim. **An operator who set an explicit probe URL
///   always wins**, even against a private home — this function never overrides a
///   deliberate operator choice, only an untouched default.
/// - If `home_url` fails to parse, or parses with no host (e.g. a `data:` URL), the
///   private-host question is unanswerable; `configured` is returned unchanged rather
///   than guessing.
pub fn resolve_probe_url(configured: &str, home_url: &str) -> String {
    // Operator's explicit choice always wins — checked first and unconditionally: if
    // `configured` is not byte-identical to the schema default, nothing below this line
    // ever runs.
    if configured != DEFAULT_CONNECTIVITY_CHECK_URL {
        return configured.to_string();
    }

    let Ok(home) = Url::parse(home_url) else {
        return configured.to_string();
    };

    let Some(host) = home.host_str() else {
        return configured.to_string();
    };

    if !is_private_host(host) {
        return configured.to_string();
    }

    // `Url::origin().ascii_serialization()` is the WHATWG origin serialization:
    // `scheme://host[:port]`, port omitted when it is the scheme's default, IPv6 hosts
    // bracketed. Exactly the `scheme://host[:port]` form this function's contract
    // promises, and it is battle-tested URL-syntax handling rather than hand-rolled.
    home.origin().ascii_serialization()
}

/// IP-literal classification backing rule 1 of [`is_private_host`].
///
/// - IPv4: [`std::net::Ipv4Addr::is_loopback`] (`127/8`),
///   [`std::net::Ipv4Addr::is_private`] (RFC 1918: `10/8`, `172.16/12`, `192.168/16` —
///   std's own boundary-correct implementation, not hand-rolled), and
///   [`std::net::Ipv4Addr::is_link_local`] (`169.254/16`).
/// - IPv6: [`std::net::Ipv6Addr::is_loopback`] (`::1`); ULA `fc00::/7` and link-local
///   `fe80::/10` are **not** covered by stable std (`is_unique_local` /
///   `is_unicast_link_local` are gated behind the unstable `ip` feature), so both are
///   checked manually against the top bits of the address's first 16-bit segment.
fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback() || is_ipv6_ula(v6) || is_ipv6_link_local(v6),
    }
}

/// IPv6 Unique Local Address, RFC 4193 `fc00::/7`: the top 7 bits of the address fix the
/// first octet to `0xfc` or `0xfd`. Masking the first 16-bit segment with `0xfe00`
/// isolates exactly those 7 bits — the 8th bit, which distinguishes `fc` from `fd`, is
/// left free by the mask, so both are matched.
fn is_ipv6_ula(v6: Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xfe00) == 0xfc00
}

/// IPv6 link-local unicast, RFC 4291 `fe80::/10`: the top 10 bits of the address are
/// fixed to `1111111010`. Masking the first 16-bit segment with `0xffc0` isolates those
/// 10 bits.
fn is_ipv6_link_local(v6: Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xffc0) == 0xfe80
}

fn strip_brackets(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===================================================================================
    // is_private_host — IPv4 RFC 1918
    // ===================================================================================

    #[test]
    fn rfc1918_10_slash_8_is_private() {
        for addr in ["10.0.0.0", "10.0.0.1", "10.255.255.255", "10.42.1.7"] {
            assert!(is_private_host(addr), "{addr}");
        }
    }

    #[test]
    fn rfc1918_192_168_slash_16_is_private() {
        for addr in ["192.168.0.0", "192.168.0.1", "192.168.255.255"] {
            assert!(is_private_host(addr), "{addr}");
        }
    }

    #[test]
    fn rfc1918_172_16_slash_12_boundary_is_exact() {
        // THE classic off-by-one. 172.16.0.0-172.31.255.255 is private; 172.15.x and
        // 172.32.x are NOT, even though a naive "first octet == 172" check (ignoring the
        // second octet's 16-31 range) would wrongly call them private too. Both edges of
        // both sides are pinned here.
        for (addr, expected_private) in [
            ("172.15.0.0", false),
            ("172.15.255.255", false), // one address below the range
            ("172.16.0.0", true),      // lower bound, inclusive
            ("172.16.0.1", true),
            ("172.31.255.255", true), // upper bound, inclusive
            ("172.32.0.0", false),    // one address above the range
            ("172.32.0.1", false),
        ] {
            assert_eq!(is_private_host(addr), expected_private, "{addr}");
        }
    }

    // ===================================================================================
    // is_private_host — loopback
    // ===================================================================================

    #[test]
    fn ipv4_loopback_slash_8_is_private() {
        for addr in ["127.0.0.1", "127.0.0.0", "127.255.255.255"] {
            assert!(is_private_host(addr), "{addr}");
        }
    }

    #[test]
    fn ipv6_loopback_is_private() {
        assert!(is_private_host("::1"));
    }

    #[test]
    fn the_literal_localhost_is_private_case_insensitively() {
        for host in ["localhost", "LOCALHOST", "LocalHost"] {
            assert!(is_private_host(host), "{host}");
        }
    }

    // ===================================================================================
    // is_private_host — link-local
    // ===================================================================================

    #[test]
    fn ipv4_link_local_slash_16_is_private() {
        for addr in ["169.254.0.0", "169.254.1.1", "169.254.255.255"] {
            assert!(is_private_host(addr), "{addr}");
        }
    }

    #[test]
    fn ipv6_link_local_fe80_slash_10_boundary_is_exact() {
        for (addr, expected_private) in [
            ("fe7f:ffff:ffff:ffff:ffff:ffff:ffff:ffff", false), // one below the range
            ("fe80::", true),                                   // lower bound
            ("fe80::1", true),
            ("febf:ffff:ffff:ffff:ffff:ffff:ffff:ffff", true), // upper bound
            ("fec0::", false), // one above the range (historic "site-local", not link-local)
        ] {
            assert_eq!(is_private_host(addr), expected_private, "{addr}");
        }
    }

    // ===================================================================================
    // is_private_host — IPv6 ULA
    // ===================================================================================

    #[test]
    fn ipv6_ula_fc00_slash_7_boundary_is_exact() {
        // fc00::/7 covers BOTH fc00:: and fd00:: (the brief calls this out explicitly:
        // the 8th bit, distinguishing fc/fd, is inside the /7, not a fixed boundary).
        for (addr, expected_private) in [
            ("fbff:ffff:ffff:ffff:ffff:ffff:ffff:ffff", false), // one below the range
            ("fc00::", true),                                   // lower bound
            ("fc00::1", true),
            ("fd00::1", true), // the fd00:: form specifically
            ("fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff", true), // upper bound
            ("fe00::", false), // one above the range
        ] {
            assert_eq!(is_private_host(addr), expected_private, "{addr}");
        }
    }

    // ===================================================================================
    // is_private_host — .local suffix (mDNS) and single-label hostnames
    // ===================================================================================

    #[test]
    fn dot_local_suffix_is_private_case_insensitively() {
        for host in ["printer.local", "PRINTER.LOCAL", "kiosk-server.local"] {
            assert!(is_private_host(host), "{host}");
        }
    }

    #[test]
    fn single_label_hostname_is_private() {
        // No dot at all: can only ever resolve via an internal/split-horizon resolver.
        for host in ["kiosk-server", "intranet", "monitor01"] {
            assert!(is_private_host(host), "{host}");
        }
    }

    // ===================================================================================
    // is_private_host — bracketed IPv6 (defensive; see module docs on the assumption)
    // ===================================================================================

    #[test]
    fn bracketed_ipv6_is_classified_the_same_as_unbracketed() {
        assert!(is_private_host("[::1]"));
        assert!(is_private_host("[fc00::1]"));
        assert!(is_private_host("[fe80::1]"));
        assert!(!is_private_host("[2001:db8::1]"));
    }

    // ===================================================================================
    // is_private_host — public hosts are NOT private
    // ===================================================================================

    #[test]
    fn ordinary_public_hosts_are_not_private() {
        for host in [
            "app.example.com",
            "93.184.216.34", // documentation-range public IPv4 (RFC 5737 TEST-NET-3-ish; not RFC1918)
            "2001:db8::1",   // IPv6 documentation prefix, RFC 3849 — not ULA, not link-local
            "www.gstatic.com",
        ] {
            assert!(!is_private_host(host), "{host}");
        }
    }

    // ===================================================================================
    // resolve_probe_url
    // ===================================================================================

    #[test]
    fn default_probe_url_with_private_home_switches_to_the_home_origin() {
        // Single-label host, default port -> port omitted from the origin.
        let resolved = resolve_probe_url(
            DEFAULT_CONNECTIVITY_CHECK_URL,
            "http://kiosk-server/app/page",
        );
        assert_eq!(resolved, "http://kiosk-server");
    }

    #[test]
    fn default_probe_url_with_private_home_and_explicit_port_keeps_the_port() {
        let resolved = resolve_probe_url(
            DEFAULT_CONNECTIVITY_CHECK_URL,
            "http://10.0.0.5:8080/dashboard",
        );
        assert_eq!(resolved, "http://10.0.0.5:8080");
    }

    #[test]
    fn default_probe_url_with_private_ipv6_home_brackets_the_origin_host() {
        let resolved = resolve_probe_url(DEFAULT_CONNECTIVITY_CHECK_URL, "http://[fc00::1]:9000/x");
        assert_eq!(resolved, "http://[fc00::1]:9000");
    }

    #[test]
    fn default_probe_url_with_public_home_keeps_gstatic() {
        let resolved = resolve_probe_url(
            DEFAULT_CONNECTIVITY_CHECK_URL,
            "https://app.example.com/kiosk",
        );
        assert_eq!(resolved, DEFAULT_CONNECTIVITY_CHECK_URL);
    }

    #[test]
    fn explicit_probe_url_with_private_home_keeps_the_explicit_url() {
        // THE load-bearing test: an operator who set their own probe URL always wins,
        // even though the home is private and would otherwise trigger the swap. The
        // explicit URL is returned byte-for-byte, including its own path — it is not
        // reduced to an origin the way the home-URL fallback is.
        let explicit = "https://intranet-monitor.local/ping";
        let resolved = resolve_probe_url(explicit, "http://10.0.0.5/app");
        assert_eq!(resolved, explicit);
    }

    #[test]
    fn explicit_probe_url_with_public_home_keeps_the_explicit_url() {
        let explicit = "https://intranet-monitor.local/ping";
        let resolved = resolve_probe_url(explicit, "https://app.example.com/kiosk");
        assert_eq!(resolved, explicit);
    }

    #[test]
    fn a_configured_value_that_only_looks_like_the_default_is_treated_as_explicit() {
        // The subtlety called out in the brief: comparison against the default is EXACT
        // string equality, not a normalised-URL equality. A near-miss spelling of the
        // default (trailing slash) must NOT be treated as "unset" — it is whatever the
        // operator actually wrote, verbatim.
        let near_miss = "https://www.gstatic.com/generate_204/";
        assert_ne!(
            near_miss, DEFAULT_CONNECTIVITY_CHECK_URL,
            "premise: differs by a slash"
        );
        let resolved = resolve_probe_url(near_miss, "http://10.0.0.5/app");
        assert_eq!(resolved, near_miss);
    }

    #[test]
    fn unparseable_home_url_keeps_configured_unchanged() {
        let resolved = resolve_probe_url(DEFAULT_CONNECTIVITY_CHECK_URL, ":::");
        assert_eq!(resolved, DEFAULT_CONNECTIVITY_CHECK_URL);
    }

    #[test]
    fn a_home_url_with_no_host_keeps_configured_unchanged() {
        // Premise: a data: URL parses fine but has no host to classify.
        let home = "data:text/html,hi";
        let parsed = Url::parse(home).expect("data: URLs parse");
        assert_eq!(parsed.host_str(), None, "premise");
        let resolved = resolve_probe_url(DEFAULT_CONNECTIVITY_CHECK_URL, home);
        assert_eq!(resolved, DEFAULT_CONNECTIVITY_CHECK_URL);
    }
}
