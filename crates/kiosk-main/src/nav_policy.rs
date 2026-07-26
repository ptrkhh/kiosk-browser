//! Live-swappable navigation policy (spec §3.6, P1-D2b Task 1).
//!
//! `NavPolicy` is a thin, read-only view over the config-derived navigation inputs
//! (allowlist + scheme allowlist). It never reimplements the decision logic — every
//! verdict is delegated to `kiosk_core::nav::decide`, which is itself adversarially
//! host-tested in `kiosk-core`. This type exists only so the live config (which
//! changes underneath a running kiosk on every successful fetch) can be read
//! lock-free by WebView2's navigation callbacks (`ArcSwap::load`) while being
//! swapped out by the config-apply path (`ArcSwap::store`) without blocking either
//! side.

use std::sync::Arc;

use arc_swap::ArcSwap;
use kiosk_core::config::schema::Content;
use kiosk_core::nav::allowlist::Allowlist;
use kiosk_core::nav::{decide, Decision};

/// Shared handle to the live policy: cloned into every reader (the nav guard) and
/// the single writer (the config-apply path). `ArcSwap` gives readers a lock-free
/// `load`, and the writer an atomic `store` — no navigation is ever judged against a
/// half-updated policy.
pub type SharedNavPolicy = Arc<ArcSwap<NavPolicy>>;

/// The navigation inputs derived from the currently-applied config. Rebuilt (never
/// mutated) on every config apply, including the very first one at boot.
pub struct NavPolicy {
    allowlist: Allowlist,
    scheme_allowlist: Vec<String>,
}

impl NavPolicy {
    /// `active_url` is the home URL already expanded through
    /// `identity::expand_device_id_template` (i.e. `ConfigManager::home_url()`'s
    /// return value) — never `content.url` directly, which may still carry the
    /// `{device_id}` template.
    pub fn from_config(content: &Content, active_url: &str) -> NavPolicy {
        NavPolicy {
            // `Allowlist::new` itself implements cfg-02 (implicit home allow) and
            // arch-08 (empty-list origin lock) — nothing here reimplements either.
            allowlist: Allowlist::new(&content.allowlist, active_url),
            scheme_allowlist: content.scheme_allowlist.clone(),
        }
    }

    /// The single per-navigation verdict, routed through `kiosk_core::nav::decide` —
    /// never `Allowlist::allows` directly (that would bypass the `kiosk://`-from-remote
    /// guard; see `decide`'s own docs).
    pub fn decision_for(&self, url: &str) -> Decision {
        decide(
            url,
            &self.allowlist,
            &self.scheme_allowlist,
            is_remote_origin(url),
        )
    }
}

/// App-origin (bundled pages / mp4) vs remote content. Single source of truth;
/// `nav::feeds_fsm` delegates here so the FSM-feed filter and the nav guard agree by
/// construction.
pub fn is_remote_origin(url: &str) -> bool {
    match tauri::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
    {
        Some(host) => host != "tauri.localhost" && host != "kioskasset.localhost",
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::config::schema::Content;

    fn content(allow: &[&str], schemes: &[&str]) -> Content {
        Content {
            url: Some("https://home.test/app".into()),
            allowlist: allow.iter().map(|s| s.to_string()).collect(),
            scheme_allowlist: schemes.iter().map(|s| s.to_string()).collect(),
            ..Content::default()
        }
    }

    #[test]
    fn home_is_implicitly_allowed_even_if_not_in_allowlist() {
        let p = NavPolicy::from_config(
            &content(&["https://other.test/*"], &[]),
            "https://home.test/app",
        );
        assert!(
            p.decision_for("https://home.test/app").is_allowed(),
            "home must never self-block (cfg-02)"
        );
    }

    #[test]
    fn off_allowlist_remote_url_is_blocked() {
        let p = NavPolicy::from_config(
            &content(&["https://home.test/*"], &[]),
            "https://home.test/app",
        );
        assert!(!p.decision_for("https://evil.test/x").is_allowed());
    }

    #[test]
    fn empty_allowlist_locks_to_active_origin() {
        // arch-08 bootstrap window
        let p = NavPolicy::from_config(&content(&[], &[]), "https://home.test/app");
        assert!(
            p.decision_for("https://home.test/anything").is_allowed(),
            "same origin allowed"
        );
        assert!(
            !p.decision_for("https://home.test.evil.com/").is_allowed(),
            "different host blocked"
        );
    }

    #[test]
    fn app_origin_pages_are_not_remote() {
        assert!(!is_remote_origin("http://tauri.localhost/error.html"));
        assert!(!is_remote_origin(
            "http://kioskasset.localhost/kiosk-offline.mp4"
        ));
        assert!(is_remote_origin("https://home.test/app"));
        // Brief's literal text asserted `!is_remote_origin(...)` here, flagged "NOTE:
        // verify". Verified wrong: a host merely PREFIXED with the app-origin label is
        // NOT the app origin — it must be treated as remote, exactly like nav.rs's own
        // `a_spoofed_prefix_host_still_feeds_the_fsm` (which asserts `feeds_fsm(..)` is
        // `true` for this same URL). Flipping this assertion would make `is_remote_origin`
        // disagree with `feeds_fsm` on the one case that matters (a spoofed prefix host),
        // reopening exactly the bypass `feeds_fsm`'s own doc comment calls out.
        assert!(
            is_remote_origin("http://tauri.localhost.evil.com/"),
            "host-match not prefix"
        );
    }
}
