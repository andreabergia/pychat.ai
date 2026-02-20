use crate::agent::{AgentConfig, AgentProgressEvent, run_question_with_events};
use crate::cli::theme::Theme;
use crate::config::{ThemeConfig, ThemeToken};
use crate::llm::gemini::GeminiProvider;
use crate::python::{InputCompleteness, PythonSession, UserRunResult};
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
use std::io::{self, IsTerminal};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Python,
    Assistant,
}

pub struct AppState {
    pub mode: Mode,
    pub python: PythonSession,
    pub llm: Option<GeminiProvider>,
    pub agent_config: AgentConfig,
    pub theme_config: ThemeConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputKind {
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
enum TimelineEntry {
    UserInputPython(String),
    OutputLine { kind: OutputKind, text: String },
    AssistantTurn(AssistantTurn),
}

#[derive(Debug, Clone)]
struct AssistantTurn {
    prompt: String,
    events: Vec<AssistantStepEvent>,
    state: AssistantTurnState,
}

#[derive(Debug, Clone)]
enum AssistantTurnState {
    InFlight,
    CompletedText(String),
    CompletedError(String),
}

#[derive(Debug, Clone)]
enum AssistantStepEvent {
    ToolRequest { text: String },
    ToolResult { text: String },
}

#[derive(Debug, Clone)]
struct UiState {
    mode: Mode,
    python_input: String,
    assistant_input: String,
    show_assistant_steps: bool,
    history: Vec<String>,
    history_index: Option<usize>,
    timeline: Vec<TimelineEntry>,
    should_quit: bool,
    theme: Theme,
}

impl UiState {
    fn new(mode: Mode, color_enabled: bool, theme_config: &ThemeConfig) -> Self {
        Self {
            mode,
            python_input: String::new(),
            assistant_input: String::new(),
            show_assistant_steps: true,
            history: Vec::new(),
            history_index: None,
            timeline: Vec::new(),
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
        for line in split_output_lines(text) {
            self.timeline.push(TimelineEntry::OutputLine {
                kind,
                text: line.to_string(),
            });
        }
    }

    fn push_user_input(&mut self, text: &str) {
        for line in split_output_lines(text) {
            self.timeline
                .push(TimelineEntry::UserInputPython(line.to_string()));
        }
    }

    fn push_assistant_turn(&mut self, prompt: String) -> usize {
        let index = self.timeline.len();
        self.timeline
            .push(TimelineEntry::AssistantTurn(AssistantTurn {
                prompt,
                events: Vec::new(),
                state: AssistantTurnState::InFlight,
            }));
        index
    }

    fn assistant_turn_mut(&mut self, index: usize) -> Option<&mut AssistantTurn> {
        match self.timeline.get_mut(index) {
            Some(TimelineEntry::AssistantTurn(turn)) => Some(turn),
            _ => None,
        }
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
    let mut ui_state = UiState::new(state.mode, color_enabled, &state.theme_config);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let run_result = run_tui_loop(&mut terminal, state, &mut ui_state).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

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
            ui_state.push_timeline_output(
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

    if ui_state.mode == Mode::Python {
        ui_state.push_user_input(&line);
    }
    ui_state.push_history(&line);

    match ui_state.mode {
        Mode::Python => match state.python.run_user_input(&line) {
            Ok(UserRunResult::Evaluated(result)) => {
                if !result.stdout.is_empty() {
                    ui_state.push_timeline_output(OutputKind::PythonStdout, &result.stdout);
                }
                if !result.stderr.is_empty() {
                    ui_state.push_timeline_output(OutputKind::PythonStderr, &result.stderr);
                }
                ui_state.push_timeline_output(OutputKind::PythonValue, &result.value_repr);
            }
            Ok(UserRunResult::Executed(result)) => {
                if !result.stdout.is_empty() {
                    ui_state.push_timeline_output(OutputKind::PythonStdout, &result.stdout);
                }
                if !result.stderr.is_empty() {
                    ui_state.push_timeline_output(OutputKind::PythonStderr, &result.stderr);
                }
            }
            Ok(UserRunResult::Failed {
                stdout,
                stderr,
                exception,
            }) => {
                if !stdout.is_empty() {
                    ui_state.push_timeline_output(OutputKind::PythonStdout, &stdout);
                }
                if !stderr.is_empty() {
                    ui_state.push_timeline_output(OutputKind::PythonStderr, &stderr);
                }
                ui_state.push_timeline_output(OutputKind::PythonTraceback, &exception.traceback);
            }
            Err(err) => {
                ui_state.push_timeline_output(OutputKind::SystemError, &format!("error: {err}"));
            }
        },
        Mode::Assistant => {
            let Some(provider) = &state.llm else {
                ui_state.push_timeline_output(
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
                    if let Some(turn) = ui_state.assistant_turn_mut(turn_index) {
                        turn.state = AssistantTurnState::CompletedText(answer.text);
                    }
                }
                Err(err) => {
                    let message = format!("Assistant request failed: {err}");
                    if let Some(turn) = ui_state.assistant_turn_mut(turn_index) {
                        turn.state = AssistantTurnState::CompletedError(message);
                    }
                }
            };
        }
    }

    Ok(())
}

fn draw_ui(frame: &mut ratatui::Frame<'_>, ui_state: &UiState) {
    let prompt = prompt_for(ui_state.mode);
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

    let lines = render_timeline_lines(ui_state);

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
                ui_state.theme.style(prompt_token_for(ui_state.mode)),
            )
        } else {
            Span::styled(
                prompt_padding.clone(),
                ui_state.theme.style(prompt_token_for(ui_state.mode)),
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

    let status_right = Paragraph::new("PyAiChat")
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

fn render_timeline_lines(ui_state: &UiState) -> Vec<Line<'static>> {
    if ui_state.timeline.is_empty() {
        return vec![Line::from(Span::styled(
            "Welcome to PyAIChat. TAB toggles Python/AI mode. Ctrl-T toggles showing agent thinking.",
            ui_state
                .theme
                .style(output_token_for(OutputKind::SystemInfo)),
        ))];
    }

    let mut lines = Vec::new();
    for entry in &ui_state.timeline {
        match entry {
            TimelineEntry::UserInputPython(text) => lines.push(Line::from(vec![
                Span::styled("py> ", ui_state.theme.style(ThemeToken::PythonPrompt)),
                Span::styled(
                    text.clone(),
                    ui_state
                        .theme
                        .style(output_token_for(OutputKind::UserInputPython)),
                ),
            ])),
            TimelineEntry::OutputLine { kind, text } => lines.push(Line::from(Span::styled(
                text.clone(),
                ui_state.theme.style(output_token_for(*kind)),
            ))),
            TimelineEntry::AssistantTurn(turn) => lines.extend(render_assistant_turn_lines(
                &ui_state.theme,
                turn,
                ui_state.show_assistant_steps,
            )),
        }
    }

    lines
}

fn render_assistant_turn_lines(
    theme: &Theme,
    turn: &AssistantTurn,
    show_assistant_steps: bool,
) -> Vec<Line<'static>> {
    const THINKING_BLOCK_PADDING: &str = "  ";
    let mut lines = Vec::new();

    lines.push(Line::from(vec![
        Span::styled("ai> ", theme.style(ThemeToken::AssistantPrompt)),
        Span::styled(
            turn.prompt.clone(),
            theme.style(output_token_for(OutputKind::UserInputAssistant)),
        ),
    ]));

    if show_assistant_steps {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw(THINKING_BLOCK_PADDING),
            Span::styled(
                "Thinking...",
                theme.style(output_token_for(OutputKind::AssistantWaiting)),
            ),
        ]));
        for event in &turn.events {
            match event {
                AssistantStepEvent::ToolRequest { text } => {
                    lines.push(Line::from(Span::styled(
                        format!("{THINKING_BLOCK_PADDING}{text}"),
                        theme.style(output_token_for(OutputKind::AssistantProgressRequest)),
                    )));
                }
                AssistantStepEvent::ToolResult { text } => {
                    lines.push(Line::from(Span::styled(
                        format!("{THINKING_BLOCK_PADDING}{text}"),
                        theme.style(output_token_for(OutputKind::AssistantProgressResult)),
                    )));
                }
            }
        }
        lines.push(Line::from(""));
    }

    match &turn.state {
        AssistantTurnState::InFlight => {}
        AssistantTurnState::CompletedText(text) => {
            for line in split_output_lines(text) {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    theme.style(output_token_for(OutputKind::AssistantText)),
                )));
            }
        }
        AssistantTurnState::CompletedError(message) => {
            for line in split_output_lines(message) {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    theme.style(output_token_for(OutputKind::SystemError)),
                )));
            }
        }
    }

    lines
}

fn split_output_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }

    text.lines().collect()
}

fn prompt_token_for(mode: Mode) -> ThemeToken {
    match mode {
        Mode::Python => ThemeToken::PythonPrompt,
        Mode::Assistant => ThemeToken::AssistantPrompt,
    }
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

pub fn prompt_for(mode: Mode) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::{
        AssistantStepEvent, AssistantTurn, AssistantTurnState, Mode, append_newline_with_indent,
        format_tool_error_line, format_tool_request_line, format_tool_result_line,
        input_cursor_position, last_line_indent, mode_status_text, preview_text, prompt_for,
        render_assistant_turn_lines, resolve_color_enabled_with, split_output_lines, toggle_mode,
    };
    use crate::cli::theme::Theme;
    use serde_json::json;

    fn completed_turn_fixture() -> AssistantTurn {
        AssistantTurn {
            prompt: "inspect x".to_string(),
            events: vec![
                AssistantStepEvent::ToolRequest {
                    text: "-> Inspecting: x".to_string(),
                },
                AssistantStepEvent::ToolResult {
                    text: "<- Inspection complete: int".to_string(),
                },
            ],
            state: AssistantTurnState::CompletedText("x is an int".to_string()),
        }
    }

    fn text_lines(lines: Vec<ratatui::text::Line<'static>>) -> Vec<String> {
        lines.into_iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn test_toggle_mode() {
        assert_eq!(toggle_mode(Mode::Python), Mode::Assistant);
        assert_eq!(toggle_mode(Mode::Assistant), Mode::Python);
    }

    #[test]
    fn test_prompt_for() {
        assert_eq!(prompt_for(Mode::Python), "py> ");
        assert_eq!(prompt_for(Mode::Assistant), "ai> ");
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
    fn split_lines_works() {
        assert_eq!(split_output_lines("a\nb\n"), vec!["a", "b"]);
        assert!(split_output_lines("").is_empty());
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
    fn render_assistant_turn_hides_steps_when_toggle_off() {
        let turn = completed_turn_fixture();
        let lines = text_lines(render_assistant_turn_lines(
            &Theme::new(false),
            &turn,
            false,
        ));
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
        let turn = completed_turn_fixture();
        let lines = text_lines(render_assistant_turn_lines(&Theme::new(false), &turn, true));
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
    }

    #[test]
    fn toggle_is_retroactive_for_completed_turn() {
        let turn = completed_turn_fixture();
        let with_steps = text_lines(render_assistant_turn_lines(&Theme::new(false), &turn, true));
        let without_steps = text_lines(render_assistant_turn_lines(
            &Theme::new(false),
            &turn,
            false,
        ));
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
        let turn = AssistantTurn {
            prompt: "inspect y".to_string(),
            events: vec![AssistantStepEvent::ToolRequest {
                text: "-> Inspecting: y".to_string(),
            }],
            state: AssistantTurnState::InFlight,
        };

        let hidden = text_lines(render_assistant_turn_lines(
            &Theme::new(false),
            &turn,
            false,
        ));
        assert!(
            !hidden
                .iter()
                .any(|line| line.starts_with("  -> Inspecting:"))
        );
        assert!(!hidden.iter().any(|line| line == "  Thinking..."));

        let shown = text_lines(render_assistant_turn_lines(&Theme::new(false), &turn, true));
        assert!(shown.iter().any(|line| line == "  Thinking..."));
        assert!(
            shown
                .iter()
                .any(|line| line.starts_with("  -> Inspecting:"))
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
    fn assistant_error_renders_message() {
        let turn = AssistantTurn {
            prompt: "inspect z".to_string(),
            events: Vec::new(),
            state: AssistantTurnState::CompletedError("Assistant request failed: boom".to_string()),
        };

        let lines = text_lines(render_assistant_turn_lines(
            &Theme::new(false),
            &turn,
            false,
        ));
        assert!(
            lines
                .iter()
                .any(|line| line == "Assistant request failed: boom")
        );
    }

    #[test]
    fn thinking_block_has_blank_line_padding() {
        let turn = completed_turn_fixture();
        let lines = text_lines(render_assistant_turn_lines(&Theme::new(false), &turn, true));
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
}
