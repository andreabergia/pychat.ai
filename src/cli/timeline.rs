use crate::cli::theme::Theme;
use crate::config::ThemeToken;
use crate::llm::provider::LlmTokenUsageTotals;
use ratatui::text::{Line, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputKind {
    UserInputPython,
    UserInputAssistant,
    PythonValue,
    PythonStdout,
    PythonStderr,
    PythonTraceback,
    AssistantText,
    AssistantWaiting,
    AssistantProgressRequest,
    AssistantProgressResult,
    SystemInfo,
    SystemError,
}

#[derive(Debug, Clone)]
pub(crate) enum TimelineEntry {
    UserInputPython(String),
    UserInputCommand(String),
    OutputLine { kind: OutputKind, text: String },
    AssistantTurn(AssistantTurn),
}

#[derive(Debug, Clone)]
pub(crate) struct AssistantTurn {
    pub(crate) prompt: String,
    pub(crate) events: Vec<AssistantStepEvent>,
    pub(crate) state: AssistantTurnState,
    pub(crate) token_usage: Option<LlmTokenUsageTotals>,
}

#[derive(Debug, Clone)]
pub(crate) enum AssistantTurnState {
    InFlight,
    CompletedText(String),
    CompletedError(String),
}

#[derive(Debug, Clone)]
pub(crate) enum AssistantStepEvent {
    ToolRequest { text: String },
    ToolResult { text: String },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Timeline {
    entries: Vec<TimelineEntry>,
}

impl Timeline {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push_output(&mut self, kind: OutputKind, text: &str) {
        for line in split_output_lines(text) {
            self.entries.push(TimelineEntry::OutputLine {
                kind,
                text: line.to_string(),
            });
        }
    }

    pub(crate) fn push_user_input_python(&mut self, text: &str) {
        for line in split_output_lines(text) {
            self.entries
                .push(TimelineEntry::UserInputPython(line.to_string()));
        }
    }

    pub(crate) fn push_user_input_command(&mut self, text: &str) {
        for line in split_output_lines(text) {
            self.entries
                .push(TimelineEntry::UserInputCommand(line.to_string()));
        }
    }

    pub(crate) fn push_assistant_turn(&mut self, prompt: String) -> usize {
        let index = self.entries.len();
        self.entries
            .push(TimelineEntry::AssistantTurn(AssistantTurn {
                prompt,
                events: Vec::new(),
                state: AssistantTurnState::InFlight,
                token_usage: None,
            }));
        index
    }

    pub(crate) fn assistant_turn_mut(&mut self, index: usize) -> Option<&mut AssistantTurn> {
        match self.entries.get_mut(index) {
            Some(TimelineEntry::AssistantTurn(turn)) => Some(turn),
            _ => None,
        }
    }

    pub(crate) fn render_lines(
        &self,
        theme: &Theme,
        show_assistant_steps: bool,
    ) -> Vec<Line<'static>> {
        if self.entries.is_empty() {
            return vec![Line::from(Span::styled(
                "Welcome to PyChat.ai. TAB toggles Python/AI mode. Ctrl-T toggles showing agent thinking.",
                theme.style(output_token_for(OutputKind::SystemInfo)),
            ))];
        }

        let context = RenderContext {
            theme,
            show_assistant_steps,
        };
        let mut lines = Vec::new();
        for entry in &self.entries {
            widget_for_entry(entry).render(&context, &mut lines);
        }

        lines
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}

trait TimelineWidget {
    fn render(&self, context: &RenderContext<'_>, lines: &mut Vec<Line<'static>>);
}

struct RenderContext<'a> {
    theme: &'a Theme,
    show_assistant_steps: bool,
}

struct PythonInputWidget<'a> {
    text: &'a str,
}

impl TimelineWidget for PythonInputWidget<'_> {
    fn render(&self, context: &RenderContext<'_>, lines: &mut Vec<Line<'static>>) {
        lines.push(Line::from(vec![
            Span::styled("py> ", context.theme.style(ThemeToken::PythonPrompt)),
            Span::styled(
                self.text.to_string(),
                context
                    .theme
                    .style(output_token_for(OutputKind::UserInputPython)),
            ),
        ]));
    }
}

struct CommandInputWidget<'a> {
    text: &'a str,
}

impl TimelineWidget for CommandInputWidget<'_> {
    fn render(&self, context: &RenderContext<'_>, lines: &mut Vec<Line<'static>>) {
        lines.push(Line::from(vec![
            Span::styled("cmd> ", context.theme.style(ThemeToken::CommandPrompt)),
            Span::styled(
                self.text.to_string(),
                context
                    .theme
                    .style(output_token_for(OutputKind::UserInputPython)),
            ),
        ]));
    }
}

struct OutputLineWidget<'a> {
    kind: OutputKind,
    text: &'a str,
}

impl TimelineWidget for OutputLineWidget<'_> {
    fn render(&self, context: &RenderContext<'_>, lines: &mut Vec<Line<'static>>) {
        lines.push(Line::from(Span::styled(
            self.text.to_string(),
            context.theme.style(output_token_for(self.kind)),
        )));
    }
}

struct AssistantTurnWidget<'a> {
    turn: &'a AssistantTurn,
}

impl TimelineWidget for AssistantTurnWidget<'_> {
    fn render(&self, context: &RenderContext<'_>, lines: &mut Vec<Line<'static>>) {
        const THINKING_BLOCK_PADDING: &str = "  ";
        lines.push(Line::from(vec![
            Span::styled("ai> ", context.theme.style(ThemeToken::AssistantPrompt)),
            Span::styled(
                self.turn.prompt.clone(),
                context
                    .theme
                    .style(output_token_for(OutputKind::UserInputAssistant)),
            ),
        ]));

        if context.show_assistant_steps {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw(THINKING_BLOCK_PADDING),
                Span::styled(
                    "Thinking...",
                    context
                        .theme
                        .style(output_token_for(OutputKind::AssistantWaiting)),
                ),
            ]));

            for event in &self.turn.events {
                match event {
                    AssistantStepEvent::ToolRequest { text } => {
                        lines.push(Line::from(Span::styled(
                            format!("{THINKING_BLOCK_PADDING}{text}"),
                            context
                                .theme
                                .style(output_token_for(OutputKind::AssistantProgressRequest)),
                        )));
                    }
                    AssistantStepEvent::ToolResult { text } => {
                        lines.push(Line::from(Span::styled(
                            format!("{THINKING_BLOCK_PADDING}{text}"),
                            context
                                .theme
                                .style(output_token_for(OutputKind::AssistantProgressResult)),
                        )));
                    }
                }
            }

            lines.push(Line::from(""));
        }

        match &self.turn.state {
            AssistantTurnState::InFlight => {}
            AssistantTurnState::CompletedText(text) => {
                for line in split_output_lines(text) {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        context
                            .theme
                            .style(output_token_for(OutputKind::AssistantText)),
                    )));
                }
                render_turn_token_total(context, lines, self.turn.token_usage.as_ref());
            }
            AssistantTurnState::CompletedError(message) => {
                for line in split_output_lines(message) {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        context
                            .theme
                            .style(output_token_for(OutputKind::SystemError)),
                    )));
                }
                render_turn_token_total(context, lines, self.turn.token_usage.as_ref());
            }
        }
    }
}

fn render_turn_token_total(
    context: &RenderContext<'_>,
    lines: &mut Vec<Line<'static>>,
    usage: Option<&LlmTokenUsageTotals>,
) {
    let Some(usage) = usage else {
        return;
    };
    if usage.is_zero() {
        return;
    }

    let total_text = if usage.total_tokens == 0 {
        "?".to_string()
    } else {
        usage.total_tokens.to_string()
    };
    lines.push(Line::from(Span::styled(
        format!("  Tokens (turn): {total_text}"),
        context
            .theme
            .style(output_token_for(OutputKind::SystemInfo)),
    )));
}

fn widget_for_entry(entry: &TimelineEntry) -> Box<dyn TimelineWidget + '_> {
    match entry {
        TimelineEntry::UserInputPython(text) => Box::new(PythonInputWidget { text }),
        TimelineEntry::UserInputCommand(text) => Box::new(CommandInputWidget { text }),
        TimelineEntry::OutputLine { kind, text } => {
            Box::new(OutputLineWidget { kind: *kind, text })
        }
        TimelineEntry::AssistantTurn(turn) => Box::new(AssistantTurnWidget { turn }),
    }
}

fn split_output_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }

    text.lines().collect()
}

fn output_token_for(kind: OutputKind) -> ThemeToken {
    match kind {
        OutputKind::UserInputPython => ThemeToken::UserInputPython,
        OutputKind::UserInputAssistant => ThemeToken::UserInputAssistant,
        OutputKind::PythonValue => ThemeToken::PythonValue,
        OutputKind::PythonStdout => ThemeToken::PythonStdout,
        OutputKind::PythonStderr => ThemeToken::PythonStderr,
        OutputKind::PythonTraceback => ThemeToken::PythonTraceback,
        OutputKind::AssistantText => ThemeToken::AssistantText,
        OutputKind::AssistantWaiting => ThemeToken::AssistantWaiting,
        OutputKind::AssistantProgressRequest => ThemeToken::AssistantProgressRequest,
        OutputKind::AssistantProgressResult => ThemeToken::AssistantProgressResult,
        OutputKind::SystemInfo => ThemeToken::SystemInfo,
        OutputKind::SystemError => ThemeToken::SystemError,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AssistantStepEvent, AssistantTurnState, OutputKind, Timeline, output_token_for,
        split_output_lines,
    };
    use crate::cli::theme::Theme;
    use crate::config::ThemeToken;
    use crate::llm::provider::LlmTokenUsageTotals;

    fn text_lines(lines: Vec<ratatui::text::Line<'static>>) -> Vec<String> {
        lines.into_iter().map(|line| line.to_string()).collect()
    }

    fn completed_turn_fixture() -> Timeline {
        let mut timeline = Timeline::new();
        let idx = timeline.push_assistant_turn("inspect x".to_string());
        let turn = timeline
            .assistant_turn_mut(idx)
            .expect("assistant turn index should exist");
        turn.events = vec![
            AssistantStepEvent::ToolRequest {
                text: "-> Inspecting: x".to_string(),
            },
            AssistantStepEvent::ToolResult {
                text: "<- Inspection complete: int".to_string(),
            },
        ];
        turn.state = AssistantTurnState::CompletedText("x is an int".to_string());
        turn.token_usage = Some(LlmTokenUsageTotals {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
        });
        timeline
    }

    #[test]
    fn split_lines_works() {
        assert_eq!(split_output_lines("a\nb\n"), vec!["a", "b"]);
        assert!(split_output_lines("").is_empty());
    }

    #[test]
    fn output_kind_maps_to_theme_tokens() {
        assert_eq!(
            output_token_for(OutputKind::AssistantProgressRequest),
            ThemeToken::AssistantProgressRequest
        );
        assert_eq!(
            output_token_for(OutputKind::PythonTraceback),
            ThemeToken::PythonTraceback
        );
    }

    #[test]
    fn empty_timeline_renders_welcome_message() {
        let lines = text_lines(Timeline::new().render_lines(&Theme::new(false), true));
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("Welcome to PyChat.ai. TAB toggles Python/AI mode."));
    }

    #[test]
    fn render_assistant_turn_hides_steps_when_toggle_off() {
        let lines = text_lines(completed_turn_fixture().render_lines(&Theme::new(false), false));
        assert!(lines.iter().any(|line| line == "ai> inspect x"));
        assert!(lines.iter().any(|line| line == "x is an int"));
        assert!(
            !lines
                .iter()
                .any(|line| line.starts_with("  -> Inspecting:"))
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.starts_with("  <- Inspection complete:"))
        );
        assert!(!lines.iter().any(|line| line == "  Thinking..."));
    }

    #[test]
    fn render_assistant_turn_shows_steps_when_toggle_on() {
        let lines = text_lines(completed_turn_fixture().render_lines(&Theme::new(false), true));
        assert!(lines.iter().any(|line| line == "  Thinking..."));
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("  -> Inspecting:"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("  <- Inspection complete:"))
        );
        assert!(lines.iter().any(|line| line == "x is an int"));
        assert!(lines.iter().any(|line| line == "  Tokens (turn): 15"));
    }

    #[test]
    fn toggle_is_retroactive_for_completed_turn() {
        let timeline = completed_turn_fixture();
        let with_steps = text_lines(timeline.render_lines(&Theme::new(false), true));
        let without_steps = text_lines(timeline.render_lines(&Theme::new(false), false));
        assert_ne!(with_steps, without_steps);
        assert!(
            with_steps
                .iter()
                .any(|line| line.starts_with("  <- Inspection complete:"))
        );
        assert!(
            !without_steps
                .iter()
                .any(|line| line.starts_with("  <- Inspection complete:"))
        );
    }

    #[test]
    fn inflight_turn_shows_thinking_header_and_optional_steps() {
        let mut timeline = Timeline::new();
        let idx = timeline.push_assistant_turn("inspect y".to_string());
        let turn = timeline
            .assistant_turn_mut(idx)
            .expect("assistant turn index should exist");
        turn.events = vec![AssistantStepEvent::ToolRequest {
            text: "-> Inspecting: y".to_string(),
        }];
        turn.state = AssistantTurnState::InFlight;

        let hidden = text_lines(timeline.render_lines(&Theme::new(false), false));
        assert!(
            !hidden
                .iter()
                .any(|line| line.starts_with("  -> Inspecting:"))
        );
        assert!(!hidden.iter().any(|line| line == "  Thinking..."));

        let shown = text_lines(timeline.render_lines(&Theme::new(false), true));
        assert!(shown.iter().any(|line| line == "  Thinking..."));
        assert!(
            shown
                .iter()
                .any(|line| line.starts_with("  -> Inspecting:"))
        );
    }

    #[test]
    fn assistant_error_renders_message() {
        let mut timeline = Timeline::new();
        let idx = timeline.push_assistant_turn("inspect z".to_string());
        let turn = timeline
            .assistant_turn_mut(idx)
            .expect("assistant turn index should exist");
        turn.state =
            AssistantTurnState::CompletedError("Assistant request failed: boom".to_string());
        turn.token_usage = Some(LlmTokenUsageTotals {
            input_tokens: 2,
            output_tokens: 1,
            total_tokens: 3,
        });

        let lines = text_lines(timeline.render_lines(&Theme::new(false), false));
        assert!(
            lines
                .iter()
                .any(|line| line == "Assistant request failed: boom")
        );
        assert!(lines.iter().any(|line| line == "  Tokens (turn): 3"));
    }

    #[test]
    fn thinking_block_has_blank_line_padding() {
        let lines = text_lines(completed_turn_fixture().render_lines(&Theme::new(false), true));
        let thinking_idx = lines
            .iter()
            .position(|line| line == "  Thinking...")
            .expect("thinking header");
        let request_idx = lines
            .iter()
            .position(|line| line.starts_with("  -> Inspecting:"))
            .expect("request line");
        let result_idx = lines
            .iter()
            .position(|line| line.starts_with("  <- Inspection complete:"))
            .expect("result line");

        assert!(thinking_idx > 0, "thinking line should not be first");
        assert_eq!(lines[thinking_idx - 1], "");
        assert!(
            thinking_idx < request_idx,
            "thinking header should precede requests"
        );
        assert!(
            result_idx + 1 < lines.len(),
            "result line should not be last"
        );
        assert_eq!(lines[result_idx + 1], "");
    }

    #[test]
    fn mixed_entries_render_in_order() {
        let mut timeline = Timeline::new();
        timeline.push_user_input_python("x = 1");
        timeline.push_output(OutputKind::PythonStdout, "hello");
        timeline.push_output(OutputKind::PythonStderr, "warn");
        timeline.push_output(OutputKind::PythonValue, "1");
        timeline.push_output(OutputKind::PythonTraceback, "Traceback...");
        timeline.push_output(OutputKind::SystemError, "error: boom");
        let idx = timeline.push_assistant_turn("inspect x".to_string());
        let turn = timeline
            .assistant_turn_mut(idx)
            .expect("assistant turn index should exist");
        turn.events.push(AssistantStepEvent::ToolRequest {
            text: "-> Inspecting: x".to_string(),
        });
        turn.events.push(AssistantStepEvent::ToolResult {
            text: "<- Inspection complete: int".to_string(),
        });
        turn.state = AssistantTurnState::CompletedText("x is an int".to_string());

        let lines = text_lines(timeline.render_lines(&Theme::new(false), true));
        let py_idx = lines
            .iter()
            .position(|line| line == "py> x = 1")
            .expect("python input");
        let stdout_idx = lines
            .iter()
            .position(|line| line == "hello")
            .expect("stdout");
        let stderr_idx = lines
            .iter()
            .position(|line| line == "warn")
            .expect("stderr");
        let value_idx = lines.iter().position(|line| line == "1").expect("value");
        let traceback_idx = lines
            .iter()
            .position(|line| line == "Traceback...")
            .expect("traceback");
        let error_idx = lines
            .iter()
            .position(|line| line == "error: boom")
            .expect("system error");
        let ai_idx = lines
            .iter()
            .position(|line| line == "ai> inspect x")
            .expect("assistant prompt");

        assert!(py_idx < stdout_idx);
        assert!(stdout_idx < stderr_idx);
        assert!(stderr_idx < value_idx);
        assert!(value_idx < traceback_idx);
        assert!(traceback_idx < error_idx);
        assert!(error_idx < ai_idx);
        assert!(lines.iter().any(|line| line == "  Thinking..."));
        assert!(lines.iter().any(|line| line == "x is an int"));
    }

    #[test]
    fn multiline_entries_split_and_preserve_order() {
        let mut timeline = Timeline::new();
        timeline.push_user_input_python("a = 1\nb = 2");
        timeline.push_output(OutputKind::PythonStdout, "first\nsecond");
        let idx = timeline.push_assistant_turn("summarize".to_string());
        let turn = timeline
            .assistant_turn_mut(idx)
            .expect("assistant turn index should exist");
        turn.state = AssistantTurnState::CompletedText("line one\nline two".to_string());

        let lines = text_lines(timeline.render_lines(&Theme::new(false), false));
        assert_eq!(lines[0], "py> a = 1");
        assert_eq!(lines[1], "py> b = 2");
        assert_eq!(lines[2], "first");
        assert_eq!(lines[3], "second");
        assert_eq!(lines[4], "ai> summarize");
        assert_eq!(lines[5], "line one");
        assert_eq!(lines[6], "line two");
    }
}
