#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Help,
    Mode(Option<CommandMode>),
    Clear,
    History(Option<usize>),
    Trace,
    Inspect { expr: String },
    LastError,
    Include { path: String },
    ShowSource { name: String },
    Steps(Option<bool>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandMode {
    Python,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParseError {
    message: String,
}

impl ParseError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}

pub(crate) const HELP_TEXT: &str = "Available commands:\n  /help                Show this command list\n  /mode [py|ai]        Show or switch active mode\n  /clear               Clear the timeline output\n  /history [n]         Show command/input history (or last n)\n  /trace               Show path to the current trace file\n  /inspect <expr>      Inspect a Python expression as structured JSON\n  /last_error          Show the last Python exception traceback\n  /include <file.py>   Execute a Python file in the current session\n  /run <file>          Execute a file path exactly as provided\n  /show_source <name>  Show source code for a function/class/module name\n  /steps [on|off]      Show or hide assistant reasoning steps";

pub(crate) fn parse_command(line: &str) -> Result<Command, ParseError> {
    if !line.starts_with('/') {
        return Err(ParseError::new("not a command"));
    }

    let trimmed = line.trim();
    if trimmed == "/" {
        return Err(ParseError::new("empty command. Try /help"));
    }

    let command_text = &trimmed[1..];
    let mut parts = command_text.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("").to_ascii_lowercase();
    if name.is_empty() {
        return Err(ParseError::new("empty command. Try /help"));
    }
    let rest = parts.next().map(str::trim).unwrap_or("");

    match name.as_str() {
        "help" => expect_no_args(rest, Command::Help, "usage: /help"),
        "mode" => parse_mode(rest),
        "clear" => expect_no_args(rest, Command::Clear, "usage: /clear"),
        "history" => parse_history(rest),
        "trace" => expect_no_args(rest, Command::Trace, "usage: /trace"),
        "inspect" => parse_required_text_arg(rest, "usage: /inspect <expr>")
            .map(|expr| Command::Inspect { expr }),
        "last_error" => expect_no_args(rest, Command::LastError, "usage: /last_error"),
        "include" => parse_include(rest),
        "run" => parse_run(rest),
        "show_source" => parse_required_text_arg(rest, "usage: /show_source <name>")
            .map(|name| Command::ShowSource { name }),
        "steps" => parse_steps(rest),
        _ => Err(ParseError::new(format!(
            "unknown command '/{name}'. Try /help"
        ))),
    }
}

pub(crate) fn is_command_line(line: &str) -> bool {
    line.starts_with('/')
}

fn expect_no_args(rest: &str, command: Command, usage: &str) -> Result<Command, ParseError> {
    if rest.is_empty() {
        Ok(command)
    } else {
        Err(ParseError::new(usage))
    }
}

fn parse_mode(rest: &str) -> Result<Command, ParseError> {
    if rest.is_empty() {
        return Ok(Command::Mode(None));
    }

    match rest {
        "py" => Ok(Command::Mode(Some(CommandMode::Python))),
        "ai" => Ok(Command::Mode(Some(CommandMode::Assistant))),
        _ => Err(ParseError::new("usage: /mode [py|ai]")),
    }
}

fn parse_history(rest: &str) -> Result<Command, ParseError> {
    if rest.is_empty() {
        return Ok(Command::History(None));
    }

    let value = rest
        .parse::<usize>()
        .map_err(|_| ParseError::new("usage: /history [n]"))?;
    if value == 0 {
        return Err(ParseError::new("usage: /history [n] (n must be >= 1)"));
    }

    Ok(Command::History(Some(value)))
}

fn parse_include(rest: &str) -> Result<Command, ParseError> {
    if rest.is_empty() {
        return Err(ParseError::new(
            "missing file argument. usage: /include <file.py>",
        ));
    }

    let path = rest.to_string();
    if !path.ends_with(".py") {
        return Err(ParseError::new("usage: /include <file.py>"));
    }
    Ok(Command::Include { path })
}

fn parse_run(rest: &str) -> Result<Command, ParseError> {
    if rest.is_empty() {
        return Err(ParseError::new("missing file argument. usage: /run <file>"));
    }

    Ok(Command::Include {
        path: rest.to_string(),
    })
}

fn parse_steps(rest: &str) -> Result<Command, ParseError> {
    if rest.is_empty() {
        return Ok(Command::Steps(None));
    }

    match rest {
        "on" => Ok(Command::Steps(Some(true))),
        "off" => Ok(Command::Steps(Some(false))),
        _ => Err(ParseError::new("usage: /steps [on|off]")),
    }
}

fn parse_required_text_arg(rest: &str, usage: &str) -> Result<String, ParseError> {
    if rest.is_empty() {
        return Err(ParseError::new(usage));
    }
    Ok(rest.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Command, CommandMode, HELP_TEXT, is_command_line, parse_command};

    #[test]
    fn help_text_lists_all_supported_commands() {
        for needle in [
            "/help",
            "/mode [py|ai]",
            "/clear",
            "/history [n]",
            "/trace",
            "/inspect <expr>",
            "/last_error",
            "/include <file.py>",
            "/run <file>",
            "/show_source <name>",
            "/steps [on|off]",
        ] {
            assert!(HELP_TEXT.contains(needle), "missing help entry: {needle}");
        }
    }

    #[test]
    fn parse_simple_commands() {
        assert_eq!(parse_command("/help").expect("help"), Command::Help);
        assert_eq!(parse_command("/clear").expect("clear"), Command::Clear);
        assert_eq!(parse_command("/trace").expect("trace"), Command::Trace);
        assert_eq!(
            parse_command("/last_error").expect("last_error"),
            Command::LastError
        );
    }

    #[test]
    fn parse_mode_optional_argument() {
        assert_eq!(parse_command("/mode").expect("mode"), Command::Mode(None));
        assert_eq!(
            parse_command("/mode py").expect("mode py"),
            Command::Mode(Some(CommandMode::Python))
        );
        assert_eq!(
            parse_command("/mode ai").expect("mode ai"),
            Command::Mode(Some(CommandMode::Assistant))
        );
    }

    #[test]
    fn parse_history_optional_n() {
        assert_eq!(
            parse_command("/history").expect("history"),
            Command::History(None)
        );
        assert_eq!(
            parse_command("/history 12").expect("history 12"),
            Command::History(Some(12))
        );
    }

    #[test]
    fn parse_inspect_source_and_include_arguments() {
        assert_eq!(
            parse_command("/inspect x[0]").expect("inspect"),
            Command::Inspect {
                expr: "x[0]".to_string()
            }
        );
        assert_eq!(
            parse_command("/show_source my_fn").expect("show_source"),
            Command::ShowSource {
                name: "my_fn".to_string()
            }
        );
        assert_eq!(
            parse_command("/include script.py").expect("include"),
            Command::Include {
                path: "script.py".to_string()
            }
        );
        assert_eq!(
            parse_command("/run script.py").expect("run alias"),
            Command::Include {
                path: "script.py".to_string()
            }
        );
        assert_eq!(
            parse_command("/run script").expect("run alias without extension"),
            Command::Include {
                path: "script".to_string()
            }
        );
    }

    #[test]
    fn parse_steps_optional_state() {
        assert_eq!(
            parse_command("/steps").expect("steps"),
            Command::Steps(None)
        );
        assert_eq!(
            parse_command("/steps on").expect("steps on"),
            Command::Steps(Some(true))
        );
        assert_eq!(
            parse_command("/steps off").expect("steps off"),
            Command::Steps(Some(false))
        );
    }

    #[test]
    fn parse_reports_usage_for_invalid_arguments() {
        assert_eq!(
            parse_command("/mode bad")
                .expect_err("invalid mode")
                .message(),
            "usage: /mode [py|ai]"
        );
        assert_eq!(
            parse_command("/history 0")
                .expect_err("invalid history size")
                .message(),
            "usage: /history [n] (n must be >= 1)"
        );
        assert_eq!(
            parse_command("/include not_python.txt")
                .expect_err("invalid include path")
                .message(),
            "usage: /include <file.py>"
        );
        assert_eq!(
            parse_command("/include")
                .expect_err("missing include argument")
                .message(),
            "missing file argument. usage: /include <file.py>"
        );
        assert_eq!(
            parse_command("/run")
                .expect_err("missing run argument")
                .message(),
            "missing file argument. usage: /run <file>"
        );
        assert_eq!(
            parse_command("/steps maybe")
                .expect_err("invalid steps")
                .message(),
            "usage: /steps [on|off]"
        );
    }

    #[test]
    fn parse_reports_unknown_commands() {
        assert_eq!(
            parse_command("/bogus")
                .expect_err("unknown command")
                .message(),
            "unknown command '/bogus'. Try /help"
        );
    }

    #[test]
    fn parse_reports_empty_command_when_name_is_missing() {
        assert_eq!(
            parse_command("/ help")
                .expect_err("missing command name")
                .message(),
            "empty command. Try /help"
        );
    }

    #[test]
    fn command_line_detection_is_prefix_based() {
        assert!(is_command_line("/help"));
        assert!(is_command_line("/inspect x"));
        assert!(!is_command_line(" /help"));
        assert!(!is_command_line("print('/help')"));
    }
}
