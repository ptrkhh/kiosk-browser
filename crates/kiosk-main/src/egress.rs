//! Subresource egress containment (P1-D2b Task 4, spec §7 / SEC-10, Windows).
//!
//! `crate::nav`'s `NavigationStarting` guard covers top-level (main-frame) navigations
//! only. A loaded page can still exfiltrate data without ever navigating: an `<img
//! src=https://evil/a>`, a CSS `url(https://evil/a)`, a `fetch()`, or a beacon are all
//! subresource loads, not navigations, and none of them ever reaches `NavigationStarting`
//! (confirmed against `nav.rs`'s own doc comment on that event). This module closes that
//! hole by subscribing WebView2's `WebResourceRequested` with an ALL-contexts filter and
//! substituting a synthetic 403 for anything `NavPolicy::resource_allowed` denies.
//!
//! `WebResourceRequested`'s args have **no `Cancel`/`SetCancel`** (unlike every other
//! guard in this crate) — the only way to stop the load is to set `args.Response` to a
//! response object built via the owning `ICoreWebView2Environment::CreateWebResourceResponse`.
//! See `windows_impl::install` for the exact bindings.rs call sites.

use std::path::PathBuf;
use std::sync::Arc;

use crate::nav_policy::{NavPolicy, SharedNavPolicy};
use crate::telemetry::Telemetry;

/// Stable, greppable `nav.blocked` reason label for this module's block class (plan-
/// defined, same convention as `scheme_guard::REASON_DOWNLOAD` — no
/// `kiosk_core::nav::BlockReason` variant exists for it, and adding one is out of scope).
const REASON_EGRESS: &str = "egress";

pub type PolicyUpdates = std::sync::mpsc::Receiver<Arc<NavPolicy>>;

#[cfg(windows)]
pub fn install(
    window: &tauri::WebviewWindow,
    telem: Telemetry,
    nav_policy: SharedNavPolicy,
    _policy_updates: PolicyUpdates,
    _data_dir: PathBuf,
    _app_origin: &'static str,
    _asset_origin: &'static str,
) {
    windows_impl::install(window, telem, nav_policy);
}

#[cfg(not(windows))]
pub fn install(
    window: &tauri::WebviewWindow,
    telem: Telemetry,
    nav_policy: SharedNavPolicy,
    policy_updates: PolicyUpdates,
    data_dir: PathBuf,
    app_origin: &'static str,
    asset_origin: &'static str,
) {
    linux_impl::install(
        window,
        telem,
        nav_policy,
        policy_updates,
        data_dir,
        app_origin,
        asset_origin,
    );
}

#[cfg(windows)]
mod windows_impl {
    use windows::core::{Interface, HSTRING};

    use crate::nav_policy::SharedNavPolicy;
    use crate::telemetry::Telemetry;

    pub fn install(window: &tauri::WebviewWindow, telem: Telemetry, nav_policy: SharedNavPolicy) {
        let result = window.with_webview(move |platform_webview| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2WebResourceRequestedEventArgs, ICoreWebView2_2,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            };
            use webview2_com::WebResourceRequestedEventHandler;

            let controller = platform_webview.controller();
            let webview2 = match controller.CoreWebView2() {
                Ok(w) => w,
                Err(e) => {
                    eprintln!(
                        "egress: CoreWebView2() unavailable, subresource egress will never be blocked: {e}"
                    );
                    return;
                }
            };

            // `Environment()` — needed to synthesize the blocked `Response` below — lives
            // on `ICoreWebView2_2` (webview2-com-sys 0.38.2's bindings.rs:41053, inside
            // `impl ICoreWebView2_2` starting at bindings.rs:40975), not the base
            // `ICoreWebView2`, so the base interface obtained from the controller must be
            // cast up first (same idiom as `scheme_guard`'s `ICoreWebView2_18`/`_4` casts).
            let webview2_2 = match webview2.cast::<ICoreWebView2_2>() {
                Ok(w) => w,
                Err(e) => {
                    eprintln!(
                        "egress: CoreWebView2 does not implement ICoreWebView2_2, subresource egress will never be blocked: {e}"
                    );
                    return;
                }
            };
            let environment = match webview2_2.Environment() {
                Ok(env) => env,
                Err(e) => {
                    eprintln!(
                        "egress: Environment() failed, subresource egress will never be blocked: {e}"
                    );
                    return;
                }
            };

            // `AddWebResourceRequestedFilter("*", COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)`
            // (bindings.rs:1708) is required before `WebResourceRequested` fires for
            // anything — an unfiltered subscription sees no events at all.
            if let Err(e) = webview2
                .AddWebResourceRequestedFilter(&HSTRING::from("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)
            {
                eprintln!(
                    "egress: AddWebResourceRequestedFilter failed, subresource egress will never be blocked: {e}"
                );
                return;
            }

            let handler = WebResourceRequestedEventHandler::create(Box::new(
                move |_sender, args: Option<ICoreWebView2WebResourceRequestedEventArgs>| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let request = args.Request()?;
                    let mut uri_pw = windows::core::PWSTR::null();
                    request.Uri(&mut uri_pw)?;
                    let uri = webview2_com::take_pwstr(uri_pw);

                    if nav_policy.load().resource_allowed(&uri) {
                        return Ok(());
                    }

                    // No `Cancel` on this args type (bindings.rs:37812-37866): the load is
                    // stopped by substituting a synthetic 403 `Response` instead of letting
                    // the real (possibly data-carrying) request go out.
                    //
                    // Telemetry-flood note: a single hostile page can issue 100+ off-list
                    // requests (every pixel of a tracking pixel farm, say). We deliberately
                    // do NOT add a second limiter here — `Telemetry::nav_blocked` already
                    // feeds the Logger's `nav.blocked` bucket, which coalesces at 20/burst
                    // then emits a suppressed-summary (shared with `nav.rs`/`scheme_guard`'s
                    // own blocked events); a second bucket would just double-count the same
                    // burst under a different key.
                    // Emit `nav.blocked{egress}` ONLY when the substitution actually
                    // took — a failed 403 substitution is fail-open (the data egressed),
                    // so logging "blocked" there would make the dashboard lie.
                    match environment.CreateWebResourceResponse(
                        None::<&windows::Win32::System::Com::IStream>,
                        403,
                        &HSTRING::from("Forbidden"),
                        &HSTRING::from(""),
                    ) {
                        Ok(response) => match args.SetResponse(&response) {
                            Ok(()) => telem.nav_blocked(super::REASON_EGRESS, &uri),
                            Err(e) => eprintln!(
                                "egress: SetResponse failed, request left un-substituted (fail-open): {e}"
                            ),
                        },
                        Err(e) => eprintln!(
                            "egress: CreateWebResourceResponse failed, request left un-substituted (fail-open): {e}"
                        ),
                    }
                    Ok(())
                },
            ));
            let mut token: i64 = 0;
            if let Err(e) = webview2.add_WebResourceRequested(&handler, &mut token) {
                eprintln!(
                    "egress: add_WebResourceRequested failed, subresource egress will never be blocked: {e}"
                );
            }
        });
        if let Err(e) = result {
            eprintln!("egress: with_webview failed, subresource egress will never be blocked: {e}");
        }
    }
}

#[cfg(not(windows))]
mod linux_impl {
    //! WebKitGTK 2.24 exposes the content-filter store in C but the safe
    //! `webkit2gtk` bindings comment out `add_filter`/`remove_filter`. The small
    //! FFI island below uses only those generated functions and keeps every raw
    //! pointer alive until the async callback completes. Enforcement belongs to
    //! the native filter; resource-load signals are observation only.

    use std::cell::RefCell;
    use std::ffi::CString;
    use std::path::{Path, PathBuf};
    use std::ptr;
    use std::rc::Rc;
    use std::time::Duration;

    use kiosk_core::nav::filter::FilterOutput;
    use webkit2gtk::glib::translate::{from_glib_full, ToGlibPtr};
    use webkit2gtk::{gio, glib};
    use webkit2gtk::{
        URIRequestExt, UserContentInjectedFrames, UserContentManagerExt, UserScript,
        UserScriptInjectionTime, WebResourceExt, WebViewExt,
    };

    use super::PolicyUpdates;
    use crate::nav_policy::{NavPolicy, SharedNavPolicy};
    use crate::telemetry::Telemetry;

    struct FilterRequest {
        identifier: String,
        output: FilterOutput,
    }

    struct FilterState {
        active: Option<String>,
        pending: Option<FilterRequest>,
        saving: bool,
        next_identifier: u64,
    }

    struct SaveState {
        manager: webkit2gtk::UserContentManager,
        store: *mut webkit2gtk_sys::WebKitUserContentFilterStore,
        source: webkit2gtk::glib::Bytes,
        identifier: CString,
        previous: Option<String>,
        controller: Rc<RefCell<FilterState>>,
        storage_path: PathBuf,
        telem: Telemetry,
    }

    pub fn install(
        window: &tauri::WebviewWindow,
        telem: Telemetry,
        nav_policy: SharedNavPolicy,
        policy_updates: PolicyUpdates,
        data_dir: PathBuf,
        app_origin: &'static str,
        asset_origin: &'static str,
    ) {
        let telem_for_webview = telem.clone();
        let result = window.with_webview(move |platform_webview| {
            let telem = telem_for_webview;
            let webview = platform_webview.inner();
            let Some(manager) = webview.user_content_manager() else {
                telem.config_error("egress.filter_absent");
                eprintln!("egress: WebKit user-content-manager unavailable");
                return;
            };

            let storage_path = data_dir.join("content-filters");
            if let Err(error) = std::fs::create_dir_all(&storage_path) {
                telem.config_error("egress.filter_absent");
                eprintln!("egress: cannot create {}: {error}", storage_path.display());
            }

            // CSP is a belt only. It is replaced/removed on every policy update;
            // a refused pattern never leaves an older, tighter CSP behind.
            let csp_script = Rc::new(RefCell::new(None));
            install_csp(
                &manager,
                &csp_script,
                &nav_policy.load(),
                &telem,
                app_origin,
                asset_origin,
            );

            // Layer 1: compile and queue the initial native filter.
            let filter_state = Rc::new(RefCell::new(FilterState {
                active: None,
                pending: None,
                saving: false,
                next_identifier: 0,
            }));
            enqueue_policy(
                &manager,
                &filter_state,
                &storage_path,
                &telem,
                &nav_policy.load(),
            );

            // Layer 2 companion: record only failed off-policy resources. The
            // filter remains the enforcement authority, so this callback never
            // cancels, rewrites, or otherwise changes a request.
            let policy_observe = nav_policy.clone();
            let telem_observe = telem.clone();
            webview.connect_resource_load_started(move |_wv, resource, request| {
                let Some(uri) = request.uri() else { return };
                let uri = uri.to_string();
                let policy = policy_observe.clone();
                let telem = telem_observe.clone();
                resource.connect_failed(move |_resource, _error| {
                    if !policy.load().resource_allowed(&uri) {
                        telem.nav_blocked(super::REASON_EGRESS, &uri);
                    }
                });
            });

            let manager_updates = manager.clone();
            let state_updates = filter_state.clone();
            let csp_updates = csp_script.clone();
            let policy_updates = Rc::new(RefCell::new(policy_updates));
            let policy_updates_policy = nav_policy.clone();
            let storage_updates = storage_path.clone();
            let telem_updates = telem.clone();
            webkit2gtk::glib::source::timeout_add_local(Duration::from_millis(100), move || {
                let mut latest = None;
                while let Ok(policy) = policy_updates.borrow().try_recv() {
                    latest = Some(policy);
                }
                if let Some(policy) = latest {
                    install_csp(
                        &manager_updates,
                        &csp_updates,
                        &policy,
                        &telem_updates,
                        app_origin,
                        asset_origin,
                    );
                    enqueue_policy(
                        &manager_updates,
                        &state_updates,
                        &storage_updates,
                        &telem_updates,
                        &policy,
                    );
                } else {
                    // The receiver is allowed to outlive fetch during normal
                    // shutdown; ArcSwap remains the source for the observer.
                    let _ = &policy_updates_policy;
                }
                glib::ControlFlow::Continue
            });
        });
        if let Err(e) = result {
            eprintln!("egress: with_webview failed, Linux filter was not installed: {e}");
            telem.config_error("egress.filter_absent");
        }
    }

    fn enqueue_policy(
        manager: &webkit2gtk::UserContentManager,
        controller: &Rc<RefCell<FilterState>>,
        storage_path: &Path,
        telem: &Telemetry,
        policy: &NavPolicy,
    ) {
        let output = policy.egress_filter();
        for pattern in &output.refused {
            telem.config_warn("egress.filter_pattern", pattern);
        }
        let identifier = {
            let mut state = controller.borrow_mut();
            state.next_identifier = state.next_identifier.saturating_add(1);
            format!("kiosk-{}-{}", std::process::id(), state.next_identifier)
        };
        let request = FilterRequest { identifier, output };
        let start_now = {
            let mut state = controller.borrow_mut();
            if state.saving {
                state.pending = Some(request);
                None
            } else {
                state.saving = true;
                Some(request)
            }
        };
        if let Some(request) = start_now {
            start_save(
                manager.clone(),
                controller.clone(),
                storage_path.to_path_buf(),
                telem.clone(),
                request,
            );
        }
    }

    fn start_save(
        manager: webkit2gtk::UserContentManager,
        controller: Rc<RefCell<FilterState>>,
        storage_path: PathBuf,
        telem: Telemetry,
        request: FilterRequest,
    ) {
        let previous = controller.borrow().active.clone();
        let Ok(identifier) = CString::new(request.identifier.clone()) else {
            telem.config_error("egress.filter_absent");
            finish_save(
                manager,
                controller,
                storage_path,
                telem,
                request.identifier,
                false,
                previous,
            );
            return;
        };
        let Ok(storage) = CString::new(storage_path.to_string_lossy().as_bytes()) else {
            telem.config_error("egress.filter_absent");
            finish_save(
                manager,
                controller,
                storage_path,
                telem,
                request.identifier,
                false,
                previous,
            );
            return;
        };
        let source = webkit2gtk::glib::Bytes::from(request.output.json.as_bytes());
        let store =
            unsafe { webkit2gtk_sys::webkit_user_content_filter_store_new(storage.as_ptr()) };
        if store.is_null() {
            telem.config_error("egress.filter_absent");
            finish_save(
                manager,
                controller,
                storage_path,
                telem,
                request.identifier,
                false,
                previous,
            );
            return;
        }
        let state = Box::new(SaveState {
            manager,
            store,
            source,
            identifier,
            previous,
            controller,
            storage_path,
            telem,
        });
        let source_ptr = state.source.to_glib_none().0;
        let id_ptr = state.identifier.as_ptr();
        let store_ptr = state.store;
        let raw_state = Box::into_raw(state);
        unsafe {
            webkit2gtk_sys::webkit_user_content_filter_store_save(
                store_ptr,
                id_ptr,
                source_ptr,
                ptr::null_mut(),
                Some(save_finished),
                raw_state as glib::ffi::gpointer,
            );
        }
    }

    unsafe extern "C" fn save_finished(
        _source_object: *mut glib::gobject_ffi::GObject,
        result: *mut gio::ffi::GAsyncResult,
        user_data: glib::ffi::gpointer,
    ) {
        let state = Box::from_raw(user_data as *mut SaveState);
        let mut error = ptr::null_mut();
        let filter = webkit2gtk_sys::webkit_user_content_filter_store_save_finish(
            state.store,
            result,
            &mut error,
        );
        let mut success = !filter.is_null();
        if !error.is_null() {
            let error: glib::Error = from_glib_full(error);
            eprintln!("egress: content-filter save failed: {error}");
            state.telem.config_error("egress.filter_absent");
            success = false;
        }
        if success {
            webkit2gtk_sys::webkit_user_content_manager_add_filter(
                state.manager.to_glib_none().0,
                filter,
            );
            if let Some(previous) = &state.previous {
                state.manager.remove_filter_by_id(previous);
            }
            webkit2gtk_sys::webkit_user_content_filter_unref(filter);
        } else if !filter.is_null() {
            webkit2gtk_sys::webkit_user_content_filter_unref(filter);
        }
        glib::gobject_ffi::g_object_unref(state.store as *mut glib::gobject_ffi::GObject);
        let manager = state.manager.clone();
        let controller = state.controller.clone();
        let storage_path = state.storage_path.clone();
        let telem = state.telem.clone();
        let identifier = state.identifier.to_string_lossy().into_owned();
        let previous = state.previous.clone();
        drop(state);
        finish_save(
            manager,
            controller,
            storage_path,
            telem,
            identifier,
            success,
            previous,
        );
    }

    fn finish_save(
        manager: webkit2gtk::UserContentManager,
        controller: Rc<RefCell<FilterState>>,
        storage_path: PathBuf,
        telem: Telemetry,
        identifier: String,
        success: bool,
        _previous: Option<String>,
    ) {
        let next = {
            let mut state = controller.borrow_mut();
            if success {
                state.active = Some(identifier);
            }
            state.saving = false;
            state.pending.take()
        };
        if let Some(request) = next {
            let mut state = controller.borrow_mut();
            state.saving = true;
            drop(state);
            start_save(manager, controller, storage_path, telem, request);
        }
    }

    fn install_csp(
        manager: &webkit2gtk::UserContentManager,
        current: &Rc<RefCell<Option<UserScript>>>,
        policy: &NavPolicy,
        telem: &Telemetry,
        app_origin: &str,
        asset_origin: &str,
    ) {
        if let Some(previous) = current.borrow_mut().take() {
            manager.remove_script(&previous);
        }
        let Some(csp) = policy.csp_policy(app_origin, asset_origin) else {
            telem.config_error("egress.csp_absent");
            return;
        };
        let source = format!(
            "(function(){{var p={};function a(){{var h=document.head||document.documentElement;if(!h)return;var m=document.querySelector('meta[http-equiv=\\\"Content-Security-Policy\\\"]');if(!m){{m=document.createElement('meta');m.httpEquiv='Content-Security-Policy';h.insertBefore(m,h.firstChild);}}m.content=p;}}a();document.addEventListener('readystatechange',a,true);}})();",
            serde_json::to_string(&csp).expect("CSP string is JSON serializable")
        );
        let script = UserScript::new(
            &source,
            UserContentInjectedFrames::AllFrames,
            UserScriptInjectionTime::Start,
            &[],
            &[],
        );
        manager.add_script(&script);
        *current.borrow_mut() = Some(script);
    }
}

#[cfg(test)]
mod tests {
    /// `REASON_EGRESS`'s stable label is pinned here so a future refactor cannot silently
    /// drift from the plan's literal `"egress"`.
    #[test]
    fn the_stable_egress_block_reason_label_is_pinned() {
        assert_eq!(super::REASON_EGRESS, "egress");
    }
}
