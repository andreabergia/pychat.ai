use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser, Clone, PartialEq, Eq)]
#[command(name = "pyaichat")]
#[command(
    about = "Minimal Python REPL with a conversational assistant",
    long_about = "Minimal Python REPL with a conversational assistant\n\nConfig file loading:\n  - --config <path> (explicit file, overrides default path discovery)\n  - Default probe path when --config is not provided:\n    1. $XDG_CONFIG_HOME/pyaichat/config.toml\n    2. ~/.config/pyaichat/config.toml"
)]
pub struct CliArgs {
    /// Load config from this file path instead of the default discovery path.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::CliArgs;
    use clap::Parser;

    #[test]
    fn parse_defaults() {
        let args = CliArgs::try_parse_from(["pyaichat"]).expect("should parse");
        assert_eq!(args.config, None);
    }

    #[test]
    fn parse_config_flag() {
        let args =
            CliArgs::try_parse_from(["pyaichat", "--config", "/tmp/custom.toml"]).expect("parse");
        assert_eq!(
            args.config.as_deref(),
            Some(std::path::Path::new("/tmp/custom.toml"))
        );
    }
}
