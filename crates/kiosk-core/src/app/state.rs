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
            // video. `fallback = error_page` (rule 6) is Task 2 and falls through to the
            // no-op below — so this arm genuinely branches on `fallback`.
            (Online { .. }, NavigationFailed) if self.cfg.fallback == Fallback::Video => {
                self.go_offline()
            }

            // Rule 8 + the Task 2/3 seam: a LinkChanged to the link we are already in is a
            // no-op (the prober already damped it), and every other unmatched pair is a
            // no-op for now.
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
}
