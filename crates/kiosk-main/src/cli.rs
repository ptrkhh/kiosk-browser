//! Minimal hand-rolled CLI (YAGNI: no clap). `kiosk.ini` (spec §5.1) is now the real
//! configuration source; `--windowed` remains a dev/diagnostic flag and `--config`
//! overrides the install dir it (and the credential/mp4 next to it) is read from
//! (spec §4 "File & directory conventions": "next to binaries (override: --config
//! <path>)").

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub windowed: bool,
    /// Overrides the install dir `kiosk.ini`/the credential file/the offline mp4 are
    /// read from (default: the running exe's own directory). Spec §4's `--config
    /// <path>` names a DIRECTORY, not the ini file itself — `kiosk.ini` inside it.
    pub config: Option<String>,
}

impl Args {
    pub fn parse(mut items: impl Iterator<Item = String>) -> Args {
        let mut args = Args::default();
        let _argv0 = items.next();
        while let Some(item) = items.next() {
            match item.as_str() {
                "--windowed" => args.windowed = true,
                "--config" => args.config = items.next(),
                "--version" => {
                    println!("kiosk-main {}", kiosk_core::app_version());
                    std::process::exit(0);
                }
                other => eprintln!("kiosk-main: ignoring unknown argument {other:?}"),
            }
        }
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(items: &[&str]) -> Args {
        Args::parse(items.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_all_flags() {
        let a = parse(&["kiosk-main", "--config", "D:\\kiosk", "--windowed"]);
        assert_eq!(
            a,
            Args {
                windowed: true,
                config: Some("D:\\kiosk".into()),
            }
        );
    }

    #[test]
    fn defaults_are_off() {
        assert_eq!(parse(&["kiosk-main"]), Args::default());
    }

    #[test]
    fn config_without_value_is_ignored() {
        assert_eq!(parse(&["kiosk-main", "--config"]), Args::default());
    }
}
