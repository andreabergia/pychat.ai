use crate::agent::{AgentConfig, AgentProgressEvent, run_question_with_events};
use crate::cli::commands::{Command, CommandMode, HELP_TEXT, is_command_line, parse_command};
use crate::cli::theme::Theme;
use crate::cli::timeline::{
    AssistantStepEvent, AssistantTurn, AssistantTurnState, OutputKind, Timeline,
};
use crate::config::{ThemeConfig, ThemeToken};
use crate::llm::gemini::GeminiProvider;
use crate::python::{
    CapabilityError, CapabilityProvider, InputCompleteness, PythonSession, UserRunResult,
};
use crate::trace::SessionTrace;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Padding, Paragraph, Wrap};
use serde_json::Value;
use std::fs;
use std::io::{self, ErrorKind, IsTerminal};
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Python,
    Assistant,
}

pub struct AppState {
    pub mode: Mode,
    pub session_id: String,
    pub python: PythonSession,
    pub llm: Option<GeminiProvider>,
    pub agent_config: AgentConfig,
    pub theme_config: ThemeConfig,
    pub trace: SessionTrace,
}

#[derive(Debug, Clone)]
struct UiState {
    mode: Mode,
    session_id: String,
    python_input: String,
    assistant_input: String,
    show_assistant_steps: bool,
    history: Vec<String>,
    history_index: Option<usize>,
    timeline: Timeline,
    should_quit: bool,
    theme: Theme,
}

impl UiState {
    fn new(
        mode: Mode,
        session_id: String,
        color_enabled: bool,
        theme_config: &ThemeConfig,
    ) -> Self {
        Self {
            mode,
            session_id,
            python_input: String::new(),
            assistant_input: String::new(),
            show_assistant_steps: true,
            history: Vec::new(),
            history_index: None,
            timeline: Timeline::new(),
            should_quit: false,
            theme: Theme::from_config(color_enabled, theme_config),
        }
    }

    fn current_input(&self) -> &str {
        match self.mode {
            Mode::Python => &self.python_input,
            Mode::Assistant => &self.assistant_input,
        }
    }

    fn current_input_mut(&mut self) -> &mut String {
        match self.mode {
            Mode::Python => &mut self.python_input,
            Mode::Assistant => &mut self.assistant_input,
        }
    }

    fn push_timeline_output(&mut self, kind: OutputKind, text: &str) {
        self.timeline.push_output(kind, text);
    }

    fn push_user_input(&mut self, text: &str) {
        self.timeline.push_user_input_python(text);
    }

    fn push_assistant_turn(&mut self, prompt: String) -> usize {
        self.timeline.push_assistant_turn(prompt)
    }

    fn assistant_turn_mut(&mut self, index: usize) -> Option<&mut AssistantTurn> {
        self.timeline.assistant_turn_mut(index)
    }

    fn push_history(&mut self, line: &str) {
        self.history.push(line.to_string());
        self.history_index = None;
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }

        let next_index = match self.history_index {
            Some(0) => 0,
            Some(i) => i.saturating_sub(1),
            None => self.history.len() - 1,
        };

        self.history_index = Some(next_index);
        *self.current_input_mut() = self.history[next_index].clone();
    }

    fn history_next(&mut self) {
        if self.history.is_empty() {
            return;
        }

        match self.history_index {
            Some(i) if i + 1 < self.history.len() => {
                let next_index = i + 1;
                self.history_index = Some(next_index);
                *self.current_input_mut() = self.history[next_index].clone();
            }
            Some(_) => {
                self.history_index = None;
                self.current_input_mut().clear();
            }
            None => {}
        }
    }
}

pub async fn run_repl(state: &mut AppState) -> Result<()> {
    let color_enabled = resolve_color_enabled();
    let mut ui_state = UiState::new(
        state.mode,
        state.session_id.clone(),
        color_enabled,
        &state.theme_config,
    );

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let run_result = run_tui_loop(&mut terminal, state, &mut ui_state).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    println!("{}", session_closed_message(state.trace.file_path()));

    run_result
}

async fn run_tui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
    ui_state: &mut UiState,
) -> Result<()> {
    let poll_timeout = Duration::from_millis(50);

    loop {
        terminal.draw(|frame| draw_ui(frame, ui_state))?;

        if ui_state.should_quit {
            break;
        }

        if !event::poll(poll_timeout)? {
            continue;
        }

        if let Event::Key(key) = event::read()? {
            handle_key_event(terminal, state, ui_state, key).await?;
        }
    }

    state.mode = ui_state.mode;
    Ok(())
}

async fn handle_key_event(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
    ui_state: &mut UiState,
    key: KeyEvent,
) -> Result<()> {
    match key.code {
        KeyCode::Tab => {
            ui_state.mode = toggle_mode(ui_state.mode);
            ui_state.history_index = None;
        }
        KeyCode::Enter => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                insert_python_newline(ui_state);
            } else {
                handle_enter(terminal, state, ui_state).await?;
            }
        }
        KeyCode::Backspace => {
            ui_state.current_input_mut().pop();
            ui_state.history_index = None;
        }
        KeyCode::Up => {
            ui_state.history_prev();
        }
        KeyCode::Down => {
            ui_state.history_next();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            ui_state.should_quit = true;
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            ui_state.should_quit = true;
        }
        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            insert_python_newline(ui_state);
        }
        KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            ui_state.show_assistant_steps = !ui_state.show_assistant_steps;
        }
        KeyCode::Char(ch) => {
            ui_state.current_input_mut().push(ch);
            ui_state.history_index = None;
        }
        _ => {}
    }

    Ok(())
}

async fn handle_enter(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
    ui_state: &mut UiState,
) -> Result<()> {
    if ui_state.mode != Mode::Python {
        submit_current_line(terminal, state, ui_state).await?;
        return Ok(());
    }

    if ui_state.current_input().trim().is_empty() {
        submit_current_line(terminal, state, ui_state).await?;
        return Ok(());
    }

    match state
        .python
        .check_input_completeness(ui_state.current_input())
    {
        Ok(InputCompleteness::Incomplete) => insert_python_newline(ui_state),
        Ok(InputCompleteness::Complete) | Ok(InputCompleteness::Invalid) => {
            submit_current_line(terminal, state, ui_state).await?;
        }
        Err(err) => {
            push_output(
                ui_state,
                &state.trace,
                OutputKind::SystemError,
                &format!("error checking python input completeness: {err}"),
            );
            submit_current_line(terminal, state, ui_state).await?;
        }
    }

    Ok(())
}

fn insert_python_newline(ui_state: &mut UiState) {
    if ui_state.mode != Mode::Python {
        return;
    }
    append_newline_with_indent(&mut ui_state.python_input);
    ui_state.history_index = None;
}

async fn submit_current_line(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut AppState,
    ui_state: &mut UiState,
) -> Result<()> {
    let line = match ui_state.mode {
        Mode::Python => {
            let line = ui_state.python_input.clone();
            ui_state.python_input.clear();
            line
        }
        Mode::Assistant => {
            let line = ui_state.assistant_input.trim().to_string();
            ui_state.assistant_input.clear();
            line
        }
    };

    if line.trim().is_empty() {
        return Ok(());
    }

    if line.eq_ignore_ascii_case("exit") || line.eq_ignore_ascii_case("quit") {
        ui_state.should_quit = true;
        return Ok(());
    }

    if is_command_line(&line) {
        ui_state.push_history(&line);
        execute_command(state, ui_state, &line);
        return Ok(());
    }

    if ui_state.mode == Mode::Python {
        ui_state.push_user_input(&line);
        state.trace.log_input_python(&line);
    }
    if ui_state.mode == Mode::Assistant {
        state.trace.log_input_assistant(&line);
    }
    ui_state.push_history(&line);

    match ui_state.mode {
        Mode::Python => match state.python.run_user_input(&line) {
            Ok(UserRunResult::Evaluated(result)) => {
                if !result.stdout.is_empty() {
                    push_output(
                        ui_state,
                        &state.trace,
                        OutputKind::PythonStdout,
                        &result.stdout,
                    );
                }
                if !result.stderr.is_empty() {
                    push_output(
                        ui_state,
                        &state.trace,
                        OutputKind::PythonStderr,
                        &result.stderr,
                    );
                }
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::PythonValue,
                    &result.value_repr,
                );
            }
            Ok(UserRunResult::Executed(result)) => {
                if !result.stdout.is_empty() {
                    push_output(
                        ui_state,
                        &state.trace,
                        OutputKind::PythonStdout,
                        &result.stdout,
                    );
                }
                if !result.stderr.is_empty() {
                    push_output(
                        ui_state,
                        &state.trace,
                        OutputKind::PythonStderr,
                        &result.stderr,
                    );
                }
            }
            Ok(UserRunResult::Failed {
                stdout,
                stderr,
                exception,
            }) => {
                if !stdout.is_empty() {
                    push_output(ui_state, &state.trace, OutputKind::PythonStdout, &stdout);
                }
                if !stderr.is_empty() {
                    push_output(ui_state, &state.trace, OutputKind::PythonStderr, &stderr);
                }
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::PythonTraceback,
                    &exception.traceback,
                );
            }
            Err(err) => {
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::SystemError,
                    &format!("error: {err}"),
                );
            }
        },
        Mode::Assistant => {
            let Some(provider) = &state.llm else {
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::SystemError,
                    "Assistant unavailable: missing GEMINI_API_KEY. Configure it in your shell, .env file, or config file (example: GEMINI_API_KEY=your_key).",
                );
                return Ok(());
            };

            let turn_index = ui_state.push_assistant_turn(line.clone());
            terminal.draw(|frame| draw_ui(frame, ui_state))?;

            let mut on_event = |event: AgentProgressEvent| {
                match event {
                    AgentProgressEvent::StepStarted { .. } => {}
                    AgentProgressEvent::ModelResponse { .. } => {
                        // Keep model response metadata internal; show only tool-level progress.
                    }
                    AgentProgressEvent::ToolRequest {
                        step: _,
                        name,
                        args_json,
                        id: _,
                    } => {
                        state.trace.log_output(
                            output_trace_kind(OutputKind::AssistantProgressRequest),
                            &format_tool_request_line(&name, &args_json),
                        );
                        if let Some(turn) = ui_state.assistant_turn_mut(turn_index) {
                            turn.events.push(AssistantStepEvent::ToolRequest {
                                text: format_tool_request_line(&name, &args_json),
                            });
                        }
                    }
                    AgentProgressEvent::ToolResult {
                        step: _,
                        name,
                        response_json,
                        id: _,
                    } => {
                        state.trace.log_output(
                            output_trace_kind(OutputKind::AssistantProgressResult),
                            &format_tool_result_line(&name, &response_json),
                        );
                        if let Some(turn) = ui_state.assistant_turn_mut(turn_index) {
                            turn.events.push(AssistantStepEvent::ToolResult {
                                text: format_tool_result_line(&name, &response_json),
                            });
                        }
                    }
                }
                let _ = terminal.draw(|frame| draw_ui(frame, ui_state));
            };

            match run_question_with_events(
                provider,
                &state.python,
                &line,
                &state.agent_config,
                &mut on_event,
            )
            .await
            {
                Ok(answer) => {
                    state
                        .trace
                        .log_output(output_trace_kind(OutputKind::AssistantText), &answer.text);
                    if let Some(turn) = ui_state.assistant_turn_mut(turn_index) {
                        turn.state = AssistantTurnState::CompletedText(answer.text);
                    }
                }
                Err(err) => {
                    let message = format!("Assistant request failed: {err}");
                    state
                        .trace
                        .log_output(output_trace_kind(OutputKind::SystemError), &message);
                    if let Some(turn) = ui_state.assistant_turn_mut(turn_index) {
                        turn.state = AssistantTurnState::CompletedError(message);
                    }
                }
            };
        }
    }

    Ok(())
}

fn execute_command(state: &mut AppState, ui_state: &mut UiState, line: &str) {
    ui_state.timeline.push_user_input_command(line);
    state.trace.log_output("cmd.in", line);

    let command = match parse_command(line) {
        Ok(command) => command,
        Err(err) => {
            push_output(
                ui_state,
                &state.trace,
                OutputKind::SystemError,
                err.message(),
            );
            return;
        }
    };

    match command {
        Command::Help => {
            push_output(ui_state, &state.trace, OutputKind::SystemInfo, HELP_TEXT);
        }
        Command::Mode(mode) => match mode {
            Some(CommandMode::Python) => {
                ui_state.mode = Mode::Python;
                ui_state.history_index = None;
                push_output(ui_state, &state.trace, OutputKind::SystemInfo, "mode: py");
            }
            Some(CommandMode::Assistant) => {
                ui_state.mode = Mode::Assistant;
                ui_state.history_index = None;
                push_output(ui_state, &state.trace, OutputKind::SystemInfo, "mode: ai");
            }
            None => {
                let current = match ui_state.mode {
                    Mode::Python => "py",
                    Mode::Assistant => "ai",
                };
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::SystemInfo,
                    &format!("mode: {current}"),
                );
            }
        },
        Command::Clear => {
            ui_state.timeline.clear();
            push_output(ui_state, &state.trace, OutputKind::SystemInfo, "cleared");
        }
        Command::History(limit) => {
            let text = format_history_output(&ui_state.history, limit);
            push_output(ui_state, &state.trace, OutputKind::SystemInfo, &text);
        }
        Command::Trace => {
            push_output(
                ui_state,
                &state.trace,
                OutputKind::SystemInfo,
                &state.trace.file_path().display().to_string(),
            );
        }
        Command::Inspect { expr } => match state.python.inspect(&expr) {
            Ok(info) => match serde_json::to_string_pretty(&info.value) {
                Ok(pretty) => push_output(ui_state, &state.trace, OutputKind::SystemInfo, &pretty),
                Err(err) => push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::SystemError,
                    &format!("failed to format inspect result: {err}"),
                ),
            },
            Err(CapabilityError::PythonException(exc)) => {
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::PythonTraceback,
                    &exc.traceback,
                );
            }
            Err(err) => {
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::SystemError,
                    &format!("inspect failed: {err}"),
                );
            }
        },
        Command::LastError => match state.python.get_last_exception() {
            Ok(Some(exc)) => {
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::PythonTraceback,
                    &exc.traceback,
                );
            }
            Ok(None) => {
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::SystemInfo,
                    "no python exception recorded",
                );
            }
            Err(err) => {
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::SystemError,
                    &format!("failed to read last error: {err}"),
                );
            }
        },
        Command::Include { path } => execute_include_command(state, ui_state, &path),
        Command::ShowSource { name } => execute_source_command(state, ui_state, &name),
        Command::Steps(steps) => {
            if let Some(value) = steps {
                ui_state.show_assistant_steps = value;
            } else {
                ui_state.show_assistant_steps = !ui_state.show_assistant_steps;
            }
            let steps_text = if ui_state.show_assistant_steps {
                "on"
            } else {
                "off"
            };
            push_output(
                ui_state,
                &state.trace,
                OutputKind::SystemInfo,
                &format!("steps: {steps_text}"),
            );
        }
    }
}

fn execute_include_command(state: &mut AppState, ui_state: &mut UiState, path: &str) {
    let path_ref = Path::new(path);
    let source = match fs::read_to_string(path_ref) {
        Ok(content) => content,
        Err(err) => {
            let message = if err.kind() == ErrorKind::NotFound {
                format!("file not found: {}", path_ref.display())
            } else {
                format!("failed to read {}: {err}", path_ref.display())
            };
            push_output(ui_state, &state.trace, OutputKind::SystemError, &message);
            return;
        }
    };

    match state.python.run_exec_input(&source) {
        Ok(UserRunResult::Executed(result)) => {
            if !result.stdout.is_empty() {
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::PythonStdout,
                    &result.stdout,
                );
            }
            if !result.stderr.is_empty() {
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::PythonStderr,
                    &result.stderr,
                );
            }
            push_output(
                ui_state,
                &state.trace,
                OutputKind::SystemInfo,
                &format!("included {}", path_ref.display()),
            );
        }
        Ok(UserRunResult::Failed {
            stdout,
            stderr,
            exception,
        }) => {
            if !stdout.is_empty() {
                push_output(ui_state, &state.trace, OutputKind::PythonStdout, &stdout);
            }
            if !stderr.is_empty() {
                push_output(ui_state, &state.trace, OutputKind::PythonStderr, &stderr);
            }
            push_output(
                ui_state,
                &state.trace,
                OutputKind::PythonTraceback,
                &exception.traceback,
            );
        }
        Ok(UserRunResult::Evaluated(_)) => {
            push_output(
                ui_state,
                &state.trace,
                OutputKind::SystemError,
                "internal error: include unexpectedly evaluated expression",
            );
        }
        Err(err) => {
            push_output(
                ui_state,
                &state.trace,
                OutputKind::SystemError,
                &format!("include failed: {err}"),
            );
        }
    }
}

fn execute_source_command(state: &mut AppState, ui_state: &mut UiState, name: &str) {
    if !is_safe_source_target(name) {
        push_output(
            ui_state,
            &state.trace,
            OutputKind::SystemError,
            "usage: /show_source <name>",
        );
        return;
    }

    let code = format!("print(__import__('inspect').getsource({name}), end='')");
    match state.python.exec_code(&code) {
        Ok(result) => {
            if !result.stdout.is_empty() {
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::SystemInfo,
                    &result.stdout,
                );
            }
            if !result.stderr.is_empty() {
                push_output(
                    ui_state,
                    &state.trace,
                    OutputKind::PythonStderr,
                    &result.stderr,
                );
            }
        }
        Err(err) => {
            push_output(
                ui_state,
                &state.trace,
                OutputKind::SystemError,
                &format!("source failed: {err}"),
            );
        }
    }
}

fn is_safe_source_target(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    name.split('.').all(is_ascii_identifier)
}

fn is_ascii_identifier(segment: &str) -> bool {
    let mut chars = segment.bytes();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != b'_' {
        return false;
    }

    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == b'_')
}

fn format_history_output(history: &[String], limit: Option<usize>) -> String {
    if history.is_empty() {
        return "history is empty".to_string();
    }

    let count = limit.unwrap_or(history.len()).min(history.len());
    let start = history.len().saturating_sub(count);
    history[start..]
        .iter()
        .enumerate()
        .map(|(idx, line)| format!("{:>4}: {}", start + idx + 1, line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn draw_ui(frame: &mut ratatui::Frame<'_>, ui_state: &UiState) {
    let command_input = is_command_line(ui_state.current_input());
    let prompt = prompt_for(ui_state.mode, command_input);
    let input_lines = render_input_lines(ui_state.current_input());
    let input_line_count = input_lines.len().max(1);
    let max_input_lines = 6usize;
    let input_visible_lines = input_line_count.min(max_input_lines);
    let input_height = u16::try_from(input_visible_lines.saturating_add(2)).unwrap_or(u16::MAX);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(input_height),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let lines = ui_state
        .timeline
        .render_lines(&ui_state.theme, ui_state.show_assistant_steps);

    let visible_lines = usize::from(chunks[0].height);
    let scroll = lines.len().saturating_sub(visible_lines);
    let scroll = u16::try_from(scroll).unwrap_or(u16::MAX);

    let output = Paragraph::new(lines)
        .block(Block::default().padding(Padding::new(1, 1, 0, 0)))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(output, chunks[0]);

    let input_scroll =
        u16::try_from(input_line_count.saturating_sub(input_visible_lines)).unwrap_or(u16::MAX);
    let prompt_padding = " ".repeat(prompt.chars().count());
    let mut rendered_lines = Vec::with_capacity(input_lines.len());
    for (idx, line) in input_lines.into_iter().enumerate() {
        let prompt_span = if idx == 0 {
            Span::styled(
                prompt,
                ui_state
                    .theme
                    .style(prompt_token_for(ui_state.mode, command_input)),
            )
        } else {
            Span::styled(
                prompt_padding.clone(),
                ui_state
                    .theme
                    .style(prompt_token_for(ui_state.mode, command_input)),
            )
        };
        rendered_lines.push(Line::from(vec![
            prompt_span,
            Span::styled(line, ui_state.theme.style(ThemeToken::InputBlock)),
        ]));
    }

    let input_widget = Paragraph::new(rendered_lines)
        .block(
            Block::default()
                .padding(Padding::new(1, 1, 1, 1))
                .style(ui_state.theme.style(ThemeToken::InputBlock)),
        )
        .wrap(Wrap { trim: false })
        .scroll((input_scroll, 0));
    frame.render_widget(input_widget, chunks[1]);

    let mode_text = mode_status_text(ui_state.mode, ui_state.show_assistant_steps);
    let status_left = Paragraph::new(mode_text).style(ui_state.theme.style(ThemeToken::Status));
    frame.render_widget(status_left, chunks[2]);

    let status_right = Paragraph::new(status_right_text(&ui_state.session_id))
        .style(ui_state.theme.style(ThemeToken::Status))
        .alignment(ratatui::layout::Alignment::Right);
    frame.render_widget(status_right, chunks[2]);

    let (cursor_row, cursor_col) = input_cursor_position(ui_state.current_input());
    let cursor_row = cursor_row.saturating_sub(usize::from(input_scroll));
    let cursor_x = chunks[1]
        .x
        .saturating_add(1)
        .saturating_add(u16::try_from(prompt.chars().count()).unwrap_or(u16::MAX))
        .saturating_add(u16::try_from(cursor_col).unwrap_or(u16::MAX));
    let cursor_y = chunks[1]
        .y
        .saturating_add(1)
        .saturating_add(u16::try_from(cursor_row).unwrap_or(u16::MAX));
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn prompt_token_for(mode: Mode, command_input: bool) -> ThemeToken {
    if command_input {
        return ThemeToken::CommandPrompt;
    }

    match mode {
        Mode::Python => ThemeToken::PythonPrompt,
        Mode::Assistant => ThemeToken::AssistantPrompt,
    }
}

fn format_tool_request_line(name: &str, args_json: &Value) -> String {
    match name {
        "list_globals" => "-> Listing globals".to_string(),
        "inspect" => format!(
            "-> Inspecting: {}",
            extract_expr_preview(args_json).unwrap_or_else(|| "<missing expr>".to_string())
        ),
        "eval_expr" => format!(
            "-> Evaluating: {}",
            extract_expr_preview(args_json).unwrap_or_else(|| "<missing expr>".to_string())
        ),
        _ => format!("-> Calling tool: {name}"),
    }
}

fn format_tool_result_line(name: &str, response_json: &Value) -> String {
    if !response_json
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return format_tool_error_line(name, response_json);
    }

    let Some(result) = response_json.get("result") else {
        return format!("Tool completed: {name}");
    };

    match name {
        "list_globals" => {
            let count = result
                .get("globals")
                .and_then(Value::as_array)
                .map_or(0, |globals| globals.len());
            format!("<- Found {count} globals")
        }
        "inspect" => {
            let info = result
                .get("type")
                .and_then(|ty| ty.get("name"))
                .and_then(Value::as_str)
                .or_else(|| result.get("kind").and_then(Value::as_str))
                .unwrap_or("value");
            format!("<- Inspection complete: {}", preview_text(info, 80))
        }
        "eval_expr" => {
            let value_repr = result
                .get("value_repr")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            format!("<- Evaluated: {}", preview_text(value_repr, 80))
        }
        _ => format!("<- Tool completed: {name}"),
    }
}

fn format_tool_error_line(name: &str, response_json: &Value) -> String {
    let (code, message) = response_json
        .get("error")
        .and_then(Value::as_object)
        .map(|error| {
            let code = error
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_string();
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("tool failed")
                .to_string();
            (code, message)
        })
        .unwrap_or_else(|| ("error".to_string(), "tool failed".to_string()));
    format!(
        "<- Tool error ({name}): {code}: {}",
        preview_text(&message, 100)
    )
}

fn extract_expr_preview(args_json: &Value) -> Option<String> {
    args_json
        .as_object()
        .and_then(|args| args.get("expr"))
        .and_then(Value::as_str)
        .map(|expr| preview_text(expr, 80))
}

fn preview_text(value: &str, max_len: usize) -> String {
    let normalized = normalize_whitespace(value);
    truncate_chars(&normalized, max_len)
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(value: &str, max_len: usize) -> String {
    let mut chars = value.chars();
    let preview: String = chars.by_ref().take(max_len).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

fn last_line_indent(input: &str) -> String {
    input
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .collect()
}

fn append_newline_with_indent(input: &mut String) {
    let indent = last_line_indent(input);
    input.push('\n');
    input.push_str(&indent);
}

fn render_input_lines(input: &str) -> Vec<&str> {
    if input.is_empty() {
        return vec![""];
    }
    input.split('\n').collect()
}

fn input_cursor_position(input: &str) -> (usize, usize) {
    if input.is_empty() {
        return (0, 0);
    }

    let lines: Vec<&str> = input.split('\n').collect();
    let row = lines.len().saturating_sub(1);
    let col = lines[row].chars().count();
    (row, col)
}

fn resolve_color_enabled() -> bool {
    resolve_color_enabled_with(
        std::env::var("NO_COLOR").ok(),
        std::env::var("PYAICHAT_FORCE_COLOR").ok(),
        io::stdout().is_terminal(),
    )
}

fn resolve_color_enabled_with(
    no_color: Option<String>,
    force_color: Option<String>,
    is_tty: bool,
) -> bool {
    if let Some(value) = force_color
        && matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    {
        return true;
    }

    if no_color.is_some() {
        return false;
    }

    is_tty
}

pub fn prompt_for(mode: Mode, command_input: bool) -> &'static str {
    if command_input {
        return "cmd> ";
    }

    match mode {
        Mode::Python => "py> ",
        Mode::Assistant => "ai> ",
    }
}

fn toggle_mode(mode: Mode) -> Mode {
    match mode {
        Mode::Python => Mode::Assistant,
        Mode::Assistant => Mode::Python,
    }
}

fn mode_status_text(mode: Mode, show_assistant_steps: bool) -> String {
    let mode_text = match mode {
        Mode::Python => "Mode: Python",
        Mode::Assistant => "Mode: AI Assistant",
    };
    let steps_text = if show_assistant_steps { "On" } else { "Off" };
    format!("{mode_text} | Show agent thinking: {steps_text} (Ctrl-T)")
}

fn status_right_text(session_id: &str) -> String {
    format!("PyAiChat | Session: {session_id}")
}

fn session_closed_message(trace_file_path: &std::path::Path) -> String {
    format!(
        "PyAiChat session ended.\nTrace file: {}",
        trace_file_path.display()
    )
}

fn push_output(ui_state: &mut UiState, trace: &SessionTrace, kind: OutputKind, text: &str) {
    ui_state.push_timeline_output(kind, text);
    trace.log_output(output_trace_kind(kind), text);
}

fn output_trace_kind(kind: OutputKind) -> &'static str {
    match kind {
        OutputKind::UserInputPython => "py.in",
        OutputKind::UserInputAssistant => "ai.in",
        OutputKind::PythonValue => "py.out",
        OutputKind::PythonStdout => "py.out",
        OutputKind::PythonStderr => "py.err",
        OutputKind::PythonTraceback => "py.tb",
        OutputKind::AssistantText => "ai.out",
        OutputKind::AssistantWaiting => "ai.wait",
        OutputKind::AssistantProgressRequest => "ai.step",
        OutputKind::AssistantProgressResult => "ai.step",
        OutputKind::SystemInfo => "sys.info",
        OutputKind::SystemError => "sys.err",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppState, Mode, UiState, append_newline_with_indent, execute_command,
        format_history_output, format_tool_error_line, format_tool_request_line,
        format_tool_result_line, input_cursor_position, is_safe_source_target, last_line_indent,
        mode_status_text, output_trace_kind, preview_text, prompt_for, resolve_color_enabled_with,
        session_closed_message, status_right_text, toggle_mode,
    };
    use crate::agent::AgentConfig;
    use crate::cli::timeline::OutputKind;
    use crate::config::ThemeConfig;
    use crate::python::PythonSession;
    use crate::trace::SessionTrace;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn test_toggle_mode() {
        assert_eq!(toggle_mode(Mode::Python), Mode::Assistant);
        assert_eq!(toggle_mode(Mode::Assistant), Mode::Python);
    }

    #[test]
    fn test_prompt_for() {
        assert_eq!(prompt_for(Mode::Python, false), "py> ");
        assert_eq!(prompt_for(Mode::Assistant, false), "ai> ");
        assert_eq!(prompt_for(Mode::Python, true), "cmd> ");
    }

    #[test]
    fn force_color_overrides_no_color() {
        assert!(resolve_color_enabled_with(
            Some("1".to_string()),
            Some("true".to_string()),
            false
        ));
    }

    #[test]
    fn no_color_disables_when_not_forced() {
        assert!(!resolve_color_enabled_with(
            Some("1".to_string()),
            None,
            true
        ));
    }

    #[test]
    fn tty_enables_colors_by_default() {
        assert!(resolve_color_enabled_with(None, None, true));
        assert!(!resolve_color_enabled_with(None, None, false));
    }

    #[test]
    fn preview_text_truncates_and_normalizes_whitespace() {
        let text = preview_text("a  \n\t b", 3);
        assert_eq!(text, "a b");
        let text = preview_text(&"x".repeat(20), 10);
        assert_eq!(text, format!("{}...", "x".repeat(10)));
    }

    #[test]
    fn last_line_indent_uses_only_leading_whitespace() {
        assert_eq!(last_line_indent("    if True:"), "    ");
        assert_eq!(last_line_indent("x = 1"), "");
        assert_eq!(last_line_indent("x = 1\n\t  y = 2"), "\t  ");
    }

    #[test]
    fn append_newline_with_indent_copies_previous_indent() {
        let mut input = "if True:\n    x = 1".to_string();
        append_newline_with_indent(&mut input);
        assert_eq!(input, "if True:\n    x = 1\n    ");
    }

    #[test]
    fn input_cursor_position_tracks_multiline_tail() {
        assert_eq!(input_cursor_position(""), (0, 0));
        assert_eq!(input_cursor_position("abc"), (0, 3));
        assert_eq!(input_cursor_position("a\nbc"), (1, 2));
    }

    #[test]
    fn mode_status_text_includes_step_toggle_value() {
        assert_eq!(
            mode_status_text(Mode::Python, true),
            "Mode: Python | Show agent thinking: On (Ctrl-T)"
        );
        assert_eq!(
            mode_status_text(Mode::Assistant, false),
            "Mode: AI Assistant | Show agent thinking: Off (Ctrl-T)"
        );
    }

    #[test]
    fn status_right_text_includes_session_id() {
        assert_eq!(status_right_text("abc123"), "PyAiChat | Session: abc123");
    }

    #[test]
    fn session_closed_message_includes_trace_file_path() {
        assert_eq!(
            session_closed_message(std::path::Path::new(
                "/tmp/pyaichat/traces/session-abc123.log"
            )),
            "PyAiChat session ended.\nTrace file: /tmp/pyaichat/traces/session-abc123.log"
        );
    }

    #[test]
    fn output_trace_kind_maps_tokens() {
        assert_eq!(output_trace_kind(OutputKind::PythonStdout), "py.out");
        assert_eq!(
            output_trace_kind(OutputKind::AssistantProgressResult),
            "ai.step"
        );
    }

    #[test]
    fn format_tool_request_line_uses_semantic_labels() {
        assert_eq!(
            format_tool_request_line("list_globals", &json!({})),
            "-> Listing globals"
        );
        assert_eq!(
            format_tool_request_line("inspect", &json!({"expr":"value [ 0 ]"})),
            "-> Inspecting: value [ 0 ]"
        );
        assert_eq!(
            format_tool_request_line("eval_expr", &json!({"expr":"a + b"})),
            "-> Evaluating: a + b"
        );
    }

    #[test]
    fn format_tool_result_line_summarizes_known_tools() {
        assert_eq!(
            format_tool_result_line(
                "list_globals",
                &json!({"ok":true,"result":{"globals":[{"name":"a"},{"name":"b"}]}})
            ),
            "<- Found 2 globals"
        );
        assert_eq!(
            format_tool_result_line(
                "inspect",
                &json!({"ok":true,"result":{"type":{"name":"dict"}}})
            ),
            "<- Inspection complete: dict"
        );
        assert_eq!(
            format_tool_result_line("eval_expr", &json!({"ok":true,"result":{"value_repr":"3"}})),
            "<- Evaluated: 3"
        );
    }

    #[test]
    fn format_tool_error_line_includes_code_and_reason() {
        assert_eq!(
            format_tool_error_line(
                "inspect",
                &json!({"ok":false,"error":{"code":"python_exception","message":"NameError: x"}})
            ),
            "<- Tool error (inspect): python_exception: NameError: x"
        );
    }

    #[test]
    fn format_history_output_limits_tail_entries() {
        let history = vec![
            "a = 1".to_string(),
            "/help".to_string(),
            "x + 1".to_string(),
            "/history 2".to_string(),
        ];
        assert_eq!(
            format_history_output(&history, Some(2)),
            "   3: x + 1\n   4: /history 2"
        );
    }

    #[test]
    fn source_target_validation_allows_identifier_paths_only() {
        assert!(is_safe_source_target("my_fn"));
        assert!(is_safe_source_target("module.ClassName"));
        assert!(!is_safe_source_target(""));
        assert!(!is_safe_source_target("1name"));
        assert!(!is_safe_source_target("obj.method()"));
        assert!(!is_safe_source_target("__import__('os').system"));
    }

    #[test]
    fn execute_command_mode_and_steps_updates_ui_state() {
        let dir = tempdir().expect("tempdir");
        let mut state = test_app_state("mode-steps", dir.path());
        let mut ui_state = test_ui_state();

        execute_command(&mut state, &mut ui_state, "/mode ai");
        assert_eq!(ui_state.mode, Mode::Assistant);

        execute_command(&mut state, &mut ui_state, "/steps off");
        assert!(!ui_state.show_assistant_steps);

        execute_command(&mut state, &mut ui_state, "/steps");
        assert!(ui_state.show_assistant_steps);
    }

    #[test]
    fn execute_command_trace_prints_exact_path() {
        let dir = tempdir().expect("tempdir");
        let mut state = test_app_state("trace", dir.path());
        let mut ui_state = test_ui_state();

        execute_command(&mut state, &mut ui_state, "/trace");
        let lines = timeline_text_lines(&ui_state);
        assert!(
            lines.iter().any(|line| line == "cmd> /trace"),
            "command input should be rendered in timeline"
        );
        let trace_path = state.trace.file_path().display().to_string();
        assert!(lines.contains(&trace_path));
    }

    #[test]
    fn include_and_run_execute_python_file_and_preserve_state() {
        let dir = tempdir().expect("tempdir");
        let file_path = dir.path().join("script.py");
        std::fs::write(&file_path, "value = 41\nprint('loaded')\n").expect("write script");

        let mut state = test_app_state("include", dir.path());
        let mut ui_state = test_ui_state();

        execute_command(
            &mut state,
            &mut ui_state,
            &format!("/include {}", file_path.display()),
        );
        assert_eq!(
            state.python.eval_expr("value").expect("value").value_repr,
            "41"
        );

        execute_command(
            &mut state,
            &mut ui_state,
            &format!("/run {}", file_path.display()),
        );
        assert_eq!(
            state.python.eval_expr("value").expect("value").value_repr,
            "41"
        );
    }

    #[test]
    fn include_reports_missing_file_error() {
        let dir = tempdir().expect("tempdir");
        let mut state = test_app_state("missing-file", dir.path());
        let mut ui_state = test_ui_state();

        execute_command(&mut state, &mut ui_state, "/include does_not_exist.py");

        let lines = timeline_text_lines(&ui_state);
        assert!(
            lines
                .iter()
                .any(|line| line == "file not found: does_not_exist.py"),
            "missing file error should be visible"
        );
    }

    #[test]
    fn include_rejects_non_python_path() {
        let dir = tempdir().expect("tempdir");
        let mut state = test_app_state("invalid-path", dir.path());
        let mut ui_state = test_ui_state();

        execute_command(&mut state, &mut ui_state, "/include not_python.txt");

        let lines = timeline_text_lines(&ui_state);
        assert!(
            lines.iter().any(|line| line == "usage: /include <file.py>"),
            "usage text should be shown"
        );
    }

    #[test]
    fn run_without_argument_reports_missing_file_argument() {
        let dir = tempdir().expect("tempdir");
        let mut state = test_app_state("run-missing-arg", dir.path());
        let mut ui_state = test_ui_state();

        execute_command(&mut state, &mut ui_state, "/run");

        let lines = timeline_text_lines(&ui_state);
        assert!(
            lines
                .iter()
                .any(|line| line == "missing file argument. usage: /run <file.py>"),
            "missing file argument text should be shown"
        );
    }

    #[test]
    fn show_source_rejects_unsafe_expression() {
        let dir = tempdir().expect("tempdir");
        let mut state = test_app_state("source-injection", dir.path());
        let mut ui_state = test_ui_state();

        let marker = dir.path().join("marker.txt");
        let command = format!(
            "/show_source __import__('pathlib').Path({:?}).write_text('x')",
            marker
        );

        execute_command(&mut state, &mut ui_state, &command);

        let lines = timeline_text_lines(&ui_state);
        assert!(
            lines
                .iter()
                .any(|line| line == "usage: /show_source <name>"),
            "unsafe source expression should be rejected"
        );
        assert!(
            !marker.exists(),
            "unsafe expression should not execute side effects"
        );
    }

    fn test_app_state(session_id: &str, trace_dir: &std::path::Path) -> AppState {
        AppState {
            mode: Mode::Python,
            session_id: session_id.to_string(),
            python: PythonSession::initialize().expect("python"),
            llm: None,
            agent_config: AgentConfig::default(),
            theme_config: ThemeConfig::default(),
            trace: SessionTrace::create_in_temp_dir(session_id, trace_dir).expect("trace"),
        }
    }

    fn test_ui_state() -> UiState {
        UiState::new(
            Mode::Python,
            "test-session".to_string(),
            false,
            &ThemeConfig::default(),
        )
    }

    fn timeline_text_lines(ui_state: &UiState) -> Vec<String> {
        ui_state
            .timeline
            .render_lines(&ui_state.theme, ui_state.show_assistant_steps)
            .into_iter()
            .map(|line| line.to_string())
            .collect()
    }
}
