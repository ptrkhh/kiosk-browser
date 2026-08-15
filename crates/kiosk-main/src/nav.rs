//! Navigation-outcome detection (P1-D2a Task 6, spec §Architecture actor-spine).
//!
//! Tauri's own `on_page_load` hook (`tauri::App::on_page_load`) only reports
//! [`tauri_runtime::webview::PageLoadEvent`], and that enum has exactly two variants —
//! `Started` and `Finished` — with **no failure variant at all** (confirmed against
//! tauri 2.11.5's `tauri-runtime` source). It cannot drive `AppEvent::NavigationFailed`.
//!
//! WebView2's native `NavigationCompleted` event *can*: its
//! `ICoreWebView2NavigationCompletedEventArgs::IsSuccess` reports exactly what
//! `on_page_load` cannot. But `NavigationCompleted`'s args carry only a `NavigationId`,
//! never the URL — and the FSM must hear outcomes ONLY for genuine remote/content
//! navigations, never for the bundled app-origin pages `TauriSink` itself navigates to
//! (splash/offline/error.html at `http://tauri.localhost`, the mp4 at
//! `http://kioskasset.localhost`). Feeding the FSM the error page's own successful load
//! would wedge the error-page retry sub-machine (C1): kiosk-core `app/state.rs:259`
//! reads `(ErrorPage, NavigationCommitted)` as "the retry committed" and drops to
//! `Online` with no re-navigation, so the countdown retry never runs.
//!
//! So we also subscribe to `NavigationStarting` (whose args DO carry `Uri` + the same
//! `NavigationId`) and correlate: a small `navId -> uri` map, populated on start,
//! consumed on completion, gives the completed handler the URL to classify with
//! [`feeds_fsm`]. This is outcome DETECTION only — no `NavigationStarting` cancellation
//! or allowlist (that stays D2b) — so the plan's "no nav intercept in D2a" holds.
//! Reached the same way the (now-removed) P0 spike reached `AcceleratorKeyPressed`:
//! `WebviewWindow::with_webview` → the WebView2 controller → `webview2-com`'s bindings.

use kiosk_core::app::state::Event as AppEvent;
use kiosk_core::nav::BlockReason;
use tokio::sync::mpsc;

use crate::nav_policy::{NavPolicy, SharedNavPolicy};
use crate::telemetry::Telemetry;

/// Pure, host-testable (compiled on all targets): does a navigation to `url` feed the
/// FSM? `false` for the app origins that serve bundled pages / the offline mp4
/// (`http://tauri.localhost`, `http://kioskasset.localhost`) — those are internal, not
/// content — and for anything unparseable/host-less; `true` for genuine remote content.
///
/// Delegates to `nav_policy::is_remote_origin` — the single source of truth shared with
/// the nav guard (P1-D2b), so the FSM-feed filter and the guard can never disagree on
/// what counts as "remote".
fn feeds_fsm(url: &str) -> bool {
    crate::nav_policy::is_remote_origin(url)
}

/// The `is_main_frame` argument Linux's builder line (`main.rs`) always passes to
/// [`should_block`] — WebKitGTK's `decide-policy`/`NavigationAction` gives no main-frame/
/// sub-frame distinction at the level wry exposes (`Fn(&Url) -> bool`, no frame info), so
/// Linux makes a deliberate choice instead of inheriting Windows' distinction: enforce the
/// guard on EVERY frame. Naming it here, once, means `main.rs`'s call site and this module's
/// own regression test both read the same value rather than each hardcoding `true`
/// independently — so a future edit that changes the decision necessarily changes both at
/// once, and the test cannot silently stay green through a flip.
pub(crate) const ENFORCE_ALL_FRAMES: bool = true;

/// The pure classification behind the P1-D2b navigation guard (spec §3.6): should this
/// navigation be cancelled? `None` (allow) when `is_main_frame` is `false` — sub-resource
/// (iframe/subresource) navigations are Task 4's separate egress boundary, never this
/// guard's job — or when the URL is an app-origin bundled page (`feeds_fsm`'s own
/// `is_remote_origin` gate: `Allowlist::allows` has no special case for
/// `tauri.localhost`/`kioskasset.localhost`, so `TauriSink`'s own navigation to
/// splash/offline/error/the mp4 would otherwise be judged against the operator's
/// remote-content allowlist and could self-block) — or when
/// [`NavPolicy::decision_for`] allows it; `Some(reason)` otherwise. Never reimplements
/// the matcher: every verdict is `decide`'s, reached only through `decision_for`.
///
/// That `is_main_frame == false` carve-out is a Windows-only escape in practice: Linux's
/// caller (`main.rs`'s builder line) always passes [`ENFORCE_ALL_FRAMES`] (`true`), so on
/// Linux every frame — sub-frames included — IS this guard's job, because `egress.rs` has
/// no Linux body yet (P2-A scope) to catch them instead.
pub(crate) fn should_block(
    policy: &NavPolicy,
    url: &str,
    is_main_frame: bool,
) -> Option<BlockReason> {
    if !is_main_frame || !crate::nav_policy::is_remote_origin(url) {
        return None;
    }
    policy.decision_for(url).block_reason()
}

/// Installs the outcome handlers on `window`'s live `ICoreWebView2`, forwarding every
/// resolved REMOTE navigation's outcome through `tx` and (on failure only) a `nav.error`
/// telemetry event through `telem`. App-origin bundled/asset navigations are filtered
/// out. Also enforces `nav_policy` (P1-D2b Task 2): a main-frame navigation `decide`s
/// against is cancelled before it ever starts, and reported as `nav.blocked`. Call once,
/// right after the webview is built.
/// `ready` is pulsed on the FIRST successful `NavigationCompleted` of ANY
/// origin — including a bundled app-origin page such as the offline error page —
/// not just remote/content navigations (arch-03: webview initialized + first
/// nav committed; the watchdog only needs to know the app is alive and
/// rendering, not that a remote site was reachable). `NavigationCompleted`
/// fires on every navigation, so the pulse is latched to the first success
/// only. This is the heartbeat client's cue to send `Frame::Ready` to the
/// launcher.
#[cfg(windows)]
pub fn install(
    window: &tauri::WebviewWindow,
    tx: mpsc::Sender<AppEvent>,
    telem: Telemetry,
    nav_policy: SharedNavPolicy,
    ready: std::sync::Arc<tokio::sync::Notify>,
) {
    windows_impl::install(window, tx, telem, nav_policy, ready);
}

#[cfg(not(windows))]
pub fn install(
    window: &tauri::WebviewWindow,
    tx: mpsc::Sender<AppEvent>,
    telem: Telemetry,
    nav_policy: SharedNavPolicy,
    ready: std::sync::Arc<tokio::sync::Notify>,
) {
    linux_impl::install(window, tx, telem, nav_policy, ready);
}

#[cfg(windows)]
mod windows_impl {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    use kiosk_core::app::state::Event as AppEvent;
    use tokio::sync::mpsc;

    use crate::nav_policy::SharedNavPolicy;
    use crate::telemetry::Telemetry;

    pub fn install(
        window: &tauri::WebviewWindow,
        tx: mpsc::Sender<AppEvent>,
        telem: Telemetry,
        nav_policy: SharedNavPolicy,
        ready: std::sync::Arc<tokio::sync::Notify>,
    ) {
        let result = window.with_webview(move |platform_webview| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2NavigationCompletedEventArgs, ICoreWebView2NavigationStartingEventArgs,
                ICoreWebView2NewWindowRequestedEventArgs, COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN,
            };
            use webview2_com::{
                NavigationCompletedEventHandler, NavigationStartingEventHandler,
                NewWindowRequestedEventHandler,
            };

            let controller = platform_webview.controller();
            let webview2 = match controller.CoreWebView2() {
                Ok(w) => w,
                Err(e) => {
                    eprintln!(
                        "nav: CoreWebView2() unavailable, navigation outcome will never be observed: {e}"
                    );
                    return;
                }
            };

            // navId -> uri, populated by NavigationStarting and consumed by
            // NavigationCompleted. Both handlers run on WebView2's single UI thread, so
            // `Rc<RefCell<..>>` is sufficient (no cross-thread sharing). Created here,
            // inside `with_webview`'s closure body — never captured by the outer `F`, so
            // `F`'s `Send` bound is unaffected.
            // ponytail: an entry leaks only if a NavigationStarting never gets a matching
            // NavigationCompleted; navigations always resolve, so the map stays bounded.
            let nav_urls: Rc<RefCell<HashMap<u64, String>>> = Rc::new(RefCell::new(HashMap::new()));

            let nav_urls_start = nav_urls.clone();
            let policy_start = nav_policy.clone();
            let telem_start = telem.clone();
            let start_handler = NavigationStartingEventHandler::create(Box::new(
                move |_sender, args: Option<ICoreWebView2NavigationStartingEventArgs>| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let mut nav_id: u64 = 0;
                    args.NavigationId(&mut nav_id)?;
                    let mut uri_pw = windows::core::PWSTR::null();
                    args.Uri(&mut uri_pw)?;
                    let uri = webview2_com::take_pwstr(uri_pw);

                    // `NavigationStarting` fires ONLY for the top-level/main-frame
                    // navigation (confirmed against webview2-com-sys 0.38.2's
                    // `ICoreWebView2NavigationStartingEventArgs` bindings: no `IsMainFrame`
                    // field exists on it — sub-frame navigations are a wholly separate
                    // event, `ICoreWebView2Frame::add_NavigationStarting`/
                    // `FrameNavigationStartingEventHandler`, which this module never
                    // subscribes to). So every navigation reaching this handler already
                    // satisfies the guard's main-frame scope; `is_main_frame` is always
                    // `true` here.
                    if let Some(reason) = super::should_block(&policy_start.load(), &uri, true) {
                        args.SetCancel(true)?;
                        telem_start.nav_blocked(reason.as_str(), &uri);
                        // No navId->uri insert: a cancelled navigation gets no matching
                        // NavigationCompleted, so nothing would ever consume this entry.
                        return Ok(());
                    }

                    nav_urls_start.borrow_mut().insert(nav_id, uri);
                    Ok(())
                },
            ));
            let mut start_token: i64 = 0;
            if let Err(e) = webview2.add_NavigationStarting(&start_handler, &mut start_token) {
                eprintln!("nav: add_NavigationStarting failed, URLs cannot be correlated: {e}");
            }

            // §3.6: a window.open()/target=_blank request must not spawn a second
            // WebView2 window (the kiosk owns exactly one). Take over the requested
            // navigation into the CURRENT webview instead — it then re-enters
            // `start_handler` above and is judged by the same guard.
            let webview2_new_window = webview2.clone();
            let new_window_handler = NewWindowRequestedEventHandler::create(Box::new(
                move |_sender, args: Option<ICoreWebView2NewWindowRequestedEventArgs>| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    args.SetHandled(true)?;
                    let mut uri_pw = windows::core::PWSTR::null();
                    args.Uri(&mut uri_pw)?;
                    let uri = webview2_com::take_pwstr(uri_pw);
                    webview2_new_window.Navigate(&windows::core::HSTRING::from(uri))?;
                    Ok(())
                },
            ));
            let mut new_window_token: i64 = 0;
            if let Err(e) =
                webview2.add_NewWindowRequested(&new_window_handler, &mut new_window_token)
            {
                eprintln!("nav: add_NewWindowRequested failed, popups may open a second window: {e}");
            }

            let ready_latch = std::sync::atomic::AtomicBool::new(false);
            let completed_handler = NavigationCompletedEventHandler::create(Box::new(
                move |_sender, args: Option<ICoreWebView2NavigationCompletedEventArgs>| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let mut nav_id: u64 = 0;
                    args.NavigationId(&mut nav_id)?;
                    let uri = nav_urls.borrow_mut().remove(&nav_id);

                    let mut is_success = windows::core::BOOL(0);
                    args.IsSuccess(&mut is_success)?;

                    // Readiness pulse fires on the FIRST successful commit of ANY
                    // origin — including the bundled app-origin offline page — not
                    // just remote/content navigations. The watchdog is asking "is
                    // the app alive and rendering", not "is the site reachable": a
                    // device that boots offline still renders the offline page
                    // successfully and must arm the launcher, or `startup_grace_s`
                    // expires and the launcher restart-loops a working kiosk into
                    // safe mode on `cause: "no_ready"`. This must run BEFORE the
                    // `feeds_fsm` filter below, which exists only to scope the
                    // navigation FSM and must not gate readiness.
                    if is_success.as_bool()
                        && ready_latch
                            .compare_exchange(false, true, std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst)
                            .is_ok()
                    {
                        ready.notify_one();
                    }

                    // C1: only genuine remote/content navigations reach the FSM. An
                    // app-origin bundled page (the error page's own commit!) or an
                    // uncorrelated navId (e.g. the very first boot splash, whose
                    // NavigationStarting fired before this handler was live) is
                    // suppressed — the FSM must never see it.
                    if !matches!(&uri, Some(u) if super::feeds_fsm(u)) {
                        return Ok(());
                    }

                    let event = if is_success.as_bool() {
                        AppEvent::NavigationCommitted
                    } else {
                        let mut status = COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN;
                        args.WebErrorStatus(&mut status)?;
                        // The numeric code maps to the `COREWEBVIEW2_WEB_ERROR_STATUS_*`
                        // constants in webview2-com-sys::bindings; good enough for an
                        // operator grepping Cloud Logging, not worth a friendly-name table.
                        telem.nav_error(&format!("{status:?}"));
                        AppEvent::NavigationFailed
                    };
                    // `try_send`, never a blocking/async send: this closure runs on
                    // WebView2's own COM callback thread, not a tokio worker — the
                    // driver's queue backing up must never stall the webview.
                    let _ = tx.try_send(event);
                    Ok(())
                },
            ));
            // webview2-com-sys 0.38.2: the token out-param is a raw `*mut i64` (mirrors
            // the P0 spike's `add_AcceleratorKeyPressed` — no `EventRegistrationToken`
            // type is generated for this Win32-interop signature).
            let mut completed_token: i64 = 0;
            if let Err(e) = webview2.add_NavigationCompleted(&completed_handler, &mut completed_token) {
                eprintln!("nav: add_NavigationCompleted failed: {e}");
            }
        });
        if let Err(e) = result {
            eprintln!("nav: with_webview failed, navigation outcome will never be observed: {e}");
        }
    }
}

/// Load-lifecycle detection (spec "`nav.rs` — load lifecycle"): outcome-only, mirrors
/// `windows_impl`'s `NavigationStarting`/`NavigationCompleted` mapping onto the same
/// `AppEvent`/`Telemetry`/ready-pulse surface. Distinct from the nav GUARD (Task 5's
/// `on_navigation` builder line in `main.rs`) — this module calls [`super::feeds_fsm`],
/// never [`super::should_block`]; the guard already ran before WebKit ever started this
/// load.
///
/// **Assumption, not derivable from the pinned bindings:** `load-changed`/`load-failed`/
/// `load-failed-with-tls-errors` are `WebKitWebView`-level signals that track the **main
/// frame's** load only, so a sub-frame's (iframe's) load never fires them —
/// `webkit2gtk-2.0.2`'s bindings (`web_view.rs:2287,2316,2355`) give signatures only, no
/// doc text confirming frame scope. The failure latch and the policy-cancellation filter
/// below both depend on this holding. Pinned observationally by smoke scenario 5 (an
/// off-allowlist iframe must produce no `NavigationFailed`, no `nav.error` and no
/// error-page transition), not asserted here.
#[cfg(not(windows))]
mod linux_impl {
    use std::cell::Cell;
    use std::rc::Rc;

    use kiosk_core::app::state::Event as AppEvent;
    use tokio::sync::mpsc;
    use webkit2gtk::{LoadEvent, PolicyError, WebViewExt};

    use crate::nav_policy::SharedNavPolicy;
    use crate::telemetry::Telemetry;

    /// A `load-failed` raised by our own guard's cancellation. Dropped statelessly: the
    /// guard already emitted the single `nav.blocked`, matching Windows'
    /// one-event-per-blocked-navigation.
    ///
    /// **Invariant:** this filter assumes exactly one `navigation_handler` is installed
    /// and that no RESPONSE or download decision is subscribed. P2-B adds both, and they
    /// raise the same error code from a different cause — P2-B must re-derive this.
    fn is_policy_cancellation(err: &webkit2gtk::glib::Error) -> bool {
        matches!(
            err.kind::<PolicyError>(),
            Some(PolicyError::FrameLoadInterruptedByPolicyChange)
        )
    }

    pub fn install(
        window: &tauri::WebviewWindow,
        tx: mpsc::Sender<AppEvent>,
        telem: Telemetry,
        _nav_policy: SharedNavPolicy,
        ready: std::sync::Arc<tokio::sync::Notify>,
    ) {
        let result = window.with_webview(move |platform_webview| {
            let webview = platform_webview.inner();

            // One latch, GTK-main-thread-only, so `Rc<Cell<..>>` is sufficient.
            let failed = Rc::new(Cell::new(false));
            let ready_latch = Rc::new(Cell::new(false));

            let failed_changed = failed.clone();
            let tx_changed = tx.clone();
            webview.connect_load_changed(move |wv, event| {
                match event {
                    // The only per-load boundary WebKit gives us. Clearing here rather
                    // than on the suppressed FINISHED is load-bearing: a load-failed with
                    // no following FINISHED must not arm the latch across navigations,
                    // which would swallow the next successful load's commit and park the
                    // kiosk on the offline video with a healthy network.
                    LoadEvent::Started => failed_changed.set(false),
                    LoadEvent::Finished => {
                        if failed_changed.get() {
                            return;
                        }
                        // READY pulses on the first successful load of ANY origin,
                        // including the bundled offline page: the watchdog asks "is the
                        // app alive and rendering", not "is the site reachable". Must run
                        // BEFORE the feeds_fsm filter.
                        if !ready_latch.replace(true) {
                            ready.notify_one();
                        }
                        // `uri()` is read at signal time and is post-redirect (the Windows
                        // navId→uri map is pre-redirect); both classify identically for
                        // `is_remote_origin`, since a redirect cannot cross into our own
                        // registered schemes. `None` classifies as not-remote — never
                        // unwrap in a signal handler.
                        let Some(uri) = wv.uri() else { return };
                        if !super::feeds_fsm(uri.as_str()) {
                            return;
                        }
                        let _ = tx_changed.try_send(AppEvent::NavigationCommitted);
                    }
                    _ => {}
                }
            });

            let failed_load = failed.clone();
            let tx_load = tx.clone();
            let telem_load = telem.clone();
            webview.connect_load_failed(move |_wv, _event, failing_uri, err| {
                if is_policy_cancellation(err) {
                    // No AppEvent, no telemetry, no latch change.
                    return true;
                }
                failed_load.set(true);
                // Both the FSM event and the telemetry sit inside the remote-origin
                // filter, mirroring Windows where `nav_error` sits after the `feeds_fsm`
                // early return. App-origin load failures stay silent.
                if super::feeds_fsm(failing_uri) {
                    telem_load.nav_error(&err.to_string());
                    let _ = tx_load.try_send(AppEvent::NavigationFailed);
                }
                true
            });

            let failed_tls = failed;
            let tx_tls = tx;
            let telem_tls = telem;
            webview.connect_load_failed_with_tls_errors(move |_wv, failing_uri, _cert, _errors| {
                failed_tls.set(true);
                if super::feeds_fsm(failing_uri) {
                    telem_tls.nav_error("tls_error");
                    let _ = tx_tls.try_send(AppEvent::NavigationFailed);
                }
                true
            });
        });
        if let Err(e) = result {
            eprintln!("nav: with_webview failed, navigation outcome will never be observed: {e}");
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn policy_cancellation_error_is_recognized() {
            let err = webkit2gtk::glib::Error::new(
                PolicyError::FrameLoadInterruptedByPolicyChange,
                "frame load interrupted by policy change",
            );
            assert!(is_policy_cancellation(&err));
        }

        /// The exact confusable regression Rule 3 exists to prevent: a same-shaped
        /// "cancelled" error from a DIFFERENT domain (a real network-layer cancellation,
        /// not our own guard's policy decision) must NOT be recognized. If the domain
        /// scoping in `is_policy_cancellation` is ever widened or dropped — e.g. "fixed"
        /// to also catch `NetworkError::Cancelled` — this goes red while the positive
        /// case above stays green, catching exactly that mistake.
        #[test]
        fn a_same_shaped_error_from_a_different_domain_is_not_recognized() {
            let err =
                webkit2gtk::glib::Error::new(webkit2gtk::NetworkError::Cancelled, "cancelled");
            assert!(!is_policy_cancellation(&err));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{feeds_fsm, should_block, ENFORCE_ALL_FRAMES};
    use crate::nav_policy::NavPolicy;
    use kiosk_core::config::schema::Content;
    use kiosk_core::nav::BlockReason;

    fn policy(allow: &[&str], home: &str) -> NavPolicy {
        NavPolicy::from_config(
            &Content {
                url: Some(home.to_string()),
                allowlist: allow.iter().map(|s| s.to_string()).collect(),
                ..Content::default()
            },
            home,
        )
    }

    #[test]
    fn main_frame_off_allowlist_is_blocked() {
        let p = policy(&["https://home.test/*"], "https://home.test/app");
        assert_eq!(
            should_block(&p, "https://evil.test/x", true),
            Some(BlockReason::NotAllowlisted)
        );
    }

    #[test]
    fn sub_frame_off_allowlist_is_not_this_guards_concern() {
        let p = policy(&["https://home.test/*"], "https://home.test/app");
        assert_eq!(should_block(&p, "https://evil.test/x", false), None);
    }

    /// Linux enforces the guard on ALL frames — the deliberate divergence from Windows,
    /// where sub-frames are waved past because `egress.rs` catches them. Asserts against
    /// [`ENFORCE_ALL_FRAMES`] itself, the same constant `main.rs`'s builder line passes as
    /// `should_block`'s third argument — not a locally-hardcoded `true` — so flipping that
    /// constant flips this test's expected outcome too (`Some(NotAllowlisted)` → `None`)
    /// instead of leaving it silently green.
    #[test]
    fn the_guard_blocks_an_off_allowlist_sub_frame_when_told_it_is_in_scope() {
        let p = policy(&["https://home.test/*"], "https://home.test/app");
        assert_eq!(
            should_block(&p, "https://evil.test/frame", ENFORCE_ALL_FRAMES),
            Some(BlockReason::NotAllowlisted)
        );
    }

    #[test]
    fn main_frame_home_is_allowed() {
        let p = policy(&["https://home.test/*"], "https://home.test/app");
        assert_eq!(should_block(&p, "https://home.test/app", true), None);
    }

    /// Bundled app-origin pages (splash/offline/error/pdf at `tauri.localhost`/
    /// `kioskasset.localhost`) must never be blocked by this guard, even against an
    /// allowlist that names neither host: `Allowlist::allows` has no special case for
    /// them (they are `TauriSink`'s own navigations, never operator-configured remote
    /// content), so `should_block` gates on `is_remote_origin` BEFORE calling
    /// `decision_for` — exactly like `feeds_fsm` does for the completed-handler side.
    #[test]
    fn bundled_app_origin_pages_are_never_blocked() {
        let p = policy(&["https://home.test/*"], "https://home.test/app");
        assert_eq!(
            should_block(&p, "http://tauri.localhost/splash.html", true),
            None
        );
        assert_eq!(
            should_block(&p, "http://kioskasset.localhost/kiosk-offline.mp4", true),
            None
        );
    }

    #[test]
    fn app_origin_bundled_pages_do_not_feed_the_fsm() {
        assert!(!feeds_fsm("http://tauri.localhost/error.html"));
        assert!(!feeds_fsm("http://tauri.localhost/offline.html"));
        assert!(!feeds_fsm("http://tauri.localhost/splash.html"));
        assert!(!feeds_fsm("http://tauri.localhost")); // bare origin, no path
    }

    #[test]
    fn the_kioskasset_mp4_does_not_feed_the_fsm() {
        assert!(!feeds_fsm("http://kioskasset.localhost/kiosk-offline.mp4"));
    }

    #[test]
    fn genuine_remote_content_feeds_the_fsm() {
        assert!(feeds_fsm("https://real.site/"));
        assert!(feeds_fsm("https://real.site/page"));
    }

    #[test]
    fn unparseable_or_hostless_does_not_feed_the_fsm() {
        assert!(!feeds_fsm("not a url"));
        assert!(!feeds_fsm("about:blank"));
    }

    // A remote host merely PREFIXED with an app-origin label must still feed the FSM —
    // origin is matched by host, not by a string prefix that `tauri.localhost.evil.com`
    // would spoof.
    #[test]
    fn a_spoofed_prefix_host_still_feeds_the_fsm() {
        assert!(feeds_fsm("https://tauri.localhost.evil.com/"));
    }
}
