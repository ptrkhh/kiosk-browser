//! Linux functional-gate primitives.
//!
//! Scenario tests are ignored by default because they need a compositor and
//! release artifacts. The helpers are deliberately stdlib-heavy: the smoke
//! gate must not pull the kiosk-main dependency graph into its test binary.

pub mod compositor;
pub mod httpd;
pub mod preflight;
pub mod spool;

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binaries {
    pub kiosk_bin: PathBuf,
    pub kioskctl_bin: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvError {
    Missing(&'static str),
    Empty(&'static str),
    NotExecutable(PathBuf),
    CommandFailed { program: PathBuf, code: Option<i32> },
    InvalidOutput(String),
    Io(String),
}

impl From<std::io::Error> for EnvError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

pub fn binaries_from_env() -> Result<Binaries, EnvError> {
    binaries_from(|key| std::env::var_os(key).map(PathBuf::from))
}

fn binaries_from<F>(mut get: F) -> Result<Binaries, EnvError>
where
    F: FnMut(&'static str) -> Option<PathBuf>,
{
    fn required<F>(key: &'static str, get: &mut F) -> Result<PathBuf, EnvError>
    where
        F: FnMut(&'static str) -> Option<PathBuf>,
    {
        let Some(path) = get(key) else {
            return Err(EnvError::Missing(key));
        };
        if path.as_os_str().is_empty() {
            return Err(EnvError::Empty(key));
        }
        Ok(path)
    }

    Ok(Binaries {
        kiosk_bin: required("KIOSK_BIN", &mut get)?,
        kioskctl_bin: required("KIOSKCTL_BIN", &mut get)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_kiosk_bin_is_an_environment_error_not_a_scenario_failure() {
        let result =
            binaries_from(|key| (key != "KIOSK_BIN").then(|| PathBuf::from("/tmp/kioskctl")));
        assert_eq!(result, Err(EnvError::Missing("KIOSK_BIN")));
    }
}
