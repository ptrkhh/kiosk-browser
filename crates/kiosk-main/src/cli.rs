//! Minimal hand-rolled CLI (YAGNI: no clap). Replaced by kiosk.ini in the
//! config plan; --windowed and --spike-input remain dev/diagnostic flags.

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub url: Option<String>,
    pub windowed: bool,
    pub spike_input: bool,
}

impl Args {
    pub fn parse(mut items: impl Iterator<Item = String>) -> Args {
        let mut args = Args::default();
        let _argv0 = items.next();
        while let Some(item) = items.next() {
            match item.as_str() {
                "--url" => args.url = items.next(),
                "--windowed" => args.windowed = true,
                "--spike-input" => args.spike_input = true,
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
        let a = parse(&[
            "kiosk-main",
            "--url",
            "https://x.test/a",
            "--windowed",
            "--spike-input",
        ]);
        assert_eq!(
            a,
            Args {
                url: Some("https://x.test/a".into()),
                windowed: true,
                spike_input: true
            }
        );
    }

    #[test]
    fn defaults_are_off() {
        assert_eq!(parse(&["kiosk-main"]), Args::default());
    }

    #[test]
    fn url_without_value_is_ignored() {
        assert_eq!(parse(&["kiosk-main", "--url"]), Args::default());
    }
}
