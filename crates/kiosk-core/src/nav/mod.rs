//! Navigation guard (spec §3.6).
//!
//! The allowlist matcher is a **pure** function of `(patterns, home URL, candidate URL)`:
//! it parses the candidate with `url::Url` and then matches on the parsed **components**,
//! never on the raw string. That purity is the whole point of putting it here rather than
//! in the webview layer — it is what lets the adversarial battery of spec §10 (RT-03) run
//! as ordinary unit tests. `webview.rs` only wires the per-OS navigation-intercept to
//! [`Allowlist::allows`].
//!
//! Every classic bypass in that battery works by making a URL *look* like it contains the
//! allowlisted host while parsing to a different one — `https://app.example.com@evil.com/`
//! has host `evil.com`. So there is exactly one rule here: **parse, then match on
//! components, and default-deny when parsing fails.** There is deliberately no string
//! fallback anywhere in this module.
//!
//! # This is NOT an exfiltration boundary (spec SEC-10)
//!
//! The navigation allowlist governs **top-level (main-frame) navigations**. It does not
//! stop an already-loaded page from *sending* data off-device: script on an allowlisted
//! origin can still `fetch()`, set an `img.src`, or smuggle bytes out through CSS, none of
//! which is a navigation. Egress containment is a **separate** boundary — the subresource
//! host-allowlist plus an injected CSP (spec §7, P1-D). Do not mistake one for the other,
//! and do not "harden" this module in the belief that it closes an exfiltration hole; by
//! construction it cannot.
//!
//! Layering (spec §4): no Tauri, no per-OS API.

pub mod allowlist;
pub mod scheme;

pub use allowlist::Allowlist;

use url::Url;

/// Why a navigation was refused.
///
/// Structured rather than a free-form string so that P1-D can put a stable, greppable
/// reason on the `nav.blocked` telemetry event (spec §6) instead of a prose message that
/// drifts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    /// Parsed fine, but it is not the home URL and matched no allowlist pattern (or, when
    /// the allowlist is empty, it is off the origin lock). Emitted by [`Allowlist::allows`].
    NotAllowlisted,
    /// `url::Url` could not parse it. Default-deny — we never fall back to string matching
    /// on a URL we failed to understand. Emitted by [`Allowlist::allows`].
    Unparseable,
    /// An external URI scheme launch (`mailto:`, `tel:`, `ms-settings:`, an OS-registered
    /// custom scheme…) that is not in `content.scheme_allowlist` (spec §3.6 H2).
    ///
    /// These are not ordinary navigations and are not stopped by cancelling the
    /// navigation-intercept, so they are gated by a separate per-OS hook. Produced by the
    /// platform layer in P1-D, not by this matcher.
    SchemeNotAllowed,
    /// A `kiosk://` navigation initiated by a **remote** origin (spec §3.6). Deciding this
    /// needs the *initiator*, which a `(url) -> Decision` matcher does not have, so it too
    /// is produced by the platform layer in P1-D. Bundled app-origin pages (PIN pad, safe
    /// mode) legitimately use `kiosk://` and must not be caught by it.
    KioskSchemeFromRemote,
}

impl BlockReason {
    /// Stable wire form for the `nav.blocked` telemetry label (spec §6).
    pub fn as_str(self) -> &'static str {
        match self {
            BlockReason::NotAllowlisted => "not_allowlisted",
            BlockReason::Unparseable => "unparseable",
            BlockReason::SchemeNotAllowed => "scheme_not_allowed",
            BlockReason::KioskSchemeFromRemote => "kiosk_scheme_from_remote",
        }
    }
}

/// The verdict for one navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Block(BlockReason),
}

impl Decision {
    pub fn is_allowed(self) -> bool {
        matches!(self, Decision::Allow)
    }

    /// The block reason, or `None` when allowed.
    pub fn block_reason(self) -> Option<BlockReason> {
        match self {
            Decision::Allow => None,
            Decision::Block(r) => Some(r),
        }
    }
}

/// The single navigation-decision entry point (spec §3.6). **P1-D must call this — and only
/// this — never [`Allowlist::allows`] or [`scheme::scheme_decision`] directly**, so the
/// routing below cannot be composed in the wrong order.
///
/// # The seam this closes
///
/// [`Allowlist::allows`] has no concept of `kiosk://` or `is_remote_origin`: it returns
/// [`Decision::Allow`] for a `kiosk://` URL the instant any operator URLPattern matches it (a
/// literal `kiosk://*` does; so does the wildcard-protocol `*://*/*` footgun). The guard that
/// blocks `kiosk://`-from-remote lives in [`scheme::scheme_decision`], *before* its own
/// scheme-allowlist. So the invariant "page script on a remote origin can never reach
/// `kiosk://`" (P1-A deleted the navigation-sentinel bridge for exactly this reason) holds
/// only if these two functions are wired in the right order. A plausible-but-wrong
/// composition — ask the allowlist first, fall back to the scheme guard on a Block — reopens
/// the hole the moment an operator pattern matches a `kiosk://` URL. `decide` makes that
/// composition unrepresentable: there is one entry point and it routes on the scheme.
///
/// # Routing contract
///
/// 1. **Unparseable → [`BlockReason::Unparseable`].** Same default-deny discipline as both
///    delegates; there is no string-matching fallback.
/// 2. **`http`/`https` (ASCII-case-insensitive) → [`Allowlist::allows`].** The allowlist is
///    the sole authority for ordinary navigations, and http(s) is the *only* scheme it ever
///    sees through `decide`. The home-URL implicit-allow and the empty-list origin lock live
///    in the allowlist, so they apply to exactly these navigations (the home URL is the
///    remote content URL and is itself http(s)).
/// 3. **Every other scheme, `kiosk` included → [`scheme::scheme_decision`].** A `kiosk://` URL
///    therefore NEVER reaches [`Allowlist::allows`]: no operator pattern can Allow it, so
///    `kiosk://`-from-remote is structurally unreachable by the allowlist rather than merely
///    unreached by convention. Every exotic scheme (`file:`, `data:`, `mailto:`, an
///    OS-registered custom scheme…) is default-denied here unless the operator listed it in
///    `scheme_allowlist`.
///
/// # Composition safety
///
/// Routing http(s) to the allowlist loses nothing the scheme guard would have caught:
/// [`scheme::scheme_decision`] returns [`Decision::Allow`] for *every* http(s) URL (its rule
/// 2), explicitly deferring to the allowlist. The two orderings therefore agree on http(s),
/// and `decide` picks the one that additionally denies the allowlist any say over exotic
/// schemes. The only behavioural consequence is intentional hardening: a `*://*/*` allowlist
/// no longer lets a `file:`/`data:`/`kiosk:` URL through as a navigation — those now require
/// an explicit `scheme_allowlist` entry (and `kiosk` can never be granted from a remote
/// origin at all).
pub fn decide(
    url: &str,
    allowlist: &Allowlist,
    scheme_allowlist: &[String],
    is_remote_origin: bool,
) -> Decision {
    // Parse once, here, purely to route on the scheme. Both delegates re-parse and would
    // themselves return Block(Unparseable) on failure; doing it here keeps the routing
    // decision explicit and keeps each delegate an independently-correct pure function.
    let Ok(parsed) = Url::parse(url) else {
        return Decision::Block(BlockReason::Unparseable);
    };

    let scheme = parsed.scheme();
    if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
        // Ordinary navigation: the allowlist is the authority (home implicit-allow, origin
        // lock, and operator patterns all live there).
        allowlist.allows(url)
    } else {
        // Every non-http(s) scheme — kiosk:// included — goes to the scheme guard. This is
        // what makes kiosk://-from-remote unreachable by the allowlist.
        scheme::scheme_decision(url, scheme_allowlist, is_remote_origin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_reasons_have_stable_telemetry_names() {
        // P1-D puts these on `nav.blocked`; renaming one silently breaks fleet dashboards.
        assert_eq!(BlockReason::NotAllowlisted.as_str(), "not_allowlisted");
        assert_eq!(BlockReason::Unparseable.as_str(), "unparseable");
        assert_eq!(BlockReason::SchemeNotAllowed.as_str(), "scheme_not_allowed");
        assert_eq!(
            BlockReason::KioskSchemeFromRemote.as_str(),
            "kiosk_scheme_from_remote"
        );
    }

    #[test]
    fn decision_accessors_agree() {
        assert!(Decision::Allow.is_allowed());
        assert_eq!(Decision::Allow.block_reason(), None);
        let d = Decision::Block(BlockReason::Unparseable);
        assert!(!d.is_allowed());
        assert_eq!(d.block_reason(), Some(BlockReason::Unparseable));
    }

    // =================================================================================
    // decide(): the single routing entry point (spec §3.6).
    //
    // These pin the composition CONTRACT so P1-D cannot get the ordering wrong. The
    // load-bearing one is `decide_blocks_kiosk_from_remote_even_when_the_allowlist_would_
    // allow_it`: it first asserts the *premise* (the allowlist, asked directly, WOULD
    // Allow the kiosk:// URL) so the test is only meaningful — and it fails loudly against
    // any `decide` that routes kiosk:// to the allowlist first.
    // =================================================================================

    const HOME: &str = "https://app.example.com/kiosk";
    const PAT: &str = "https://app.example.com/*";

    fn allowlist(patterns: &[&str], home: &str) -> Allowlist {
        let owned: Vec<String> = patterns.iter().map(|s| (*s).to_string()).collect();
        Allowlist::new(&owned, home)
    }

    #[test]
    fn decide_blocks_kiosk_from_remote_even_when_the_allowlist_would_allow_it() {
        // THE composition test. Build an allowlist whose pattern DOES match kiosk://, so a
        // `decide` that consulted the allowlist first would wrongly Allow it — reopening the
        // hole P1-A closed. `decide` routes every non-http(s) scheme to the scheme guard, so
        // the allowlist never sees kiosk:// at all.
        let bypass = allowlist(&["kiosk://*"], HOME);

        // Premise: asked directly, the allowlist WOULD Allow this kiosk:// URL.
        assert_eq!(
            bypass.allows("kiosk://safe-mode"),
            Decision::Allow,
            "premise: the operator pattern matches kiosk:// -- this is the hole decide() closes"
        );

        // But decide() blocks it, with the remote-origin reason.
        assert_eq!(
            decide("kiosk://safe-mode", &bypass, &[], true),
            Decision::Block(BlockReason::KioskSchemeFromRemote),
        );
        // …and still blocked even if the operator ALSO put "kiosk" in scheme_allowlist:
        // rule 3 in the scheme guard runs before its allowlist membership check.
        assert_eq!(
            decide("kiosk://safe-mode", &bypass, &["kiosk".to_string()], true),
            Decision::Block(BlockReason::KioskSchemeFromRemote),
        );
    }

    #[test]
    fn decide_sends_ordinary_https_navigations_to_the_allowlist() {
        let a = allowlist(&[PAT], HOME);
        // Allowlisted → Allow.
        assert_eq!(
            decide("https://app.example.com/page", &a, &[], true),
            Decision::Allow
        );
        // Not allowlisted → Block(NotAllowlisted), the allowlist's own reason.
        assert_eq!(
            decide("https://evil.com/", &a, &[], true),
            Decision::Block(BlockReason::NotAllowlisted)
        );
        // http routes the same way (case-insensitive scheme match), and stays blocked here
        // because the pattern pins https.
        assert_eq!(
            decide("HTTP://app.example.com/page", &a, &[], true),
            Decision::Block(BlockReason::NotAllowlisted)
        );
    }

    #[test]
    fn decide_sends_external_schemes_to_the_scheme_guard() {
        let a = allowlist(&[PAT], HOME);
        // Empty scheme_allowlist → default-deny.
        assert_eq!(
            decide("mailto:x@y", &a, &[], false),
            Decision::Block(BlockReason::SchemeNotAllowed)
        );
        // An operator-allowlisted external scheme is Allowed through decide().
        assert_eq!(
            decide("tel:+18005550100", &a, &["tel".to_string()], false),
            Decision::Allow
        );
    }

    #[test]
    fn decide_preserves_the_home_implicit_allow() {
        // The home URL is https, so it flows through the allowlist, where the implicit
        // home-allow (cfg-02) lives — Allowed even though the patterns would block it.
        let a = allowlist(&["https://somewhere-else.example/*"], HOME);
        // Premise: the patterns would block this origin outright.
        assert_eq!(
            a.allows("https://app.example.com/other"),
            Decision::Block(BlockReason::NotAllowlisted),
            "premise: a mis-typed allowlist blocks the home origin's other paths"
        );
        assert_eq!(decide(HOME, &a, &[], true), Decision::Allow);
    }

    #[test]
    fn decide_blocks_unparseable_urls() {
        let a = allowlist(&[PAT], HOME);
        for bad in [":::", "", "http://[::1"] {
            assert_eq!(
                decide(bad, &a, &[], true),
                Decision::Block(BlockReason::Unparseable),
                "input {bad:?}"
            );
        }
    }
}
