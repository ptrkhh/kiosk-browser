//! Network reachability scope for the connectivity prober (spec §3.3, arch-13).
//!
//! The prober (P1-C Task 4 / P1-D) measures reachability of *`content.url`'s network
//! path*, not "the internet" in general — see [`reach`] for the heuristic and the
//! probe-URL resolution that back that decision.
//!
//! Layering (spec §4): no Tauri, no per-OS API. Everything under this module is pure
//! `std`/`url` logic — no sockets, no DNS resolution, no HTTP client. The actual probe
//! GET and its timer live in `kiosk-main`; this module only decides *which URL* to GET.

pub mod reach;
