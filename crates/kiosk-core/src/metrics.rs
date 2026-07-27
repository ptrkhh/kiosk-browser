use serde_json::{Map, Value};
use std::path::Path;
use std::time::Instant;
use sysinfo::{Disks, System};

pub struct HealthSample {
    pub cpu_percent: f32,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub disk_free_mb: u64,
    pub uptime_secs: u64,
}

const MB: u64 = 1_048_576;

/// Sample host health. `sys`/`disks` are held across ticks by the caller so CPU % is a real
/// delta between refreshes (the first sample right after boot reads ~0 — acceptable for a
/// 60 s heartbeat). ponytail: no persistent averaging; the raw instantaneous reading is
/// enough signal for fleet dashboards.
pub fn sample(
    sys: &mut System,
    disks: &mut Disks,
    data_dir: &Path,
    started: Instant,
) -> HealthSample {
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    disks.refresh();
    // Free space on the disk whose mount point is the longest prefix of data_dir.
    let disk_free = disks
        .list()
        .iter()
        .filter(|d| data_dir.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
        .unwrap_or(0);
    HealthSample {
        cpu_percent: sys.global_cpu_usage(),
        mem_used_mb: sys.used_memory() / MB,
        mem_total_mb: sys.total_memory() / MB,
        disk_free_mb: disk_free / MB,
        uptime_secs: started.elapsed().as_secs(),
    }
}

/// The enumerated `health.sample` jsonPayload (spec §6 — no free-form content).
pub fn to_fields(s: &HealthSample, dropped_expired: u64) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("cpu_percent".into(), Value::from(s.cpu_percent));
    m.insert("mem_used_mb".into(), Value::from(s.mem_used_mb));
    m.insert("mem_total_mb".into(), Value::from(s.mem_total_mb));
    m.insert("disk_free_mb".into(), Value::from(s.disk_free_mb));
    m.insert("uptime_secs".into(), Value::from(s.uptime_secs));
    m.insert("spool_dropped_expired".into(), Value::from(dropped_expired));
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn sample_reports_plausible_memory_and_disk() {
        let mut sys = sysinfo::System::new();
        let mut disks = sysinfo::Disks::new_with_refreshed_list();
        let s = sample(
            &mut sys,
            &mut disks,
            std::path::Path::new("."),
            Instant::now(),
        );
        assert!(s.mem_total_mb > 0, "total memory must be readable");
        assert!(s.mem_used_mb <= s.mem_total_mb, "used <= total");
        // disk_free_mb may be 0 on an exotic mount, but the field must be present (no panic).
    }

    #[test]
    fn to_fields_has_the_enumerated_keys_plus_dropped_expired() {
        let s = HealthSample {
            cpu_percent: 1.0,
            mem_used_mb: 100,
            mem_total_mb: 200,
            disk_free_mb: 50,
            uptime_secs: 10,
        };
        let f = to_fields(&s, 7);
        for k in [
            "cpu_percent",
            "mem_used_mb",
            "mem_total_mb",
            "disk_free_mb",
            "uptime_secs",
            "spool_dropped_expired",
        ] {
            assert!(f.contains_key(k), "missing {k}");
        }
        assert_eq!(f["spool_dropped_expired"], serde_json::json!(7));
    }
}
