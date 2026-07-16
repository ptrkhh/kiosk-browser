//! The app state machine (spec §3.3, §3.5): the pure decision core that drives what a
//! locked, unattended kiosk shows — `Boot → ConfigLoad → Online ⇄ Offline → ErrorPage`,
//! plus idle-reset gating — from events, emitting effects that the `kiosk-main` webview
//! layer (P1-D2) executes.
//!
//! # Layering (spec §4)
//!
//! No Tauri, no per-OS API, no timer, no HTTP, no webview. Exactly like the connectivity
//! prober ([`crate::net::prober`]), this is a pure Mealy machine —
//! `(state, event) → (state, Vec<Effect>)`. The idle timer, the navigation, the profile
//! clear, and the config fetch all live in `kiosk-main`; this module only *decides*.
//!
//! # Not persisted, by design
//!
//! A fresh process re-boots through [`state::AppState::Boot`], re-reads config from the
//! store (P1-A) and re-probes (P1-C); there is no prior visible-state to restore, so
//! nothing here is `Serialize`d or written to disk. This is the deliberate *opposite* of
//! the anti-rollback revision floor and the telemetry spool, which persist because they
//! must survive a crash — a stale "we were Online 90 s ago" says nothing about the link
//! *now*, so durability here would be state that outlives its own meaning.

pub mod state;
