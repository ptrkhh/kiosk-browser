//! The app state machine types and transition function (spec §3.3, §3.5).
//!
//! [`Machine::on`] is the whole contract: feed it one [`Event`], it mutates the visible
//! [`AppState`] and returns the [`Effect`]s `kiosk-main` must execute. Pure and total —
//! every `(state, event)` pair has a defined outcome, and an unhandled pair is a
//! deliberate no-op (never a panic), because a locked kiosk must degrade, not crash, on a
//! stray event.
//!
//! # Carrying the home url (a deliberate design decision)
//!
//! The last-applied home url is held as [`Machine::home`], a single `Option<String>`
//! field on the machine — **not** threaded through the `Offline` / `ErrorPage` /
//! `Clearing` state variants. Rules 3, 4 and 9 all re-navigate "home" after a detour
//! through Offline / idle-reset, and the named failure mode (spec/plan) is *a machine that
//! reaches `Offline` and then cannot re-navigate on reconnect — a stuck kiosk*. Centralising
//! the url in one field written in exactly one place ([`Machine::go_online`]) means every
//! path into a non-Online state retains it for free; threading it through each variant
//! would create N transitions that must each remember to copy it, and a single missed copy
//! *is* the stuck kiosk. `Online { url }` still carries its url because that url is the
//! identity of what is on screen — but it is always a clone of `home`, by construction.
//!
//! # ErrorPage retry delivery (a deliberate design decision)
//!
//! The retry is modelled as an *effect*, not as a result carried by an event. On
//! [`Event::CountdownExpired`] the machine (Task 2) emits a [`Effect::Navigate`] retry and
//! stays in [`AppState::ErrorPage`]; the webview reports the outcome back through the same
//! [`Event::NavigationCommitted`] / [`Event::NavigationFailed`] events the `Online` state
//! already uses. This needs *fewer* event variants than a `CountdownExpired { ok: bool }`
//! model and gives the driver exactly one way to report a navigation result, so the
//! initial load and an ErrorPage retry are indistinguishable to it. Task 1 defines these
//! events and handles `NavigationFailed` in `Online` (rule 5); Task 2 adds the `ErrorPage`
//! arms without reshaping anything.
//!
//! # Driver contract: `ConfigApplied` must be level-triggered (a P1-D2 requirement)
//!
//! A `ConfigApplied` whose url *changed* is only acted on while `Online` (navigate) or
//! `Offline` (`go_online`). Delivered while `ErrorPage` or `Clearing`, it is a deliberate
//! no-op — re-navigating mid-retry or mid-clear would break the retry loop or the privacy
//! gate. That is safe **only if `kiosk-main` re-emits `ConfigApplied` at level** (idempotent,
//! on every poll — the same-url `Online` arm already absorbs the repeats), not edge-triggered
//! (once per change). Under an edge-triggered driver a url change that lands during a retry
//! or clear would be dropped and never recover; under the level-triggered contract the FSM
//! converges on the newest url the next time it is back in `Online`/`Offline`.

use crate::config::schema::Fallback;
use crate::net::prober::Link;

/// Default retry-countdown length for [`AppState::ErrorPage`], in seconds.
///
/// The spec (§3.3) describes an "auto-retry countdown" but defines **no** remote-config
/// field for its *length* — `content.error_max_retries` bounds the retry *count*, and
/// nothing bounds the *interval*. So this is a `kiosk-core` default that the driver
/// (`kiosk-main`) supplies into [`MachineConfig::error_retry_seconds`]; if a remote field
/// is ever added it threads in there without touching this FSM. 15 s sits between the 10 s
/// offline-probe and 30 s online-probe cadences (spec §3.3): long enough not to hammer a
/// failing origin, short enough to recover promptly.
pub const DEFAULT_ERROR_RETRY_SECONDS: u64 = 15;

/// The visible state of the kiosk (spec §3.3). Not persisted — see the module docs.
///
/// `ErrorPage`, `Clearing` and `Safe` are declared here so Tasks 2 & 3 (and P1-D2's
/// `--safe`) slot in additively; Task 1 implements only the `Boot`/`ConfigLoad`/`Online`/
/// `Offline` connectivity core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    /// Initial state, before any config has been resolved.
    Boot,
    /// Attempting to load config (cached last-good or bootstrap).
    ConfigLoad,
    /// Showing the content site at `url`. `url` is always the current [`Machine::home`].
    Online { url: String },
    /// Showing the looping offline video.
    Offline,
    /// Showing the bundled error page with a retry countdown (Task 2). `attempts` counts
    /// the consecutive failed loads so far (rule 7 / `error_max_retries`).
    ErrorPage { attempts: u32 },
    /// A profile clear is in flight after an idle reset (Task 3); re-display is gated until
    /// [`Event::ProfileCleared`] arrives, at which point the machine resumes into `next`
    /// (spec §3.5). Boxed because `AppState` would otherwise be infinitely sized.
    Clearing { next: Box<AppState> },
    /// Entered only out-of-band via `kiosk-main --safe`; **no** `Event` transitions into it.
    Safe,
}

/// An input to the state machine. Emitted by `kiosk-main` from the prober, the webview,
/// the idle timer, the profile-clear callback, and the config fetch.
///
/// `CountdownExpired`, `IdleExpired`, `ProfileCleared` and `Reconnected` are declared for
/// Tasks 2 & 3; Task 1 accepts them but treats them as no-ops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Config loaded/applied; the effective home url to navigate to.
    ConfigApplied { url: String },
    /// No cached config and no network (spec §3.3: "no cached config + no network").
    ConfigUnavailable,
    /// The prober flipped (P1-C only fires this on a real, already-damped flip).
    LinkChanged(Link),
    /// The webview successfully loaded the current target.
    NavigationCommitted,
    /// The current target failed to load (DNS/TLS/HTTP) while the link is believed up.
    NavigationFailed,
    /// The ErrorPage retry countdown elapsed (Task 2). The machine re-navigates; the
    /// result arrives via the next `NavigationCommitted`/`NavigationFailed`.
    CountdownExpired,
    /// The native idle timer fired (Task 3, spec §3.5).
    IdleExpired,
    /// The async profile clear finished (Task 3, spec §3.5).
    ProfileCleared,
    /// Network returned; triggers an immediate config refetch (Task 3, spec §3.3).
    Reconnected,
}

/// A side effect for `kiosk-main` to execute. The FSM never acts (spec §4) — it only
/// returns these.
///
/// `ShowErrorPage`, `ShowSplash`, `ClearProfile` and `RefetchConfig` are declared for
/// Tasks 2 & 3; Task 1 emits only `Navigate` and `ShowVideo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Navigate the webview to this URL.
    Navigate(String),
    /// Display the offline video loop.
    ShowVideo,
    /// Display the bundled error page, armed to retry after N seconds.
    ShowErrorPage { retry_after_seconds: u64 },
    /// Display the bundled boot splash.
    ShowSplash,
    /// Clear cookies/storage (and autofill/login stores if `full`).
    ClearProfile { full: bool },
    /// Kick a config refetch (on reconnect).
    RefetchConfig,
}

/// The static configuration the machine needs to decide transitions. Built by `kiosk-main`
/// from the effective [`crate::config::schema::Content`].
#[derive(Debug, Clone)]
pub struct MachineConfig {
    /// `content.fallback` — video (default) or error page, when the site fails (rules 5/6).
    pub fallback: Fallback,
    /// `content.error_max_retries` — ErrorPage attempts before falling to Offline (Task 2).
    pub error_max_retries: u32,
    /// `content.clear_data_on_reset` — whether idle reset clears the profile (Task 3).
    pub idle_clear: bool,
    /// The ErrorPage retry-countdown length (Task 2). See [`DEFAULT_ERROR_RETRY_SECONDS`].
    pub error_retry_seconds: u64,
}

/// The app state machine. Pure: no I/O, no clock, no persistence (see the module docs).
#[derive(Debug)]
pub struct Machine {
    state: AppState,
    cfg: MachineConfig,
    /// The last-applied home url — the single source of truth for re-navigation after any
    /// detour through Offline / idle-reset. `None` until the first `ConfigApplied`.
    home: Option<String>,
}

impl Machine {
    /// Build a machine in [`AppState::Boot`] with no home yet. A fresh process always
    /// starts here (there is no persisted state to restore — see the module docs).
    pub fn new(cfg: MachineConfig) -> Machine {
        Machine {
            state: AppState::Boot,
            cfg,
            home: None,
        }
    }

    /// The current visible state.
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Feed one event; mutate the state and return the effects to execute.
    ///
    /// Task 1 implements the connectivity core (rules 1–5, 8). Every `(state, event)` pair
    /// not matched below is a deliberate no-op — a stray event on a locked kiosk must not
    /// crash — and is where Tasks 2 (ErrorPage) and 3 (idle-reset/reconnect) add their arms.
    pub fn on(&mut self, event: Event) -> Vec<Effect> {
        use AppState::*;
        use Event::*;

        match (&self.state, event) {
            // Rule 1 (+ its natural extensions): config resolved while we are NOT already
            // showing the site → navigate there. `Boot`/`ConfigLoad` are the first-config
            // paths; `Offline` is the recovery path (the no-config boot of rule 2, or a
            // refetch after going Offline) — without it a kiosk that booted with no config
            // would be stuck on the video forever. (`Online + ConfigApplied` is the next
            // arm: it navigates only when the url actually changed.)
            (Boot | ConfigLoad | Offline, ConfigApplied { url }) => self.go_online(url),

            // RT-04 / OD-6 (applied this revision): a config applied while Online that
            // CHANGES `content.url` must navigate to the new url; the same url re-applied
            // (the every-300s poll) must NOT reload the live page. `home` is the current
            // url by the `go_online` invariant, so the equality check gates whether we
            // navigate — and `go_online` refreshes `home` so a later rule-4 reconnect uses
            // the new url, never the stale one. (The revert-on-failure half of RT-04 —
            // fall back to config-lastgood when the post-change load fails while the link is
            // up — needs the ConfigManager + connectivity signal + telemetry and is P1-D2's;
            // see the report's handoff note.)
            (Online { .. }, ConfigApplied { url }) => {
                if self.home.as_deref() == Some(url.as_str()) {
                    Vec::new()
                } else {
                    self.go_online(url)
                }
            }

            // Rule 2: no cached config + no network → offline video, and keep probing.
            (Boot | ConfigLoad, ConfigUnavailable) => self.go_offline(),

            // Rule 3: the prober flips offline while showing the site → offline video.
            (Online { .. }, LinkChanged(Link::Offline)) => self.go_offline(),

            // Rule 4: the prober flips online while offline → re-navigate the remembered
            // home. If we never had config (reached Offline via rule 2, `home` is None),
            // there is nothing to navigate to yet: stay Offline and wait for the pending
            // fetch to deliver `ConfigApplied`, which recovers via the arm above. That is a
            // waiting kiosk, not a stuck one.
            (Offline, LinkChanged(Link::Online)) => match self.home.clone() {
                Some(url) => self.go_online(url),
                None => Vec::new(),
            },

            // Rule 5: the site failed while the link is up and `fallback` is video → offline
            // video. Paired with the rule-6 ErrorPage arm just below, the two `fallback` guards
            // are exhaustive over `(Online, NavigationFailed)` — so this arm genuinely branches
            // on `fallback`.
            (Online { .. }, NavigationFailed) if self.cfg.fallback == Fallback::Video => {
                self.go_offline()
            }

            // Rule 6 (ErrorPage entry): the site failed while the link is up and `fallback` is
            // error_page → show the bundled error page and arm the retry countdown. `attempts`
            // starts at 1 because this triggering failure IS failed-load #1 (see the
            // [`AppState::ErrorPage`] doc). The retry itself is modelled as an effect delivered
            // on the next `CountdownExpired` (see the module docs), not emitted here.
            (Online { .. }, NavigationFailed) if self.cfg.fallback == Fallback::ErrorPage => {
                self.state = ErrorPage { attempts: 1 };
                vec![Effect::ShowErrorPage {
                    retry_after_seconds: self.cfg.error_retry_seconds,
                }]
            }

            // Rule 7 (retry fired): the countdown elapsed → re-navigate the remembered home and
            // STAY in ErrorPage with `attempts` unchanged; the outcome arrives later as
            // `NavigationCommitted`/`NavigationFailed` (retry-as-effect, see the module docs).
            // `home` is always `Some` here — ErrorPage is only reachable from `Online`, which
            // requires config — but the machine stays total: no home → nothing to navigate.
            (ErrorPage { .. }, CountdownExpired) => match self.home.clone() {
                Some(url) => vec![Effect::Navigate(url)],
                None => Vec::new(),
            },

            // Rule 7 (retry succeeded): the retry navigation committed → back to `Online`. Set
            // the state DIRECTLY (not via `go_online`) with NO effect: the webview already
            // navigated on `CountdownExpired` and has now committed, so emitting another
            // `Navigate` would loop it. `home` is the committed url by construction.
            (ErrorPage { .. }, NavigationCommitted) => match self.home.clone() {
                Some(url) => {
                    self.state = Online { url };
                    Vec::new()
                }
                None => Vec::new(),
            },

            // Rule 7 (retry failed): another failed load → count it. `n` is the running count of
            // consecutive failed loads. At `n >= error_max_retries` we give up to the offline
            // video; below it we re-arm the countdown for another try. The comparison is `>=`
            // (not `>`): with error_max_retries = 5 the 5th failed load falls to Offline, not
            // the 6th — a `>` here is a kiosk that retries one time too many.
            (ErrorPage { attempts }, NavigationFailed) => {
                let n = attempts.saturating_add(1);
                if n >= self.cfg.error_max_retries {
                    self.go_offline()
                } else {
                    self.state = ErrorPage { attempts: n };
                    vec![Effect::ShowErrorPage {
                        retry_after_seconds: self.cfg.error_retry_seconds,
                    }]
                }
            }

            // Rule 7 (prober flips offline): the link dropped while on the error page → straight
            // to the offline video, regardless of remaining retries (spec §3.3: immediate).
            // Mirrors the rule-3 `Online` arm.
            (ErrorPage { .. }, LinkChanged(Link::Offline)) => self.go_offline(),

            // Rule 9 (idle reset, THE PRIVACY GATE, spec §3.5): the idle timer fired while
            // Online and clear_data_on_reset is set → clear the FULL profile and GATE re-display
            // behind `Clearing` until the async clear reports back via `ProfileCleared`. We emit
            // ONLY `ClearProfile` here — NO `Navigate`. Emitting a navigate now would display the
            // fresh home over un-cleared session data, the exact leak §3.5 exists to prevent; the
            // re-navigation is deferred to the `ProfileCleared` arm below. `home` is always `Some`
            // (Online implies config applied), but the machine stays total: no home → no-op.
            (Online { .. }, IdleExpired) if self.cfg.idle_clear => match self.home.clone() {
                Some(url) => {
                    self.state = Clearing {
                        next: Box::new(Online { url }),
                    };
                    vec![Effect::ClearProfile { full: true }]
                }
                None => Vec::new(),
            },

            // Rule 9 (idle reset, no-clear path): the idle timer fired while Online and
            // clear_data_on_reset is NOT set → just reload home fresh, no clear, no gating.
            (Online { .. }, IdleExpired) => match self.home.clone() {
                Some(url) => self.go_online(url),
                None => Vec::new(),
            },

            // Rule 9 (clear completed): the async profile clear reported back → release the gate
            // and resume into the target `next` recorded when we entered `Clearing`. Clone the
            // boxed target into a local FIRST to release the borrow into `self.state`, then honor
            // it: an `Online` target navigates home NOW (over the freshly cleared profile); any
            // other target is set without effect (unreachable in T3, keeps the machine total).
            (Clearing { next }, ProfileCleared) => {
                let resume = (**next).clone();
                match resume {
                    Online { url } => self.go_online(url),
                    other => {
                        self.state = other;
                        Vec::new()
                    }
                }
            }

            // Rule 10 (reconnect): network returned while Offline → kick a config refetch and
            // STAY Offline. The resulting `ConfigApplied` drives the nav via the Offline+
            // ConfigApplied arm above; navigating here would race that fetch on a stale home.
            (Offline, Reconnected) => vec![Effect::RefetchConfig],

            // Rule 8 + the Task 3 seam: a LinkChanged to the link we are already in is a no-op
            // (the prober already damped it), and every other unmatched pair is a no-op for now.
            _ => Vec::new(),
        }
    }

    /// Remember `url` as home and transition to [`AppState::Online`], emitting the navigate.
    /// The single writer of `home` and the single entry into `Online`, so the invariant
    /// "`Online.url` is always the current `home`" holds by construction — that one source
    /// of truth is what lets rule 4 re-navigate home after any detour through Offline.
    fn go_online(&mut self, url: String) -> Vec<Effect> {
        self.home = Some(url.clone());
        self.state = AppState::Online { url: url.clone() };
        vec![Effect::Navigate(url)]
    }

    /// Transition to [`AppState::Offline`], emitting the offline-video effect. `home` is
    /// left untouched — that is the whole point of holding it on the machine (rule 4).
    fn go_offline(&mut self) -> Vec<Effect> {
        self.state = AppState::Offline;
        vec![Effect::ShowVideo]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "https://home.example/kiosk";
    const HOME2: &str = "https://home.example/other";

    fn cfg(fallback: Fallback) -> MachineConfig {
        MachineConfig {
            fallback,
            error_max_retries: 5,
            idle_clear: true,
            error_retry_seconds: DEFAULT_ERROR_RETRY_SECONDS,
        }
    }

    /// Same as [`cfg`] but with `idle_clear: false` — the no-clear idle-reset path (Task 3).
    fn cfg_no_clear(fallback: Fallback) -> MachineConfig {
        MachineConfig {
            idle_clear: false,
            ..cfg(fallback)
        }
    }

    /// A machine driven from cold boot to `Online { HOME }` (so `home` is remembered).
    fn online(fallback: Fallback) -> Machine {
        let mut m = Machine::new(cfg(fallback));
        let fx = m.on(Event::ConfigApplied {
            url: HOME.to_string(),
        });
        assert_eq!(
            fx,
            vec![Effect::Navigate(HOME.to_string())],
            "premise: boot+ConfigApplied must navigate home"
        );
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME.to_string()
            },
            "premise: online after config applied"
        );
        m
    }

    // Rule 1 — Boot + ConfigApplied → Online + Navigate.
    #[test]
    fn boot_config_applied_navigates_and_goes_online() {
        let mut m = Machine::new(cfg(Fallback::Video));
        assert_eq!(m.state(), &AppState::Boot, "premise: starts in Boot");

        let fx = m.on(Event::ConfigApplied {
            url: HOME.to_string(),
        });

        assert_eq!(fx, vec![Effect::Navigate(HOME.to_string())]);
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME.to_string()
            }
        );
    }

    // Rule 2 — Boot + ConfigUnavailable → Offline + ShowVideo.
    #[test]
    fn boot_config_unavailable_shows_video_and_goes_offline() {
        let mut m = Machine::new(cfg(Fallback::Video));

        let fx = m.on(Event::ConfigUnavailable);

        assert_eq!(fx, vec![Effect::ShowVideo]);
        assert_eq!(m.state(), &AppState::Offline);
    }

    // Rule 3 — Online + LinkChanged(Offline) → Offline + ShowVideo.
    #[test]
    fn online_link_offline_shows_video_and_goes_offline() {
        let mut m = online(Fallback::Video);

        let fx = m.on(Event::LinkChanged(Link::Offline));

        assert_eq!(fx, vec![Effect::ShowVideo]);
        assert_eq!(m.state(), &AppState::Offline);
    }

    // Rule 4 — Offline + LinkChanged(Online) → re-navigate the remembered home. This is the
    // anti-stuck-kiosk test: a machine that reaches Offline must be able to navigate back.
    #[test]
    fn offline_link_online_renavigates_remembered_home() {
        let mut m = online(Fallback::Video);
        m.on(Event::LinkChanged(Link::Offline));
        assert_eq!(m.state(), &AppState::Offline, "premise: offline");

        let fx = m.on(Event::LinkChanged(Link::Online));

        assert_eq!(
            fx,
            vec![Effect::Navigate(HOME.to_string())],
            "reconnect MUST re-navigate the remembered home -- a machine that reaches \
             Offline and cannot navigate back is a stuck kiosk"
        );
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME.to_string()
            }
        );
    }

    // Rule 5 — Online + NavigationFailed with fallback=video → Offline + ShowVideo.
    // (Attack target: a table that ignores `fallback` and routes to ErrorPage makes this RED.)
    #[test]
    fn online_nav_failed_video_fallback_shows_video_and_goes_offline() {
        let mut m = online(Fallback::Video);

        let fx = m.on(Event::NavigationFailed);

        assert_eq!(
            fx,
            vec![Effect::ShowVideo],
            "fallback=video: a site failure falls to the offline video"
        );
        assert_eq!(m.state(), &AppState::Offline);
    }

    // Rule 5 companion — with fallback=error_page the Video branch must NOT be taken. Pins
    // that the transition genuinely consults `fallback`. Task 1 no-ops the error_page case
    // (Task 2 routes it to ErrorPage); this assertion holds under both, and goes RED against
    // a table that ignores `fallback` and always shows the video.
    #[test]
    fn online_nav_failed_errorpage_fallback_does_not_show_video() {
        let mut m = online(Fallback::ErrorPage);

        let fx = m.on(Event::NavigationFailed);

        assert!(
            !fx.contains(&Effect::ShowVideo),
            "fallback=error_page must NOT fall to the offline video -- taking the Video \
             branch here means the table ignored `fallback`"
        );
        assert_ne!(
            m.state(),
            &AppState::Offline,
            "fallback=error_page must not land on Offline via the video branch"
        );
    }

    // Rule 8 — a LinkChanged to the link we are already in is a no-op (damping is the
    // prober's job; the FSM adds no hysteresis). Two LinkChanged(Offline) must not double-fire.
    #[test]
    fn repeated_link_offline_is_a_noop_from_offline() {
        let mut m = online(Fallback::Video);
        let first = m.on(Event::LinkChanged(Link::Offline));
        assert_eq!(first, vec![Effect::ShowVideo], "the real flip fires once");
        assert_eq!(m.state(), &AppState::Offline);

        let second = m.on(Event::LinkChanged(Link::Offline));

        assert!(
            second.is_empty(),
            "a LinkChanged(Offline) while already Offline must NOT re-fire an effect -- the \
             FSM adds no hysteresis of its own"
        );
        assert_eq!(m.state(), &AppState::Offline);
    }

    // Rule 8 (mirror) — LinkChanged(Online) while already Online is a no-op.
    #[test]
    fn repeated_link_online_is_a_noop_from_online() {
        let mut m = online(Fallback::Video);

        let fx = m.on(Event::LinkChanged(Link::Online));

        assert!(
            fx.is_empty(),
            "a LinkChanged(Online) while already Online must be a no-op"
        );
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME.to_string()
            }
        );
    }

    // Design-critical recovery: the no-config boot path (rule 2 → Offline with home=None)
    // must be recoverable. Without an Offline+ConfigApplied arm the kiosk is stuck on the
    // offline video forever — the exact failure the "carry the url" decision guards against.
    #[test]
    fn offline_without_config_recovers_when_config_arrives() {
        let mut m = Machine::new(cfg(Fallback::Video));
        m.on(Event::ConfigUnavailable);
        assert_eq!(
            m.state(),
            &AppState::Offline,
            "premise: offline, no home yet"
        );

        // Link returns before config: nothing to navigate to yet, must not crash or guess.
        let early = m.on(Event::LinkChanged(Link::Online));
        assert!(
            early.is_empty(),
            "no remembered home yet -> wait for config, do not navigate"
        );
        assert_eq!(m.state(), &AppState::Offline);

        // Config finally arrives -> online.
        let fx = m.on(Event::ConfigApplied {
            url: HOME.to_string(),
        });
        assert_eq!(fx, vec![Effect::Navigate(HOME.to_string())]);
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME.to_string()
            }
        );
    }

    // `home` is refreshed by a later ConfigApplied, so reconnect navigates the NEWEST home,
    // not the first one ever seen.
    #[test]
    fn reconnect_uses_the_latest_applied_home() {
        let mut m = online(Fallback::Video); // home = HOME
        m.on(Event::LinkChanged(Link::Offline)); // -> Offline

        let fx = m.on(Event::ConfigApplied {
            url: HOME2.to_string(),
        }); // home = HOME2, online
        assert_eq!(fx, vec![Effect::Navigate(HOME2.to_string())]);

        m.on(Event::LinkChanged(Link::Offline)); // -> Offline
        let back = m.on(Event::LinkChanged(Link::Online));

        assert_eq!(
            back,
            vec![Effect::Navigate(HOME2.to_string())],
            "reconnect must navigate the MOST RECENT home, not the original"
        );
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME2.to_string()
            }
        );
    }

    // RT-04 / OD-6 — reload-avoidance: a config re-applied with the SAME url while Online
    // must NOT re-navigate (that is the every-300s-poll case). Goes RED against a table that
    // re-navigates on every ConfigApplied.
    #[test]
    fn online_config_applied_same_url_is_a_noop() {
        let mut m = online(Fallback::Video); // Online { HOME }, home = HOME

        let fx = m.on(Event::ConfigApplied {
            url: HOME.to_string(),
        });

        assert!(
            fx.is_empty(),
            "the SAME url re-applied while Online must not reload the live page"
        );
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME.to_string()
            },
            "state unchanged when the url did not change"
        );
    }

    // RT-04 / OD-6 — the url-change gap: an applied config that CHANGES content.url while
    // Online must navigate to the new url and refresh home. Goes RED against HEAD's
    // always-ignore behaviour (deviation D2), which is the whole point of this fix.
    #[test]
    fn online_config_applied_changed_url_navigates() {
        let mut m = online(Fallback::Video); // Online { HOME }, home = HOME

        let fx = m.on(Event::ConfigApplied {
            url: HOME2.to_string(),
        });

        assert_eq!(
            fx,
            vec![Effect::Navigate(HOME2.to_string())],
            "a changed content.url while Online must navigate to it (RT-04)"
        );
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME2.to_string()
            }
        );
    }

    // RT-04 — the changed url must also refresh `home`, so a later reconnect re-navigates
    // the NEW url, never the stale one. Goes RED against an impl that ignored the Online
    // ConfigApplied (home would stay stale and reconnect would navigate the old url).
    #[test]
    fn changed_url_while_online_refreshes_home_for_reconnect() {
        let mut m = online(Fallback::Video); // Online { HOME }, home = HOME
        m.on(Event::ConfigApplied {
            url: HOME2.to_string(),
        }); // url changed -> navigate HOME2, home = HOME2
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME2.to_string()
            },
            "premise: navigated to the changed url"
        );

        m.on(Event::LinkChanged(Link::Offline)); // -> Offline
        let back = m.on(Event::LinkChanged(Link::Online));

        assert_eq!(
            back,
            vec![Effect::Navigate(HOME2.to_string())],
            "reconnect must use the url from the LAST applied config, not the stale original"
        );
    }

    // ---- Task 2: the ErrorPage retry sub-machine (rules 6, 7) ----

    // Rule 6 (entry) — Online + NavigationFailed with fallback=error_page → ErrorPage{1} +
    // ShowErrorPage. `attempts` starts at 1 because the triggering failure IS failed-load #1
    // (see `AppState::ErrorPage`'s doc). The carried retry length is pinned to the configured
    // value (not just the effect variant), which kills a hardcoded number.
    #[test]
    fn online_nav_failed_errorpage_fallback_enters_errorpage() {
        let mut m = online(Fallback::ErrorPage);

        let fx = m.on(Event::NavigationFailed);

        assert_eq!(
            fx,
            vec![Effect::ShowErrorPage {
                retry_after_seconds: DEFAULT_ERROR_RETRY_SECONDS
            }],
            "fallback=error_page: a site failure shows the bundled error page, armed with the \
             configured retry length"
        );
        assert_eq!(
            m.state(),
            &AppState::ErrorPage { attempts: 1 },
            "the triggering failure is failed-load #1, so attempts starts at 1"
        );
    }

    // Rule 7 (retry OK) — entry → CountdownExpired re-navigates the remembered home and STAYS
    // in ErrorPage with attempts unchanged (the retry is in flight) → NavigationCommitted
    // lands Online with NO further effect. The webview already navigated on CountdownExpired,
    // so a second Navigate here would loop it (see the module docs' retry-as-effect model).
    #[test]
    fn errorpage_retry_commit_returns_online_without_renavigating() {
        let mut m = online(Fallback::ErrorPage);
        m.on(Event::NavigationFailed); // -> ErrorPage{1}

        let retry = m.on(Event::CountdownExpired);
        assert_eq!(
            retry,
            vec![Effect::Navigate(HOME.to_string())],
            "the countdown fires the retry navigation to the remembered home"
        );
        assert_eq!(
            m.state(),
            &AppState::ErrorPage { attempts: 1 },
            "the retry is in flight: stay in ErrorPage with attempts unchanged until the result"
        );

        let committed = m.on(Event::NavigationCommitted);
        assert!(
            committed.is_empty(),
            "the retry already navigated on CountdownExpired; committing must NOT emit another \
             Navigate (that would loop the webview)"
        );
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME.to_string()
            },
            "a committed retry returns to Online on the remembered home"
        );
    }

    // Rule 7 (retry fails, below the cap) — entry → CountdownExpired → NavigationFailed re-arms
    // the error page as ErrorPage{2} + ShowErrorPage (attempts incremented, countdown re-armed).
    #[test]
    fn errorpage_retry_failure_rearms_and_increments() {
        let mut m = online(Fallback::ErrorPage);
        m.on(Event::NavigationFailed); // -> ErrorPage{1}
        m.on(Event::CountdownExpired); // retry navigation in flight

        let fx = m.on(Event::NavigationFailed);

        assert_eq!(
            fx,
            vec![Effect::ShowErrorPage {
                retry_after_seconds: DEFAULT_ERROR_RETRY_SECONDS
            }],
            "a failed retry below the cap re-arms the error-page countdown"
        );
        assert_eq!(
            m.state(),
            &AppState::ErrorPage { attempts: 2 },
            "a failed retry increments the consecutive-failure count"
        );
    }

    // Rule 7 (THE boundary that matters) — with error_max_retries = 5, the first FOUR failed
    // loads keep retrying on the error page (ErrorPage{1..=4}) and the FIFTH gives up to
    // Offline + ShowVideo. This one test pins BOTH load-bearing facts:
    //   * the comparison is `>=`, not `>`: a `>` mutation keeps the 5th on ErrorPage{5} and
    //     only gives up on a 6th failure (a kiosk that retries one time too many) → the
    //     Offline/ShowVideo assertions go RED;
    //   * the entry value is 1, not 0: an entry of 0 shifts the whole ladder down one, so the
    //     4th failed load would read ErrorPage{3} and the 5th would still be ErrorPage{4} →
    //     the ErrorPage{4}/Offline assertions go RED.
    #[test]
    fn errorpage_gives_up_to_offline_on_the_fifth_failed_load() {
        let mut m = online(Fallback::ErrorPage);

        // Failed load #1 (the entry): Online + NavigationFailed -> ErrorPage{1}.
        m.on(Event::NavigationFailed);
        assert_eq!(
            m.state(),
            &AppState::ErrorPage { attempts: 1 },
            "entry lands ErrorPage{{1}} (kills entry==0)"
        );

        // Failed loads #2, #3, #4: each retry (CountdownExpired) then fails, re-arming the page.
        for expected in 2..=4 {
            m.on(Event::CountdownExpired); // retry navigation in flight
            let fx = m.on(Event::NavigationFailed);
            assert_eq!(
                fx,
                vec![Effect::ShowErrorPage {
                    retry_after_seconds: DEFAULT_ERROR_RETRY_SECONDS
                }],
                "a failed load below the cap re-arms the error page"
            );
            assert_eq!(
                m.state(),
                &AppState::ErrorPage { attempts: expected },
                "still retrying below the cap"
            );
        }
        // After the 4th failed load we are on ErrorPage{4}, NOT Offline.
        assert_eq!(
            m.state(),
            &AppState::ErrorPage { attempts: 4 },
            "the 4th failed load must still be retrying (ErrorPage{{4}}), not given up"
        );

        // Failed load #5: the retry fails again -> n = 5, 5 >= 5 -> give up to the offline video.
        m.on(Event::CountdownExpired); // 5th retry navigation in flight
        let fx = m.on(Event::NavigationFailed);
        assert_eq!(
            fx,
            vec![Effect::ShowVideo],
            "the 5th failed load hits error_max_retries and falls to the offline video"
        );
        assert_eq!(
            m.state(),
            &AppState::Offline,
            "at error_max_retries the kiosk gives up to Offline"
        );
    }

    // Rule 7 (prober flips offline) — a link drop while on the error page goes straight to the
    // offline video, regardless of how many retries remain (spec §3.3: immediate). Mirrors the
    // rule-3 Online arm.
    #[test]
    fn errorpage_link_offline_shows_video_immediately() {
        let mut m = online(Fallback::ErrorPage);
        m.on(Event::NavigationFailed); // -> ErrorPage{1}
        assert_eq!(
            m.state(),
            &AppState::ErrorPage { attempts: 1 },
            "premise: on the error page"
        );

        let fx = m.on(Event::LinkChanged(Link::Offline));

        assert_eq!(
            fx,
            vec![Effect::ShowVideo],
            "a link drop on the error page falls to the offline video immediately"
        );
        assert_eq!(m.state(), &AppState::Offline);
    }

    // ---- Task 3: idle-reset gating + reconnect (rules 9, 10) ----

    // Rule 9 (THE PRIVACY GATE, spec §3.5) — Online + IdleExpired with clear_data_on_reset=true
    // must ONLY emit ClearProfile{full:true} and gate re-display behind Clearing{Online{HOME}}.
    // It must NOT emit any Navigate here: navigating home before the clear completes would
    // display the fresh session over un-cleared data — the exact leak §3.5 exists to prevent.
    #[test]
    fn idle_reset_with_clear_gates_display_behind_clearprofile() {
        let mut m = online(Fallback::Video); // idle_clear = true

        let fx = m.on(Event::IdleExpired);

        assert_eq!(
            fx,
            vec![Effect::ClearProfile { full: true }],
            "idle reset with clear must emit ONLY the full profile clear"
        );
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::Navigate(_))),
            "the privacy gate: NO Navigate may be emitted before the clear completes"
        );
        assert_eq!(
            m.state(),
            &AppState::Clearing {
                next: Box::new(AppState::Online {
                    url: HOME.to_string()
                })
            },
            "re-display is gated: the machine parks in Clearing until ProfileCleared"
        );
    }

    // Rule 9 (the gate HOLDS) — while Clearing, an event that would normally show content must
    // NOT re-display. Kills a mutation that lets a stray event navigate before the clear lands.
    #[test]
    fn clearing_does_not_redisplay_on_a_stray_event() {
        let mut m = online(Fallback::Video);
        m.on(Event::IdleExpired); // -> Clearing{Online{HOME}}
        let gated = AppState::Clearing {
            next: Box::new(AppState::Online {
                url: HOME.to_string(),
            }),
        };
        assert_eq!(m.state(), &gated, "premise: gated in Clearing");

        // A LinkChanged(Online) would normally show content; a second IdleExpired might re-fire.
        let a = m.on(Event::LinkChanged(Link::Online));
        assert!(
            !a.iter().any(|e| matches!(e, Effect::Navigate(_))),
            "no Navigate may escape the gate while Clearing"
        );
        assert_eq!(
            m.state(),
            &gated,
            "state stays Clearing under a stray LinkChanged"
        );

        let b = m.on(Event::IdleExpired);
        assert!(
            !b.iter().any(|e| matches!(e, Effect::Navigate(_))),
            "no Navigate may escape the gate on a repeated IdleExpired"
        );
        assert_eq!(
            m.state(),
            &gated,
            "state stays Clearing under a repeated IdleExpired"
        );
    }

    // Rule 9 (resume) — ProfileCleared releases the gate: NOW navigate home over the cleared
    // profile and return to Online.
    #[test]
    fn profile_cleared_resumes_gated_target_online() {
        let mut m = online(Fallback::Video);
        m.on(Event::IdleExpired); // -> Clearing{Online{HOME}}

        let fx = m.on(Event::ProfileCleared);

        assert_eq!(
            fx,
            vec![Effect::Navigate(HOME.to_string())],
            "the clear finished: NOW navigate home over the cleared profile"
        );
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME.to_string()
            },
            "resume into the gated target"
        );
    }

    // Rule 9 (no-clear path) — Online + IdleExpired with clear_data_on_reset=false reloads home
    // directly: Navigate + Online, with NO ClearProfile and without ever entering Clearing.
    #[test]
    fn idle_reset_without_clear_reloads_home_directly() {
        let mut m = Machine::new(cfg_no_clear(Fallback::Video));
        m.on(Event::ConfigApplied {
            url: HOME.to_string(),
        });
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME.to_string()
            },
            "premise: online"
        );

        let fx = m.on(Event::IdleExpired);

        assert_eq!(
            fx,
            vec![Effect::Navigate(HOME.to_string())],
            "no-clear idle reset reloads home fresh, directly"
        );
        assert!(
            !fx.iter().any(|e| matches!(e, Effect::ClearProfile { .. })),
            "no-clear path must NOT clear the profile"
        );
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME.to_string()
            },
            "no-clear idle reset never parks in Clearing"
        );
    }

    // Rule 9 (guard) — IdleExpired from a non-Online state is a no-op (no panic, no effect).
    #[test]
    fn idle_expired_from_non_online_is_a_noop() {
        // Fresh Boot.
        let mut boot = Machine::new(cfg(Fallback::Video));
        let fx = boot.on(Event::IdleExpired);
        assert!(fx.is_empty(), "IdleExpired from Boot is a no-op");
        assert_eq!(boot.state(), &AppState::Boot, "Boot unchanged");

        // Offline.
        let mut off = online(Fallback::Video);
        off.on(Event::LinkChanged(Link::Offline));
        assert_eq!(off.state(), &AppState::Offline, "premise: offline");
        let fx = off.on(Event::IdleExpired);
        assert!(fx.is_empty(), "IdleExpired from Offline is a no-op");
        assert_eq!(off.state(), &AppState::Offline, "Offline unchanged");
    }

    // Rule 10 (reconnect) — Offline + Reconnected kicks a config refetch and STAYS Offline; the
    // resulting ConfigApplied drives the nav via the existing Offline+ConfigApplied arm.
    #[test]
    fn offline_reconnected_refetches_config_and_stays_offline() {
        let mut m = online(Fallback::Video);
        m.on(Event::LinkChanged(Link::Offline));
        assert_eq!(m.state(), &AppState::Offline, "premise: offline");

        let fx = m.on(Event::Reconnected);

        assert_eq!(
            fx,
            vec![Effect::RefetchConfig],
            "reconnect kicks a config refetch"
        );
        assert_eq!(
            m.state(),
            &AppState::Offline,
            "the refetch is in flight: stay Offline until ConfigApplied lands"
        );
    }

    // Rule 10 (mirror) — Reconnected while not Offline is a no-op.
    #[test]
    fn reconnected_while_online_is_a_noop() {
        let mut m = online(Fallback::Video);

        let fx = m.on(Event::Reconnected);

        assert!(fx.is_empty(), "Reconnected while Online is a no-op");
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME.to_string()
            }
        );
    }

    // Rule 9 (mirror) — ProfileCleared while NOT in Clearing is a no-op: only the async clear
    // that entered Clearing can release the gate, so a stray ProfileCleared (e.g. delivered
    // late, after the machine already resumed) must not re-navigate or change state. Mirror of
    // reconnected_while_online_is_a_noop; pins the catch-all rather than a stray Clearing arm.
    #[test]
    fn profile_cleared_outside_clearing_is_a_noop() {
        let mut m = online(Fallback::Video);

        let fx = m.on(Event::ProfileCleared);

        assert!(
            fx.is_empty(),
            "ProfileCleared while not Clearing must emit nothing"
        );
        assert_eq!(
            m.state(),
            &AppState::Online {
                url: HOME.to_string()
            },
            "ProfileCleared while not Clearing must not change state"
        );
    }
}
