//! The external-URI-scheme guard and the `kiosk://`-from-remote rule (spec §3.6, H2).
//!
//! Read the module docs in [`super`] first.
//!
//! `mailto:`, `tel:`, `ms-settings:`, `ms-store:`, and any OS-registered custom scheme are
//! **not ordinary navigations**: cancelling a navigation does not stop them, the OS launches
//! an external app instead. On a kiosk that is an escape hatch — `ms-settings:` opens
//! Windows Settings *on top of* the locked kiosk. [`scheme_decision`] is the pure decision
//! function; P1-D wires it to WebView2's `LaunchingExternalUriScheme`, WebKitGTK's
//! `decide-policy`, and Android's `shouldOverrideUrlLoading`.

use url::Url;

use super::{BlockReason, Decision};

/// Decides whether an external URI-scheme launch — or a `kiosk://` navigation — may
/// proceed.
///
/// `is_remote_origin` is supplied by the *caller*: P1-D knows whether the navigation was
/// initiated by the remote content origin or by an app-origin bundled page (PIN pad, safe
/// mode); this function has no other way to learn it. An app-origin page's own use of
/// `kiosk://` is out of scope here — P1-D handles app-origin navigations before ever
/// consulting this guard.
///
/// Rules, applied in this exact order:
///
/// 1. **Default-deny on parse failure** — [`BlockReason::Unparseable`], no string-matching
///    fallback. Same discipline as [`super::allowlist::Allowlist::allows`].
/// 2. `http`/`https` are ordinary navigations: always [`Decision::Allow`] here. The
///    allowlist ([`super::allowlist::Allowlist`], Task 1) decides those; it is not this
///    function's job.
/// 3. **`kiosk://` from a remote origin is always blocked**
///    ([`BlockReason::KioskSchemeFromRemote`]) — checked *before* rule 4, so it can never be
///    bypassed by adding `"kiosk"` to `scheme_allowlist`. P1-A removed the `kiosk://`
///    navigation-sentinel bridge precisely because any script in the page's world could
///    fire it; an operator must not be able to re-open that hole through config.
/// 4. Every other scheme is blocked ([`BlockReason::SchemeNotAllowed`]) unless it appears in
///    `scheme_allowlist`. `scheme_allowlist` defaults to empty, so `mailto:`, `tel:`,
///    `ms-settings:`, `ms-store:`, `intent:`, `file:` are all blocked unless an operator
///    explicitly lists them.
///
/// Scheme comparison is ASCII-case-insensitive on both sides. `Url::scheme()` is already
/// lowercased by the WHATWG URL parser, but `scheme_allowlist` entries are operator-supplied
/// config and are not assumed to already be lowercase, so every comparison here uses
/// [`str::eq_ignore_ascii_case`] rather than relying on that invariant.
pub fn scheme_decision(url: &str, scheme_allowlist: &[String], is_remote_origin: bool) -> Decision {
    // Rule 1. Parse first; every rule below matches on the parsed *scheme*, never on the
    // raw string.
    let Ok(parsed) = Url::parse(url) else {
        return Decision::Block(BlockReason::Unparseable);
    };
    let scheme = parsed.scheme();

    // Rule 2. Ordinary navigations; the allowlist (Task 1) decides these, not this
    // function.
    if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
        return Decision::Allow;
    }

    // Rule 3 — MUST run before rule 4 (the allowlist membership check). A `"kiosk"` entry
    // in `scheme_allowlist` must never be able to reach this and turn into an Allow.
    if is_remote_origin && scheme.eq_ignore_ascii_case("kiosk") {
        return Decision::Block(BlockReason::KioskSchemeFromRemote);
    }

    // Rule 4. Blocked unless explicitly allowlisted, case-insensitively on both sides.
    let allowed = scheme_allowlist
        .iter()
        .any(|entry| entry.eq_ignore_ascii_case(scheme));
    if allowed {
        Decision::Allow
    } else {
        Decision::Block(BlockReason::SchemeNotAllowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every "not in scheme_allowlist" block in this file is this one.
    const NOT_ALLOWED: Decision = Decision::Block(BlockReason::SchemeNotAllowed);
    const KIOSK_FROM_REMOTE: Decision = Decision::Block(BlockReason::KioskSchemeFromRemote);
    const UNPARSEABLE: Decision = Decision::Block(BlockReason::Unparseable);

    fn allowlist(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|s| (*s).to_string()).collect()
    }

    // ---- Rule 1 (parse first): unparseable -> Block(Unparseable), never string-matched --

    #[test]
    fn unparseable_urls_are_blocked_and_never_string_matched() {
        let empty = allowlist(&[]);
        for bad in [":::", "", "   ", "not a url", "https://", "http://[::1"] {
            assert_eq!(
                scheme_decision(bad, &empty, true),
                UNPARSEABLE,
                "input {bad:?}"
            );
            assert_eq!(
                scheme_decision(bad, &empty, false),
                UNPARSEABLE,
                "input {bad:?}"
            );
        }
    }

    // ---- Rule 2: http/https are ordinary navigations, always Allow here -----------------

    #[test]
    fn http_and_https_are_allowed_with_no_allowlist_entries() {
        let empty = allowlist(&[]);
        assert_eq!(
            scheme_decision("http://example.com/", &empty, true),
            Decision::Allow
        );
        assert_eq!(
            scheme_decision("https://example.com/", &empty, true),
            Decision::Allow
        );
        // The origin flag must not matter for http/https — Task 1's allowlist is what
        // actually governs these, not this function.
        assert_eq!(
            scheme_decision("https://example.com/", &empty, false),
            Decision::Allow
        );
    }

    #[test]
    fn http_and_https_are_allowed_case_insensitively() {
        let empty = allowlist(&[]);
        assert_eq!(
            scheme_decision("HTTP://example.com/", &empty, true),
            Decision::Allow
        );
        assert_eq!(
            scheme_decision("HTTPS://example.com/", &empty, true),
            Decision::Allow
        );
        assert_eq!(
            scheme_decision("HtTpS://example.com/", &empty, true),
            Decision::Allow
        );
    }

    // ---- Rule 2/4: every other scheme is blocked by default (empty scheme_allowlist) ----

    #[test]
    fn external_schemes_are_blocked_by_default_when_allowlist_is_empty() {
        let empty = allowlist(&[]);
        for url in [
            "mailto:a@b.com",
            "tel:+18005550100",
            "ms-settings:privacy",
            "ms-store://pdp/?productid=9wzdncrfhvjl",
            "intent://scan/#Intent;scheme=zxing;package=com.example;end",
            "file:///etc/passwd",
        ] {
            assert_eq!(scheme_decision(url, &empty, false), NOT_ALLOWED, "{url:?}");
            // The origin flag must not matter for non-kiosk schemes either.
            assert_eq!(scheme_decision(url, &empty, true), NOT_ALLOWED, "{url:?}");
        }
    }

    #[test]
    fn mailto_is_blocked_regardless_of_scheme_case() {
        // Called out explicitly in the brief: MAILTO: must also block.
        let empty = allowlist(&[]);
        for url in ["MAILTO:a@b.com", "MailTo:a@b.com", "mailto:a@b.com"] {
            assert_eq!(scheme_decision(url, &empty, false), NOT_ALLOWED, "{url:?}");
        }
    }

    #[test]
    fn a_signed_empty_config_leaves_scheme_allowlist_empty_and_blocks_by_default() {
        // The `{}` document: `Content::default()` carries an empty scheme_allowlist.
        let content = crate::config::schema::Content::default();
        assert!(
            content.scheme_allowlist.is_empty(),
            "premise: {{}} has no scheme_allowlist"
        );
        assert_eq!(
            scheme_decision("mailto:a@b.com", &content.scheme_allowlist, false),
            NOT_ALLOWED
        );
        assert_eq!(
            scheme_decision("https://example.com/", &content.scheme_allowlist, false),
            Decision::Allow
        );
    }

    // ---- Rule 4: an explicitly allowlisted scheme is allowed ----------------------------

    #[test]
    fn an_explicitly_allowlisted_scheme_is_allowed() {
        let tel_only = allowlist(&["tel"]);
        assert_eq!(
            scheme_decision("tel:+18005550100", &tel_only, false),
            Decision::Allow
        );
        // A sibling scheme that was NOT listed stays blocked.
        assert_eq!(
            scheme_decision("mailto:a@b.com", &tel_only, false),
            NOT_ALLOWED
        );
    }

    #[test]
    fn scheme_allowlist_comparison_is_case_insensitive_on_both_sides() {
        // Allowlist entry uppercase, URL scheme lowercase (as the parser always produces).
        let upper_entry = allowlist(&["MAILTO"]);
        assert_eq!(
            scheme_decision("mailto:a@b.com", &upper_entry, false),
            Decision::Allow
        );

        // URL scheme uppercase, allowlist entry lowercase.
        let lower_entry = allowlist(&["mailto"]);
        assert_eq!(
            scheme_decision("MAILTO:a@b.com", &lower_entry, false),
            Decision::Allow
        );
    }

    // ---- Rule 3: kiosk:// from a remote origin is ALWAYS blocked ------------------------

    #[test]
    fn kiosk_scheme_from_remote_origin_is_blocked() {
        let empty = allowlist(&[]);
        assert_eq!(
            scheme_decision("kiosk://safe-mode", &empty, true),
            KIOSK_FROM_REMOTE
        );
    }

    #[test]
    fn kiosk_scheme_from_remote_origin_is_blocked_even_when_kiosk_is_allowlisted() {
        // THE load-bearing test. P1-A removed the kiosk:// navigation-sentinel bridge
        // because any page-world script could fire it. An operator adding "kiosk" to
        // scheme_allowlist must NOT be able to re-open that hole. This requires the
        // kiosk-from-remote check (rule 3) to run BEFORE the allowlist membership check
        // (rule 4) — an implementation that checks the allowlist first would wrongly
        // Allow this.
        let kiosk_allowlisted = allowlist(&["kiosk"]);
        assert_eq!(
            scheme_decision("kiosk://safe-mode", &kiosk_allowlisted, true),
            KIOSK_FROM_REMOTE,
        );
    }

    #[test]
    fn kiosk_scheme_from_remote_is_blocked_regardless_of_case() {
        let kiosk_allowlisted = allowlist(&["KIOSK"]);
        assert_eq!(
            scheme_decision("KIOSK://safe-mode", &kiosk_allowlisted, true),
            KIOSK_FROM_REMOTE,
        );
    }

    #[test]
    fn kiosk_scheme_when_not_remote_origin_is_not_flagged_as_kiosk_from_remote() {
        // is_remote_origin genuinely gates rule 3 — this is not "always block kiosk
        // outright regardless of the flag". App-origin use of kiosk:// is out of scope for
        // this function (P1-D handles it before ever calling this guard), so the only
        // property pinned here is that the *specific* KioskSchemeFromRemote reason is not
        // produced when the flag is false.
        let empty = allowlist(&[]);
        let d = scheme_decision("kiosk://safe-mode", &empty, false);
        assert_ne!(d, KIOSK_FROM_REMOTE);
        assert_eq!(d, NOT_ALLOWED);
    }
}
