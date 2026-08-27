/// Number of consecutive over-cap health samples required before restarting.
pub const MEM_CAP_N: u32 = 5;

/// Hysteresis-free consecutive-sample latch for the webview memory cap.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MemCap {
    over: u32,
}

impl MemCap {
    /// Return `true` once per run of `MEM_CAP_N` samples strictly over `cap_mb`.
    /// A zero cap disables enforcement and an at-or-below sample resets the run.
    pub fn observe(&mut self, rss_mb: u64, cap_mb: u64) -> bool {
        if cap_mb == 0 || rss_mb <= cap_mb {
            self.over = 0;
            return false;
        }

        self.over = self.over.saturating_add(1);
        if self.over >= MEM_CAP_N {
            self.over = 0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MemCap, MEM_CAP_N};

    #[test]
    fn a_zero_cap_never_trips() {
        let mut cap = MemCap::default();
        for _ in 0..100 {
            assert!(!cap.observe(9999, 0));
        }
    }

    #[test]
    fn the_latch_trips_once_after_n_consecutive_over_samples_then_resets() {
        let mut cap = MemCap::default();
        for _ in 0..(MEM_CAP_N - 1) {
            assert!(!cap.observe(300, 256));
        }
        assert!(cap.observe(300, 256));
        assert!(!cap.observe(300, 256), "trips once, then resets");
    }

    #[test]
    fn a_sample_at_or_below_the_cap_resets_the_run() {
        let mut cap = MemCap::default();
        for _ in 0..(MEM_CAP_N - 1) {
            assert!(!cap.observe(300, 256));
        }
        assert!(!cap.observe(256, 256));
        for _ in 0..(MEM_CAP_N - 1) {
            assert!(!cap.observe(300, 256));
        }
    }
}
