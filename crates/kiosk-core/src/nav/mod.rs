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
}
