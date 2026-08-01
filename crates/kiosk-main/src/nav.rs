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
fn should_block(policy: &NavPolicy, url: &str, is_main_frame: bool) -> Option<BlockReason> {
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
    _window: &tauri::WebviewWindow,
    _tx: mpsc::Sender<AppEvent>,
    _telem: Telemetry,
    _nav_policy: SharedNavPolicy,
    _ready: std::sync::Arc<tokio::sync::Notify>,
) {
    eprintln!("nav: only implemented on Windows; NavigationCommitted/Failed will never fire");
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

#[cfg(test)]
mod tests {
    use super::{feeds_fsm, should_block};
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
