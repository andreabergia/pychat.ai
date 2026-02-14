use clap::Parser;

#[derive(Debug, Parser, Clone, PartialEq, Eq)]
#[command(name = "pyaichat")]
#[command(about = "Minimal Python REPL with a conversational assistant")]
pub struct CliArgs {
    /// Enable verbose HTTP request/response debug logs.
    #[arg(short, long)]
    pub verbose: bool,
}

#[cfg(test)]
mod tests {
    use super::CliArgs;
    use clap::Parser;

    #[test]
    fn parse_verbose_short_flag() {
        let args = CliArgs::try_parse_from(["pyaichat", "-v"]).expect("should parse");
        assert!(args.verbose);
    }

    #[test]
    fn parse_verbose_long_flag() {
        let args = CliArgs::try_parse_from(["pyaichat", "--verbose"]).expect("should parse");
        assert!(args.verbose);
    }

    #[test]
    fn parse_defaults_to_not_verbose() {
        let args = CliArgs::try_parse_from(["pyaichat"]).expect("should parse");
        assert!(!args.verbose);
    }
}
