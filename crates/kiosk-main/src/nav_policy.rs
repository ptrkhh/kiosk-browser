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
    // ponytail: see `pdf_view()`'s doc comment — read by no COM callsite yet.
    #[allow(dead_code)]
    pdf_view: bool,
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
            pdf_view: content.pdf_view,
        }
    }

    /// The operator-configured external-scheme allowlist (P1-D2b Task 3, spec §3.6
    /// H2) — read by the `LaunchingExternalUriScheme` guard, which is a separate
    /// WebView2 event from `NavigationStarting` and so needs its own read access
    /// (`decision_for` already covers it for main-frame navigations, but external
    /// schemes never reach `NavigationStarting`).
    pub fn scheme_allowlist(&self) -> &[String] {
        &self.scheme_allowlist
    }

    /// `content.pdf_view` (spec M4): `false` ⇒ a main-frame `application/pdf`
    /// response is blocked; `true` ⇒ allowed (the bundled pdf.js viewer route is a
    /// later phase — this only means "don't block here").
    ///
    /// ponytail: not called from any COM callsite yet — P1-D2b Task 3 found no
    /// cancel-capable pre-render content-type signal in webview2-com-sys 0.38.2 to
    /// hang PDF enforcement on (see `scheme_guard`'s module doc). Kept (not deleted)
    /// so the live-config plumbing is ready the moment such a signal exists.
    #[allow(dead_code)]
    pub fn pdf_view(&self) -> bool {
        self.pdf_view
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

    /// May a **subresource** (image/CSS/script/fetch/websocket/…) load `url` (P1-D2b
    /// Task 4, spec SEC-10)? This is the exfiltration boundary `decision_for` is NOT —
    /// see `kiosk_core::nav`'s own module doc ("This is NOT an exfiltration boundary").
    ///
    /// Two rules:
    ///
    /// 1. **Inline / app-origin is always allowed.** `is_remote_origin` returns `false`
    ///    for both a `tauri.localhost`/`kioskasset.localhost` host AND for any hostless
    ///    URL (`data:`, `blob:`, `about:`…). For a main-frame *navigation* that hostless
    ///    case is irrelevant (`decide` default-denies unparseable/non-http schemes
    ///    outright); here it is the point — an inline `data:` image or `blob:` object URL
    ///    never leaves the process, so blocking it would only break legitimate bundled
    ///    assets for zero egress benefit. This is the deliberate OPPOSITE of the
    ///    main-frame case, not an oversight.
    /// 2. **A remote resource must match the allowlist**, checked directly against
    ///    [`Allowlist::allows`] — **not** `decide`/`decision_for`. Judgment call: `decide`
    ///    routes every non-http(s) scheme (a `wss://` subresource included) through
    ///    `scheme::scheme_decision` against `scheme_allowlist`, which is the operator's
    ///    *external protocol launch* list (`mailto`, `tel`, …), a different concept from
    ///    "which remote hosts may this page talk to". Routing subresources through it
    ///    would require operators to double-list every websocket host in
    ///    `scheme_allowlist` just to keep `decide`'s composition, or silently block a
    ///    legitimate `wss://` to an already-allowlisted host. Matching the allowlist
    ///    patterns directly, scheme included, keeps one authority ("is this host/scheme
    ///    on the list") for both `https://` fetches and `wss://` sockets: an operator who
    ///    wants websocket egress to a host simply lists a `wss://host/*` pattern, exactly
    ///    like any other scheme-specific allowlist entry.
    pub fn resource_allowed(&self, url: &str) -> bool {
        if !is_remote_origin(url) {
            return true;
        }
        self.allowlist.allows(url).is_allowed()
    }
}

/// A restrictive Content-Security-Policy value for the injected document-start bundle
/// (P1-D2b Task 4, spec SEC-10 / §7) — belt-and-suspenders alongside the native
/// `WebResourceRequested` filter (`crate::egress`), which is the primary enforcement
/// point. `content_origin` is the active remote content's origin (scheme+host+port,
/// e.g. `https://home.test`); bundled assets always load from `http://tauri.localhost`
/// so that origin is always admitted too.
///
/// Pure and host-tested here; **not injected by this function**. `initialization_script`
/// may only be called once per webview (a second caller clobbers the first), and P1-D2b
/// Task 6 owns the single document-start bundle — that task must consume
/// `csp_policy(active_origin)` inside its `build_injection`, not call
/// `initialization_script` a second time.
///
/// # Residual gaps (document per spec)
///
/// A CSP is a renderer-enforced, same-document policy. It does not close everything
/// `WebResourceRequested` does:
/// - **Service workers** registered before this CSP was in force (or from a
///   previously-cached response) keep their own fetch/cache policy; a CSP header on a
///   later navigation does not retroactively constrain an already-installed worker.
/// - **Preload/prefetch paths** (`<link rel=preload>`/browser speculative loads) are
///   sometimes issued before the CSP meta tag is parsed, depending on injection timing.
/// - It says nothing about **main-frame navigation** — that boundary is `nav.rs`/
///   `NavPolicy::decision_for`, wholly separate.
///
/// The native `WebResourceRequested` filter in `crate::egress` is not subject to any of
/// these gaps (it inspects every request before the renderer ever sees a response,
/// regardless of service workers or preload timing), which is why it is the primary
/// control and this CSP is the secondary one.
///
/// ponytail: not called from any COM callsite yet — T6 (the document-start injection
/// bundle) is the intended, not-yet-written caller. Host-tested below regardless, same
/// convention as `scheme_guard::pdf_decision`.
#[allow(dead_code)]
pub fn csp_policy(content_origin: &str) -> String {
    format!(
        "default-src {content_origin} http://tauri.localhost; \
         img-src {content_origin} http://tauri.localhost data:; \
         style-src {content_origin} http://tauri.localhost 'unsafe-inline'; \
         font-src {content_origin} http://tauri.localhost data:; \
         connect-src {content_origin} http://tauri.localhost; \
         media-src {content_origin} http://tauri.localhost; \
         object-src 'none'; base-uri 'none'; frame-ancestors 'none'"
    )
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

    // ---- resource_allowed (P1-D2b Task 4, SEC-10) -------------------------------------

    #[test]
    fn off_allowlist_remote_subresource_is_blocked() {
        let p = NavPolicy::from_config(
            &content(&["https://home.test/*"], &[]),
            "https://home.test/app",
        );
        assert!(!p.resource_allowed("https://evil/a"));
    }

    #[test]
    fn in_allowlist_subresource_is_allowed() {
        let p = NavPolicy::from_config(
            &content(&["https://cdn.test/*"], &[]),
            "https://home.test/app",
        );
        assert!(p.resource_allowed("https://cdn.test/assets/logo.png"));
    }

    #[test]
    fn app_origin_subresource_is_always_allowed() {
        let p = NavPolicy::from_config(
            &content(&["https://home.test/*"], &[]),
            "https://home.test/app",
        );
        assert!(p.resource_allowed("http://tauri.localhost/bundle.js"));
    }

    #[test]
    fn inline_data_uri_subresource_is_always_allowed() {
        // Inline data never leaves the process -- blocking it would only break bundled
        // assets for zero egress benefit. Opposite of the main-frame nav case, on purpose.
        let p = NavPolicy::from_config(
            &content(&["https://home.test/*"], &[]),
            "https://home.test/app",
        );
        assert!(p.resource_allowed("data:image/png;base64,AAAA"));
    }

    #[test]
    fn a_remote_websocket_to_an_allowlisted_scheme_specific_pattern_is_allowed() {
        // The judgment call documented on `resource_allowed`: matched directly against the
        // allowlist (scheme included), not routed through `scheme_allowlist`.
        let p = NavPolicy::from_config(
            &content(&["wss://home.test/*"], &[]),
            "https://home.test/app",
        );
        assert!(p.resource_allowed("wss://home.test/socket"));
    }

    // ---- csp_policy (P1-D2b Task 4) ---------------------------------------------------

    #[test]
    fn csp_policy_scopes_default_src_to_the_content_origin_and_tauri_localhost() {
        let csp = csp_policy("https://home.test");
        assert!(csp.contains("default-src"));
        assert!(csp.contains("https://home.test"));
        assert!(csp.contains("http://tauri.localhost"));
    }
}
