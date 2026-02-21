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
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Padding, Paragraph, Wrap};
use serde_json::Value;
use std::fs;
use std::io::{self, ErrorKind, IsTerminal};
use std::path::Path;
use std::time::Duration;

const TIMELINE_SCROLL_STEP: usize = 3;

#[derive(Debug, Clone, Copy)]
struct UiLayout {
    timeline: Rect,
    input: Rect,
    status: Rect,
}

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
    pub startup_message: Option<String>,
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
    timeline_scroll: usize,
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
            timeline_scroll: 0,
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

    fn scroll_timeline_up(&mut self, lines: usize, max_scroll: usize) {
        self.timeline_scroll = self.timeline_scroll.saturating_add(lines).min(max_scroll);
    }

    fn scroll_timeline_down(&mut self, lines: usize) {
        self.timeline_scroll = self.timeline_scroll.saturating_sub(lines);
    }

    fn timeline_scroll_offset(&self, max_scroll: usize) -> usize {
        self.timeline_scroll.min(max_scroll)
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
    initialize_timeline(state, &mut ui_state);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let run_result = run_tui_loop(&mut terminal, state, &mut ui_state).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    println!("{}", session_closed_message(state.trace.file_path()));

    run_result
}

fn initialize_timeline(state: &AppState, ui_state: &mut UiState) {
    if let Some(message) = state.startup_message.as_deref() {
        push_output(ui_state, &state.trace, OutputKind::SystemInfo, message);
    }
}

async fn run_tui_loop(
    terminal: &mut Terminal<impl ratatui::backend::Backend>,
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

        match event::read()? {
            Event::Key(key) => handle_key_event(terminal, state, ui_state, key).await?,
            Event::Mouse(mouse) => {
                let size = terminal.size()?;
                let area = Rect::new(0, 0, size.width, size.height);
                let layout = ui_layout(area, ui_state.current_input());
                let line_count = ui_state
                    .timeline
                    .render_lines(&ui_state.theme, ui_state.show_assistant_steps)
                    .len();
                let max_scroll =
                    timeline_max_scroll(line_count, usize::from(layout.timeline.height));
                handle_mouse_event(ui_state, mouse, layout.timeline, max_scroll);
            }
            _ => {}
        }
    }

    state.mode = ui_state.mode;
    Ok(())
}

fn handle_mouse_event(
    ui_state: &mut UiState,
    mouse: MouseEvent,
    timeline_area: Rect,
    max_timeline_scroll: usize,
) {
    if !area_contains_point(timeline_area, mouse.column, mouse.row) {
        return;
    }

    match mouse.kind {
        MouseEventKind::ScrollUp => {
            ui_state.scroll_timeline_up(TIMELINE_SCROLL_STEP, max_timeline_scroll);
        }
        MouseEventKind::ScrollDown => {
            ui_state.scroll_timeline_down(TIMELINE_SCROLL_STEP);
        }
        _ => {}
    }
}

async fn handle_key_event(
    terminal: &mut Terminal<impl ratatui::backend::Backend>,
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
    terminal: &mut Terminal<impl ratatui::backend::Backend>,
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
    terminal: &mut Terminal<impl ratatui::backend::Backend>,
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

fn ui_layout(area: Rect, current_input: &str) -> UiLayout {
    let input_line_count = render_input_lines(current_input).len().max(1);
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
        .split(area);

    UiLayout {
        timeline: chunks[0],
        input: chunks[1],
        status: chunks[2],
    }
}

fn timeline_max_scroll(total_lines: usize, visible_lines: usize) -> usize {
    total_lines.saturating_sub(visible_lines)
}

fn timeline_paragraph_scroll(
    total_lines: usize,
    visible_lines: usize,
    timeline_scroll: usize,
) -> u16 {
    let max_scroll = timeline_max_scroll(total_lines, visible_lines);
    let scroll = max_scroll.saturating_sub(timeline_scroll.min(max_scroll));
    u16::try_from(scroll).unwrap_or(u16::MAX)
}

fn area_contains_point(area: Rect, column: u16, row: u16) -> bool {
    if area.width == 0 || area.height == 0 {
        return false;
    }
    let in_x = column >= area.x && column < area.x.saturating_add(area.width);
    let in_y = row >= area.y && row < area.y.saturating_add(area.height);
    in_x && in_y
}

fn draw_ui(frame: &mut ratatui::Frame<'_>, ui_state: &UiState) {
    let command_input = is_command_line(ui_state.current_input());
    let prompt = prompt_for(ui_state.mode, command_input);
    let input_lines = render_input_lines(ui_state.current_input());
    let input_line_count = input_lines.len().max(1);
    let max_input_lines = 6usize;
    let input_visible_lines = input_line_count.min(max_input_lines);
    let layout = ui_layout(frame.area(), ui_state.current_input());

    let lines = ui_state
        .timeline
        .render_lines(&ui_state.theme, ui_state.show_assistant_steps);

    let visible_lines = usize::from(layout.timeline.height);
    let scroll = timeline_paragraph_scroll(
        lines.len(),
        visible_lines,
        ui_state.timeline_scroll_offset(timeline_max_scroll(lines.len(), visible_lines)),
    );

    let output = Paragraph::new(lines)
        .block(Block::default().padding(Padding::new(1, 1, 0, 0)))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(output, layout.timeline);

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
    frame.render_widget(input_widget, layout.input);

    let mode_text = mode_status_text(ui_state.mode, ui_state.show_assistant_steps);
    let status_left = Paragraph::new(mode_text).style(ui_state.theme.style(ThemeToken::Status));
    frame.render_widget(status_left, layout.status);

    let status_right = Paragraph::new(status_right_text(&ui_state.session_id))
        .style(ui_state.theme.style(ThemeToken::Status))
        .alignment(ratatui::layout::Alignment::Right);
    frame.render_widget(status_right, layout.status);

    let (cursor_row, cursor_col) = input_cursor_position(ui_state.current_input());
    let cursor_row = cursor_row.saturating_sub(usize::from(input_scroll));
    let cursor_x = layout
        .input
        .x
        .saturating_add(1)
        .saturating_add(u16::try_from(prompt.chars().count()).unwrap_or(u16::MAX))
        .saturating_add(u16::try_from(cursor_col).unwrap_or(u16::MAX));
    let cursor_y = layout
        .input
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
        std::env::var("PYCHAT_AI_FORCE_COLOR").ok(),
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
    format!("PyChat.ai | Session: {session_id}")
}

fn session_closed_message(trace_file_path: &std::path::Path) -> String {
    format!(
        "PyChat.ai session ended.\nTrace file: {}",
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

#[cfg(feature = "test-support")]
pub mod test_support {
    use super::{
        AppState, Mode, UiState, draw_ui, handle_key_event, handle_mouse_event, is_command_line,
        prompt_for, timeline_max_scroll, ui_layout,
    };
    use crate::agent::AgentConfig;
    use crate::cli::timeline::{AssistantStepEvent, AssistantTurnState};
    use crate::config::ThemeConfig;
    use crate::python::PythonSession;
    use crate::trace::SessionTrace;
    use anyhow::{Context, Result, bail};
    use crossterm::event::{KeyEvent, MouseEvent};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug)]
    pub struct UiStateView {
        pub mode: Mode,
        pub prompt: &'static str,
        pub input: String,
        pub timeline_scroll: usize,
        pub show_assistant_steps: bool,
        pub session_id: String,
    }

    pub struct UiHarness {
        terminal: Terminal<TestBackend>,
        app_state: AppState,
        ui_state: UiState,
    }

    #[derive(Debug, Clone)]
    pub struct DeterministicTestEnv {
        pub xdg_config_home: PathBuf,
        pub xdg_state_home: PathBuf,
        pub no_color: String,
    }

    #[derive(Debug, Clone, Copy)]
    pub struct UiRegions {
        pub timeline: Rect,
        pub input: Rect,
        pub status: Rect,
    }

    impl DeterministicTestEnv {
        pub fn apply_to_command(&self, command: &mut std::process::Command) {
            command
                .env("NO_COLOR", &self.no_color)
                .env("XDG_CONFIG_HOME", &self.xdg_config_home)
                .env("XDG_STATE_HOME", &self.xdg_state_home);
        }
    }

    impl UiHarness {
        pub fn new(width: u16, height: u16, app_state: AppState) -> Result<Self> {
            let backend = TestBackend::new(width, height);
            let terminal = Terminal::new(backend)?;
            let ui_state = UiState::new(
                app_state.mode,
                app_state.session_id.clone(),
                false,
                &app_state.theme_config,
            );

            Ok(Self {
                terminal,
                app_state,
                ui_state,
            })
        }

        pub fn app_state(&self) -> &AppState {
            &self.app_state
        }

        pub fn app_state_mut(&mut self) -> &mut AppState {
            &mut self.app_state
        }

        pub fn ui_state_view(&self) -> UiStateView {
            let input = self.ui_state.current_input().to_string();
            let command_input = is_command_line(&input);
            UiStateView {
                mode: self.ui_state.mode,
                prompt: prompt_for(self.ui_state.mode, command_input),
                input,
                timeline_scroll: self.ui_state.timeline_scroll,
                show_assistant_steps: self.ui_state.show_assistant_steps,
                session_id: self.ui_state.session_id.clone(),
            }
        }

        pub fn render(&mut self) -> Result<()> {
            self.terminal.draw(|frame| draw_ui(frame, &self.ui_state))?;
            Ok(())
        }

        pub async fn send_key(&mut self, key: KeyEvent) -> Result<()> {
            handle_key_event(
                &mut self.terminal,
                &mut self.app_state,
                &mut self.ui_state,
                key,
            )
            .await
        }

        pub fn send_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
            let regions = self.regions()?;
            let line_count = self
                .ui_state
                .timeline
                .render_lines(&self.ui_state.theme, self.ui_state.show_assistant_steps)
                .len();
            let max_scroll = timeline_max_scroll(line_count, usize::from(regions.timeline.height));
            handle_mouse_event(&mut self.ui_state, mouse, regions.timeline, max_scroll);
            Ok(())
        }

        pub fn buffer_text(&self) -> String {
            buffer_to_string(self.terminal.backend().buffer())
        }

        pub fn buffer_lines(&self) -> Vec<String> {
            buffer_to_lines(self.terminal.backend().buffer())
        }

        pub fn line(&self, row: u16) -> Option<String> {
            self.buffer_lines().get(usize::from(row)).cloned()
        }

        pub fn terminal_size(&self) -> (u16, u16) {
            let area = self.terminal.backend().buffer().area;
            (area.width, area.height)
        }

        pub fn regions(&self) -> Result<UiRegions> {
            let size = self.terminal.size()?;
            let area = Rect::new(0, 0, size.width, size.height);
            let layout = ui_layout(area, self.ui_state.current_input());
            Ok(UiRegions {
                timeline: layout.timeline,
                input: layout.input,
                status: layout.status,
            })
        }

        pub fn buffer_snapshot(&self) -> String {
            normalize_snapshot(&self.buffer_text())
        }

        pub fn seed_assistant_turn_completed(
            &mut self,
            prompt: &str,
            tool_events: &[(&str, &str)],
            response: &str,
        ) -> Result<()> {
            let index = self.ui_state.push_assistant_turn(prompt.to_string());
            let Some(turn) = self.ui_state.assistant_turn_mut(index) else {
                bail!("failed to find seeded assistant turn at index {index}");
            };

            for (kind, text) in tool_events {
                if kind.eq_ignore_ascii_case("request") {
                    turn.events.push(AssistantStepEvent::ToolRequest {
                        text: (*text).to_string(),
                    });
                } else if kind.eq_ignore_ascii_case("result") {
                    turn.events.push(AssistantStepEvent::ToolResult {
                        text: (*text).to_string(),
                    });
                } else {
                    bail!("unsupported assistant event kind: {kind}");
                }
            }
            turn.state = AssistantTurnState::CompletedText(response.to_string());
            Ok(())
        }

        pub fn seed_assistant_turn_error(&mut self, prompt: &str, error: &str) -> Result<()> {
            let index = self.ui_state.push_assistant_turn(prompt.to_string());
            let Some(turn) = self.ui_state.assistant_turn_mut(index) else {
                bail!("failed to find seeded assistant turn at index {index}");
            };
            turn.state = AssistantTurnState::CompletedError(error.to_string());
            Ok(())
        }
    }

    pub fn deterministic_app_state(session_id: &str) -> Result<AppState> {
        let (state, _) = deterministic_app_state_with_env(session_id)?;
        Ok(state)
    }

    pub fn deterministic_app_state_with_env(
        session_id: &str,
    ) -> Result<(AppState, DeterministicTestEnv)> {
        let env = deterministic_test_env()?;
        let trace_dir = env.xdg_state_home.join("pychat.ai").join("traces");
        let state = AppState {
            mode: Mode::Python,
            session_id: session_id.to_string(),
            python: PythonSession::initialize()?,
            llm: None,
            agent_config: AgentConfig::default(),
            theme_config: ThemeConfig::default(),
            startup_message: None,
            trace: SessionTrace::create_in_temp_dir(session_id, &trace_dir)?,
        };
        Ok((state, env))
    }

    pub fn deterministic_app_state_with_theme(
        session_id: &str,
        theme_config: ThemeConfig,
    ) -> Result<AppState> {
        let mut state = deterministic_app_state(session_id)?;
        state.theme_config = theme_config;
        Ok(state)
    }

    pub fn deterministic_test_env() -> Result<DeterministicTestEnv> {
        let root = unique_test_root_dir()?;
        let xdg_config_home = root.join("config-home");
        let xdg_state_home = root.join("state-home");
        fs::create_dir_all(&xdg_config_home)
            .with_context(|| format!("failed to create {}", xdg_config_home.display()))?;
        fs::create_dir_all(&xdg_state_home)
            .with_context(|| format!("failed to create {}", xdg_state_home.display()))?;
        Ok(DeterministicTestEnv {
            xdg_config_home,
            xdg_state_home,
            no_color: "1".to_string(),
        })
    }

    fn unique_test_root_dir() -> Result<PathBuf> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "pychat.ai-test-support-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
        Ok(dir)
    }

    fn buffer_to_string(buffer: &Buffer) -> String {
        buffer_to_lines(buffer).join("\n")
    }

    fn buffer_to_lines(buffer: &Buffer) -> Vec<String> {
        let mut lines = Vec::with_capacity(usize::from(buffer.area.height));
        for y in 0..buffer.area.height {
            let mut line = String::new();
            for x in 0..buffer.area.width {
                let cell = buffer
                    .cell((x, y))
                    .expect("buffer index should be in-bounds");
                line.push_str(cell.symbol());
            }
            lines.push(line);
        }
        lines
    }

    fn normalize_snapshot(text: &str) -> String {
        text.replace("\r\n", "\n")
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppState, Mode, UiState, append_newline_with_indent, area_contains_point, execute_command,
        format_history_output, format_tool_error_line, format_tool_request_line,
        format_tool_result_line, handle_mouse_event, input_cursor_position, is_safe_source_target,
        last_line_indent, mode_status_text, output_trace_kind, preview_text, prompt_for,
        resolve_color_enabled_with, session_closed_message, status_right_text, timeline_max_scroll,
        timeline_paragraph_scroll, toggle_mode,
    };
    use crate::agent::AgentConfig;
    use crate::cli::timeline::OutputKind;
    use crate::config::ThemeConfig;
    use crate::python::PythonSession;
    use crate::trace::SessionTrace;
    use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
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
        assert_eq!(status_right_text("abc123"), "PyChat.ai | Session: abc123");
    }

    #[test]
    fn session_closed_message_includes_trace_file_path() {
        assert_eq!(
            session_closed_message(std::path::Path::new(
                "/tmp/pychat.ai/traces/session-abc123.log"
            )),
            "PyChat.ai session ended.\nTrace file: /tmp/pychat.ai/traces/session-abc123.log"
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
    fn timeline_paragraph_scroll_follows_manual_offset() {
        assert_eq!(timeline_paragraph_scroll(20, 5, 0), 15);
        assert_eq!(timeline_paragraph_scroll(20, 5, 3), 12);
        assert_eq!(timeline_paragraph_scroll(20, 5, 99), 0);
    }

    #[test]
    fn area_contains_point_matches_rect_bounds() {
        let area = Rect::new(10, 5, 3, 2);
        assert!(area_contains_point(area, 10, 5));
        assert!(area_contains_point(area, 12, 6));
        assert!(!area_contains_point(area, 13, 6));
        assert!(!area_contains_point(area, 12, 7));
    }

    #[test]
    fn mouse_wheel_scrolls_timeline_with_clamp() {
        let mut ui_state = test_ui_state();
        let timeline_area = Rect::new(0, 0, 80, 8);
        let max_scroll = 7usize;

        handle_mouse_event(
            &mut ui_state,
            mouse_event(MouseEventKind::ScrollUp, 2, 2),
            timeline_area,
            max_scroll,
        );
        assert_eq!(ui_state.timeline_scroll, 3);

        handle_mouse_event(
            &mut ui_state,
            mouse_event(MouseEventKind::ScrollUp, 2, 2),
            timeline_area,
            max_scroll,
        );
        handle_mouse_event(
            &mut ui_state,
            mouse_event(MouseEventKind::ScrollUp, 2, 2),
            timeline_area,
            max_scroll,
        );
        assert_eq!(ui_state.timeline_scroll, max_scroll);

        handle_mouse_event(
            &mut ui_state,
            mouse_event(MouseEventKind::ScrollDown, 2, 2),
            timeline_area,
            max_scroll,
        );
        assert_eq!(ui_state.timeline_scroll, 4);

        handle_mouse_event(
            &mut ui_state,
            mouse_event(MouseEventKind::ScrollDown, 2, 2),
            timeline_area,
            max_scroll,
        );
        handle_mouse_event(
            &mut ui_state,
            mouse_event(MouseEventKind::ScrollDown, 2, 2),
            timeline_area,
            max_scroll,
        );
        assert_eq!(ui_state.timeline_scroll, 0);
    }

    #[test]
    fn mouse_wheel_outside_timeline_is_ignored() {
        let mut ui_state = test_ui_state();
        ui_state.timeline_scroll = 4;
        let timeline_area = Rect::new(0, 0, 80, 8);

        handle_mouse_event(
            &mut ui_state,
            mouse_event(MouseEventKind::ScrollUp, 2, 10),
            timeline_area,
            20,
        );

        assert_eq!(ui_state.timeline_scroll, 4);
    }

    #[test]
    fn mouse_wheel_does_not_change_history_selection() {
        let mut ui_state = test_ui_state();
        ui_state.history = vec!["x = 1".to_string(), "x + 1".to_string()];
        ui_state.history_index = Some(1);
        ui_state.python_input = "x + 1".to_string();
        let timeline_area = Rect::new(0, 0, 80, 8);

        handle_mouse_event(
            &mut ui_state,
            mouse_event(MouseEventKind::ScrollUp, 3, 3),
            timeline_area,
            20,
        );

        assert_eq!(ui_state.history_index, Some(1));
        assert_eq!(ui_state.python_input, "x + 1");
    }

    #[test]
    fn timeline_manual_scroll_is_preserved_when_new_output_arrives() {
        let mut ui_state = test_ui_state();
        ui_state.timeline_scroll = 5;
        ui_state.push_timeline_output(OutputKind::PythonStdout, "hello");
        ui_state.push_timeline_output(OutputKind::PythonStdout, "world");
        assert_eq!(ui_state.timeline_scroll, 5);
    }

    #[test]
    fn timeline_max_scroll_matches_content_and_viewport() {
        assert_eq!(timeline_max_scroll(0, 10), 0);
        assert_eq!(timeline_max_scroll(5, 10), 0);
        assert_eq!(timeline_max_scroll(11, 10), 1);
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
                .any(|line| line == "missing file argument. usage: /run <file>"),
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

    #[test]
    fn initialize_timeline_includes_startup_message_when_present() {
        let dir = tempdir().expect("tempdir");
        let state = AppState {
            startup_message: Some("Startup file /tmp/startup.py was executed".to_string()),
            ..test_app_state("startup-message", dir.path())
        };
        let mut ui_state = test_ui_state();

        super::initialize_timeline(&state, &mut ui_state);

        let lines = timeline_text_lines(&ui_state);
        assert!(
            lines
                .iter()
                .any(|line| line == "Startup file /tmp/startup.py was executed"),
            "startup message should be visible in timeline"
        );
    }

    #[cfg(feature = "test-support")]
    #[tokio::test]
    async fn test_support_harness_renders_and_toggles_mode() {
        use super::test_support::{UiHarness, deterministic_app_state};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let state = deterministic_app_state("phase2-ui").expect("deterministic app state");
        let mut harness = UiHarness::new(80, 20, state).expect("harness");
        harness.render().expect("render");

        let initial = harness.buffer_snapshot();
        assert!(initial.contains("Welcome to PyChat.ai"));
        assert!(initial.contains("Mode: Python"));
        assert!(initial.contains("PyChat.ai | Session: phase2-ui"));

        harness
            .send_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .expect("tab");
        harness.render().expect("render after tab");

        let toggled = harness.buffer_snapshot();
        assert!(toggled.contains("Mode: AI Assistant"));
        assert!(harness.ui_state_view().prompt.contains("ai> "));
    }

    #[cfg(feature = "test-support")]
    #[test]
    fn test_support_env_defaults_no_color_and_isolates_xdg_dirs() {
        use super::test_support::{deterministic_app_state_with_env, deterministic_test_env};

        let env = deterministic_test_env().expect("deterministic env");
        assert_eq!(env.no_color, "1");
        assert!(env.xdg_config_home.exists());
        assert!(env.xdg_state_home.exists());

        let (_state, env_for_state) =
            deterministic_app_state_with_env("phase2-env").expect("state with env");
        let trace_root = env_for_state
            .xdg_state_home
            .join("pychat.ai")
            .join("traces");
        assert!(trace_root.exists());
    }

    fn test_app_state(session_id: &str, trace_dir: &std::path::Path) -> AppState {
        AppState {
            mode: Mode::Python,
            session_id: session_id.to_string(),
            python: PythonSession::initialize().expect("python"),
            llm: None,
            agent_config: AgentConfig::default(),
            theme_config: ThemeConfig::default(),
            startup_message: None,
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

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }
}
