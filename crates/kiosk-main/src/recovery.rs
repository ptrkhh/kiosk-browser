//! Renderer crash/hang recovery (P1-D2b Task 7 — the P1 spec's "renderer hang +
//! crash recovery" deliverable): subscribes `ICoreWebView2.ProcessFailed` so a
//! wedged or dead renderer process self-heals instead of leaving the kiosk on a
//! permanently black/frozen screen with no operator on site to reboot it.
//!
//! `ProcessFailed`'s `COREWEBVIEW2_PROCESS_FAILED_KIND` (webview2-com-sys 0.38.2
//! bindings.rs:702-722) has ten values; this module only special-cases the one that
//! needs a DIFFERENT recovery than the rest —
//! `COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE` (raw `2`, a hang:
//! the process is still alive, so `Reload()` on the existing `ICoreWebView2` is
//! enough) — and treats every other kind (render-process-exited, frame-render-
//! process-exited, the browser/GPU/utility/sandbox/PPAPI process kinds, and any
//! future kind this crate doesn't yet name) as "the process is gone", recovered by
//! navigating the (WebView2-recreated-under-the-hood) webview back to `home`.
//!
//! **Recreate-vs-reload-fallback, and why this took the fallback (concern, see
//! task-7-report.md):** the brief's preferred shape is "recreate/reload the
//! webview" on a crash. A full teardown-and-rebuild of the Tauri-owned
//! `WebviewWindow`/`ICoreWebView2Controller` from inside this COM callback isn't
//! attempted here — Tauri's `WebviewWindow` is managed by the app's own event loop
//! and has no documented "rebuild this window's webview in place" entry point
//! reachable from a background COM thread, and improvising one under time pressure
//! risks exactly the kind of fragile, unverified mechanism the brief warns against.
//! In practice `Navigate()` after a `RENDER_PROCESS_EXITED` already achieves the
//! same user-visible outcome: WebView2 transparently spins up a fresh renderer
//! process for the next navigation on the *same* `ICoreWebView2`/controller/window
//! (the crashed process, not the controller, is what's gone) — so navigating home
//! IS the practical recreate for this failure class, without reaching for a riskier
//! full-object rebuild. `Reload()` for the hang case is the brief's own instruction,
//! not a fallback.

use crate::nav_policy::SharedNavPolicy;
use crate::telemetry::Telemetry;

/// What [`recovery_action`] decided to do about a `ProcessFailed` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    /// The renderer process is still alive but wedged: reload the current page on
    /// the existing `ICoreWebView2`.
    Reload,
    /// The renderer process is gone: navigate back to `home` (see module doc on
    /// why this stands in for "recreate the webview").
    NavigateHome,
}

// Raw `COREWEBVIEW2_PROCESS_FAILED_KIND` values (webview2-com-sys 0.38.2
// bindings.rs:702-722), pinned here (not imported from `windows`) so
// [`recovery_action`]/[`kind_label`] stay pure and host-testable on every target —
// same convention as `hardening::classify_permission_kind`.
const KIND_BROWSER_PROCESS_EXITED: i32 = 0;
const KIND_RENDER_PROCESS_EXITED: i32 = 1;
const KIND_RENDER_PROCESS_UNRESPONSIVE: i32 = 2;
const KIND_FRAME_RENDER_PROCESS_EXITED: i32 = 3;
const KIND_UTILITY_PROCESS_EXITED: i32 = 4;
const KIND_SANDBOX_HELPER_PROCESS_EXITED: i32 = 5;
const KIND_GPU_PROCESS_EXITED: i32 = 6;
const KIND_PPAPI_PLUGIN_PROCESS_EXITED: i32 = 7;
const KIND_PPAPI_BROKER_PROCESS_EXITED: i32 = 8;
const KIND_UNKNOWN_PROCESS_EXITED: i32 = 9;

/// Pure, host-tested: the only kind that gets `Reload` instead of `NavigateHome`
/// is the hang (`RENDER_PROCESS_UNRESPONSIVE`) — see module doc.
pub fn recovery_action(raw_kind: i32) -> RecoveryAction {
    if raw_kind == KIND_RENDER_PROCESS_UNRESPONSIVE {
        RecoveryAction::Reload
    } else {
        RecoveryAction::NavigateHome
    }
}

/// A stable, greppable label for the `webview.crash` telemetry event's `kind`
/// field (spec §6 taxonomy) — never the WebView2 enum's Rust type name, which
/// isn't guaranteed stable across `webview2-com-sys` versions.
pub fn kind_label(raw_kind: i32) -> &'static str {
    match raw_kind {
        KIND_BROWSER_PROCESS_EXITED => "browser_process_exited",
        KIND_RENDER_PROCESS_EXITED => "render_process_exited",
        KIND_RENDER_PROCESS_UNRESPONSIVE => "render_process_unresponsive",
        KIND_FRAME_RENDER_PROCESS_EXITED => "frame_render_process_exited",
        KIND_UTILITY_PROCESS_EXITED => "utility_process_exited",
        KIND_SANDBOX_HELPER_PROCESS_EXITED => "sandbox_helper_process_exited",
        KIND_GPU_PROCESS_EXITED => "gpu_process_exited",
        KIND_PPAPI_PLUGIN_PROCESS_EXITED => "ppapi_plugin_process_exited",
        KIND_PPAPI_BROKER_PROCESS_EXITED => "ppapi_broker_process_exited",
        KIND_UNKNOWN_PROCESS_EXITED => "unknown_process_exited",
        _ => "unrecognized",
    }
}

/// A stable, greppable label for the `webview.crash` telemetry `kind` field, in the
/// **WebKit** reason space. Deliberately prefixed `webkit_` and deliberately NOT routed
/// through `kind_label`, whose `i32`s are WebView2 constants with different meanings.
#[cfg(not(windows))]
fn termination_label(reason: webkit2gtk::WebProcessTerminationReason) -> &'static str {
    use webkit2gtk::WebProcessTerminationReason as R;
    match reason {
        R::Crashed => "webkit_crashed",
        R::ExceededMemoryLimit => "webkit_exceeded_memory_limit",
        R::TerminatedByApi => "webkit_terminated_by_api",
        _ => "webkit_unrecognized",
    }
}

#[cfg(windows)]
pub fn install(window: &tauri::WebviewWindow, telem: Telemetry, nav_policy: SharedNavPolicy) {
    windows_impl::install(window, telem, nav_policy);
}

#[cfg(not(windows))]
pub fn install(window: &tauri::WebviewWindow, telem: Telemetry, nav_policy: SharedNavPolicy) {
    linux_impl::install(window, telem, nav_policy);
}

#[cfg(windows)]
mod windows_impl {
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED;

    use crate::nav_policy::SharedNavPolicy;
    use crate::telemetry::Telemetry;

    pub fn install(window: &tauri::WebviewWindow, telem: Telemetry, nav_policy: SharedNavPolicy) {
        let result = window.with_webview(move |platform_webview| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2ProcessFailedEventArgs;
            use webview2_com::ProcessFailedEventHandler;

            let controller = platform_webview.controller();
            let webview2 = match controller.CoreWebView2() {
                Ok(w) => w,
                Err(e) => {
                    eprintln!(
                        "recovery: CoreWebView2() unavailable, renderer crash/hang will never be recovered: {e}"
                    );
                    return;
                }
            };

            let webview2_failed = webview2.clone();
            let handler = ProcessFailedEventHandler::create(Box::new(move |_sender, args: Option<ICoreWebView2ProcessFailedEventArgs>| -> windows::core::Result<()> {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED;
                args.ProcessFailedKind(&mut kind)?;

                telem.webview_crash(super::kind_label(kind.0));

                match super::recovery_action(kind.0) {
                    super::RecoveryAction::Reload => {
                        if let Err(e) = webview2_failed.Reload() {
                            eprintln!("recovery: Reload() after RenderProcessUnresponsive failed: {e}");
                        }
                    }
                    super::RecoveryAction::NavigateHome => {
                        // Read the LIVE home at crash time (not a boot snapshot): an
                        // operator config push after boot re-stores this policy, so a
                        // later crash recovers to the CURRENT home.
                        // ponytail: no backoff/circuit-breaker — a renderer that crashes
                        // right after loading loops ProcessFailed->Navigate. Self-paced by
                        // the crash cycle (no CPU spin) and webview_crash is rate-capped,
                        // so no flood; add a crash-count circuit-breaker if that loop ever
                        // shows up in practice.
                        let home = nav_policy.load().home().to_string();
                        if let Err(e) = webview2_failed.Navigate(&windows::core::HSTRING::from(&home)) {
                            eprintln!("recovery: Navigate(home) after ProcessFailed failed: {e}");
                        }
                    }
                }
                Ok(())
            }));
            // webview2-com-sys 0.38.2: `add_ProcessFailed`'s token out-param is a raw
            // `*mut i64` — same Win32-interop shape as every other `add_*` in this crate.
            let mut token: i64 = 0;
            if let Err(e) = webview2.add_ProcessFailed(&handler, &mut token) {
                eprintln!(
                    "recovery: add_ProcessFailed failed, renderer crash/hang will never be recovered: {e}"
                );
            }
        });
        if let Err(e) = result {
            eprintln!(
                "recovery: with_webview failed, renderer crash/hang will never be recovered: {e}"
            );
        }
    }
}

#[cfg(not(windows))]
mod linux_impl {
    use webkit2gtk::WebViewExt;

    use crate::nav_policy::SharedNavPolicy;
    use crate::telemetry::Telemetry;

    /// All three WebKit termination reasons mean the web process is gone, so all three
    /// take `NavigateHome`. There is no `Reload` branch: Windows reserves it for
    /// `RENDER_PROCESS_UNRESPONSIVE`, which has no WebKitGTK analogue (hang detection on
    /// Linux is the JS ping, P2-C C17).
    pub fn install(window: &tauri::WebviewWindow, telem: Telemetry, nav_policy: SharedNavPolicy) {
        let window_handle = window.clone();
        let result = window.with_webview(move |platform_webview| {
            let webview = platform_webview.inner();
            webview.connect_web_process_terminated(move |_wv, reason| {
                let label = super::termination_label(reason);
                telem.webview_crash(label);
                let home = nav_policy.load().home().to_string();
                match tauri::Url::parse(&home) {
                    Ok(url) => {
                        if let Err(e) = window_handle.navigate(url) {
                            eprintln!("recovery: navigate home failed after {label}: {e}");
                        }
                    }
                    Err(e) => eprintln!("recovery: home URL unparseable ({home}): {e}"),
                }
            });
        });
        if let Err(e) = result {
            eprintln!("recovery: with_webview failed, renderer crash will never be recovered: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_process_unresponsive_reloads_in_place() {
        assert_eq!(recovery_action(2), RecoveryAction::Reload);
        assert_eq!(kind_label(2), "render_process_unresponsive");
    }

    #[test]
    fn render_process_exited_navigates_home() {
        assert_eq!(recovery_action(1), RecoveryAction::NavigateHome);
        assert_eq!(kind_label(1), "render_process_exited");
    }

    #[test]
    fn frame_render_process_exited_navigates_home() {
        assert_eq!(recovery_action(3), RecoveryAction::NavigateHome);
        assert_eq!(kind_label(3), "frame_render_process_exited");
    }

    #[test]
    fn every_other_named_kind_navigates_home() {
        for raw in [0, 4, 5, 6, 7, 8, 9] {
            assert_eq!(
                recovery_action(raw),
                RecoveryAction::NavigateHome,
                "raw kind {raw} must recover via NavigateHome"
            );
        }
    }

    #[test]
    fn an_unrecognized_future_kind_defaults_to_navigate_home_not_a_panic() {
        assert_eq!(recovery_action(99), RecoveryAction::NavigateHome);
        assert_eq!(kind_label(99), "unrecognized");
    }

    #[cfg(not(windows))]
    #[test]
    fn every_webkit_termination_reason_has_a_label() {
        use webkit2gtk::WebProcessTerminationReason as R;
        assert_eq!(termination_label(R::Crashed), "webkit_crashed");
        assert_eq!(
            termination_label(R::ExceededMemoryLimit),
            "webkit_exceeded_memory_limit"
        );
        assert_eq!(
            termination_label(R::TerminatedByApi),
            "webkit_terminated_by_api"
        );
        assert_eq!(termination_label(R::__Unknown(99)), "webkit_unrecognized");
    }

    /// The two constant spaces must never be crossed: WebKit's `Crashed` is 0, which
    /// `kind_label` would render as WebView2's `browser_process_exited`.
    #[cfg(not(windows))]
    #[test]
    fn webkit_labels_never_collide_with_the_webview2_space() {
        use webkit2gtk::WebProcessTerminationReason as R;
        assert_ne!(termination_label(R::Crashed), kind_label(0));
    }
}
