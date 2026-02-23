use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser, Clone, PartialEq, Eq)]
#[command(name = "pychat.ai")]
#[command(
    about = "Minimal Python REPL with a conversational assistant",
    long_about = "Minimal Python REPL with a conversational assistant\n\nConfig file loading:\n  - --config <path> (explicit file, overrides default path discovery)\n  - Default probe path when --config is not provided:\n    1. $XDG_CONFIG_HOME/pychat.ai/config.toml\n    2. ~/.config/pychat.ai/config.toml"
)]
pub struct CliArgs {
    /// Load config from this file path instead of the default discovery path.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Initialize embedded Python and exit without starting the REPL.
    #[arg(long)]
    pub smoke_python: bool,
}

#[cfg(test)]
mod tests {
    use super::CliArgs;
    use clap::Parser;

    #[test]
    fn parse_defaults() {
        let args = CliArgs::try_parse_from(["pychat.ai"]).expect("should parse");
        assert_eq!(args.config, None);
        assert!(!args.smoke_python);
    }

    #[test]
    fn parse_config_flag() {
        let args =
            CliArgs::try_parse_from(["pychat.ai", "--config", "/tmp/custom.toml"]).expect("parse");
        assert_eq!(
            args.config.as_deref(),
            Some(std::path::Path::new("/tmp/custom.toml"))
        );
        assert!(!args.smoke_python);
    }

    #[test]
    fn parse_smoke_python_flag() {
        let args = CliArgs::try_parse_from(["pychat.ai", "--smoke-python"]).expect("parse");
        assert!(args.smoke_python);
        assert_eq!(args.config, None);
    }

    #[test]
    fn parse_config_and_smoke_python_flag() {
        let args = CliArgs::try_parse_from([
            "pychat.ai",
            "--config",
            "/tmp/custom.toml",
            "--smoke-python",
        ])
        .expect("parse");
        assert!(args.smoke_python);
        assert_eq!(
            args.config.as_deref(),
            Some(std::path::Path::new("/tmp/custom.toml"))
        );
    }
}
