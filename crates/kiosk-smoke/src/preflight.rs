use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{binaries_from_env, Binaries, EnvError};

pub fn preflight_from_env() -> Result<Binaries, EnvError> {
    let binaries = binaries_from_env()?;
    preflight(&binaries.kiosk_bin, &binaries.kioskctl_bin)?;
    Ok(binaries)
}

/// Validate the release artifacts before any scenario runs. A broken key/tool
/// setup is a runner failure, not a kiosk scenario failure.
pub fn preflight(kiosk_bin: &Path, kioskctl_bin: &Path) -> Result<(), EnvError> {
    for path in [kiosk_bin, kioskctl_bin] {
        if !is_executable(path) {
            return Err(EnvError::NotExecutable(path.to_path_buf()));
        }
    }

    let status = Command::new(kioskctl_bin).arg("selftest").status()?;
    if !status.success() {
        return Err(EnvError::CommandFailed {
            program: kioskctl_bin.to_path_buf(),
            code: status.code(),
        });
    }

    let Some(seed) = std::env::var_os("KIOSK_SIGNING_KEY_B64") else {
        return Err(EnvError::Missing("KIOSK_SIGNING_KEY_B64"));
    };
    if seed.is_empty() {
        return Err(EnvError::Empty("KIOSK_SIGNING_KEY_B64"));
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| EnvError::Io(error.to_string()))?
        .as_nanos();
    let input = std::env::temp_dir().join(format!("kiosk-smoke-preflight-{nonce}.json"));
    std::fs::write(
        &input,
        r#"{"revision":1,"device_id":"smoke-preflight","content":{"url":"https://app.example.com/"}}"#,
    )?;
    let signed = Command::new(kioskctl_bin)
        .args(["sign", input.to_string_lossy().as_ref()])
        .output()?;
    let _ = std::fs::remove_file(&input);
    if !signed.status.success() {
        return Err(EnvError::CommandFailed {
            program: kioskctl_bin.to_path_buf(),
            code: signed.status.code(),
        });
    }
    let value: serde_json::Value = serde_json::from_slice(&signed.stdout)
        .map_err(|error| EnvError::InvalidOutput(format!("kioskctl sign output: {error}")))?;
    if value
        .get("sig")
        .and_then(serde_json::Value::as_str)
        .is_none()
    {
        return Err(EnvError::InvalidOutput(
            "kioskctl sign output has no signature".into(),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && std::fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_binary_is_reported_before_running_a_scenario() {
        let result = preflight(
            Path::new("/definitely/missing/kiosk-main"),
            Path::new("/also/missing"),
        );
        assert!(matches!(result, Err(EnvError::NotExecutable(_))));
    }
}
