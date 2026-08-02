//! kiosk-launcher as a library.
//!
//! The supervisor is a binary (`main.rs` is still the only production entry
//! point and still owns the assembly). This lib target exists for ONE reason:
//! RT-13, the headless integration test, has to drive the REAL `loop_`, `pipe`,
//! `spawn` and `sink` against a real child process, and a Rust integration test
//! can only link a lib target. Nothing here is a new abstraction — it is the
//! same modules `main.rs` previously declared with `mod`.

mod clock;
pub mod job;
pub mod loop_;
pub mod pipe;
pub mod sink;
mod spawn;
pub mod timer;
