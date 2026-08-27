use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};

pub struct Spool;

impl Spool {
    pub fn events(data_dir: impl AsRef<Path>) -> io::Result<Vec<Value>> {
        events(data_dir)
    }

    pub fn count(data_dir: impl AsRef<Path>, event: &str) -> io::Result<usize> {
        count_events(data_dir, event)
    }
}

pub fn events(data_dir: impl AsRef<Path>) -> io::Result<Vec<Value>> {
    let mut files = Vec::new();
    collect_jsonl(&data_dir.as_ref().join("spool"), &mut files)?;
    files.sort();

    let mut result = Vec::new();
    for file in files {
        for (line_no, line) in std::fs::read_to_string(&file)?.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let value = serde_json::from_str(line).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}:{}: {error}", file.display(), line_no + 1),
                )
            })?;
            result.push(value);
        }
    }
    Ok(result)
}

pub fn count_events(data_dir: impl AsRef<Path>, event: &str) -> io::Result<usize> {
    Ok(events(data_dir)?
        .iter()
        .filter(|value| value.get("event").and_then(Value::as_str) == Some(event))
        .count())
}

fn collect_jsonl(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect_jsonl(&path, files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("kiosk-smoke-spool-{suffix}"));
        std::fs::create_dir_all(path.join("spool/main")).unwrap();
        path
    }

    #[test]
    fn the_spool_oracle_counts_events_by_name() {
        let dir = temp_dir();
        std::fs::write(
            dir.join("spool/main/00001.jsonl"),
            "{\"event\":\"nav.blocked\"}\n{\"event\":\"nav.committed\"}\n",
        )
        .unwrap();
        assert_eq!(count_events(&dir, "nav.blocked").unwrap(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
