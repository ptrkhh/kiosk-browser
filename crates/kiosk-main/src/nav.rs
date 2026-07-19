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
use tokio::sync::mpsc;

use crate::telemetry::Telemetry;

/// Pure, host-testable (compiled on all targets): does a navigation to `url` feed the
/// FSM? `false` for the app origins that serve bundled pages / the offline mp4
/// (`http://tauri.localhost`, `http://kioskasset.localhost`) — those are internal, not
/// content — and for anything unparseable/host-less; `true` for genuine remote content.
fn feeds_fsm(url: &str) -> bool {
    let Ok(parsed) = tauri::Url::parse(url) else {
        return false;
    };
    match parsed.host_str() {
        Some(host) => host != "tauri.localhost" && host != "kioskasset.localhost",
        None => false,
    }
}

/// Installs the outcome handlers on `window`'s live `ICoreWebView2`, forwarding every
/// resolved REMOTE navigation's outcome through `tx` and (on failure only) a `nav.error`
/// telemetry event through `telem`. App-origin bundled/asset navigations are filtered
/// out. Call once, right after the webview is built.
#[cfg(windows)]
pub fn install(window: &tauri::WebviewWindow, tx: mpsc::Sender<AppEvent>, telem: Telemetry) {
    windows_impl::install(window, tx, telem);
}

#[cfg(not(windows))]
pub fn install(_window: &tauri::WebviewWindow, _tx: mpsc::Sender<AppEvent>, _telem: Telemetry) {
    eprintln!("nav: only implemented on Windows; NavigationCommitted/Failed will never fire");
}

#[cfg(windows)]
mod windows_impl {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    use kiosk_core::app::state::Event as AppEvent;
    use tokio::sync::mpsc;

    use crate::telemetry::Telemetry;

    pub fn install(window: &tauri::WebviewWindow, tx: mpsc::Sender<AppEvent>, telem: Telemetry) {
        let result = window.with_webview(move |platform_webview| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2NavigationCompletedEventArgs, ICoreWebView2NavigationStartingEventArgs,
                COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN,
            };
            use webview2_com::{NavigationCompletedEventHandler, NavigationStartingEventHandler};

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
            let start_handler = NavigationStartingEventHandler::create(Box::new(
                move |_sender, args: Option<ICoreWebView2NavigationStartingEventArgs>| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let mut nav_id: u64 = 0;
                    args.NavigationId(&mut nav_id)?;
                    let mut uri_pw = windows::core::PWSTR::null();
                    args.Uri(&mut uri_pw)?;
                    let uri = webview2_com::take_pwstr(uri_pw);
                    nav_urls_start.borrow_mut().insert(nav_id, uri);
                    Ok(())
                },
            ));
            let mut start_token: i64 = 0;
            if let Err(e) = webview2.add_NavigationStarting(&start_handler, &mut start_token) {
                eprintln!("nav: add_NavigationStarting failed, URLs cannot be correlated: {e}");
            }

            let completed_handler = NavigationCompletedEventHandler::create(Box::new(
                move |_sender, args: Option<ICoreWebView2NavigationCompletedEventArgs>| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let mut nav_id: u64 = 0;
                    args.NavigationId(&mut nav_id)?;
                    let uri = nav_urls.borrow_mut().remove(&nav_id);

                    // C1: only genuine remote/content navigations reach the FSM. An
                    // app-origin bundled page (the error page's own commit!) or an
                    // uncorrelated navId (e.g. the very first boot splash, whose
                    // NavigationStarting fired before this handler was live) is
                    // suppressed — the FSM must never see it.
                    if !matches!(&uri, Some(u) if super::feeds_fsm(u)) {
                        return Ok(());
                    }

                    let mut is_success = windows::core::BOOL(0);
                    args.IsSuccess(&mut is_success)?;
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
    use super::feeds_fsm;

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
