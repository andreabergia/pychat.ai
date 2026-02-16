use crate::agent::{AgentConfig, run_question};
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
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Padding, Paragraph, Wrap};
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
    SystemInfo,
    SystemError,
}

#[derive(Debug, Clone)]
struct OutputEntry {
    kind: OutputKind,
    text: String,
}

#[derive(Debug, Clone)]
struct UiState {
    mode: Mode,
    python_input: String,
    assistant_input: String,
    history: Vec<String>,
    history_index: Option<usize>,
    scrollback: Vec<OutputEntry>,
    should_quit: bool,
    theme: Theme,
}

impl UiState {
    fn new(mode: Mode, color_enabled: bool) -> Self {
        Self {
            mode,
            python_input: String::new(),
            assistant_input: String::new(),
            history: Vec::new(),
            history_index: None,
            scrollback: Vec::new(),
            should_quit: false,
            theme: Theme::new(color_enabled),
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

    fn push_output(&mut self, kind: OutputKind, text: &str) {
        for line in split_output_lines(text) {
            self.scrollback.push(OutputEntry {
                kind,
                text: line.to_string(),
            });
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

#[derive(Debug, Clone, Copy)]
struct Theme {
    enabled: bool,
}

impl Theme {
    fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    fn prompt_style(&self, mode: Mode) -> Style {
        if !self.enabled {
            return Style::default().add_modifier(Modifier::BOLD);
        }

        match mode {
            Mode::Python => Style::default()
                .fg(Color::Rgb(158, 206, 106))
                .add_modifier(Modifier::BOLD),
            Mode::Assistant => Style::default()
                .fg(Color::Rgb(122, 162, 247))
                .add_modifier(Modifier::BOLD),
        }
    }

    fn output_style(&self, kind: OutputKind) -> Style {
        if !self.enabled {
            return Style::default();
        }

        match kind {
            OutputKind::UserInputPython => Style::default()
                .fg(Color::Rgb(158, 206, 106))
                .add_modifier(Modifier::DIM),
            OutputKind::UserInputAssistant => Style::default()
                .fg(Color::Rgb(122, 162, 247))
                .add_modifier(Modifier::DIM),
            OutputKind::PythonValue => Style::default().fg(Color::Rgb(224, 175, 104)),
            OutputKind::PythonStdout => Style::default().fg(Color::Rgb(192, 202, 245)),
            OutputKind::PythonStderr => Style::default().fg(Color::Rgb(255, 158, 100)),
            OutputKind::PythonTraceback => Style::default()
                .fg(Color::Rgb(247, 118, 142))
                .add_modifier(Modifier::BOLD),
            OutputKind::AssistantText => Style::default().fg(Color::Rgb(125, 207, 255)),
            OutputKind::SystemInfo => Style::default().fg(Color::Rgb(86, 95, 137)),
            OutputKind::SystemError => Style::default()
                .fg(Color::Rgb(247, 118, 142))
                .add_modifier(Modifier::BOLD),
        }
    }

    fn status_style(&self) -> Style {
        if self.enabled {
            Style::default().fg(Color::Rgb(86, 95, 137))
        } else {
            Style::default()
        }
    }

    fn input_block_style(&self) -> Style {
        if self.enabled {
            Style::default()
                .bg(Color::Rgb(22, 22, 30))
                .fg(Color::Rgb(169, 177, 214))
        } else {
            Style::default()
        }
    }
}

pub async fn run_repl(state: &mut AppState) -> Result<()> {
    let color_enabled = resolve_color_enabled();
    let mut ui_state = UiState::new(state.mode, color_enabled);

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
            handle_key_event(state, ui_state, key).await;
        }
    }

    state.mode = ui_state.mode;
    Ok(())
}

async fn handle_key_event(state: &mut AppState, ui_state: &mut UiState, key: KeyEvent) {
    match key.code {
        KeyCode::Tab => {
            ui_state.mode = toggle_mode(ui_state.mode);
            ui_state.history_index = None;
        }
        KeyCode::Enter => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                insert_python_newline(ui_state);
            } else {
                handle_enter(state, ui_state).await;
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
        KeyCode::Char(ch) => {
            ui_state.current_input_mut().push(ch);
            ui_state.history_index = None;
        }
        _ => {}
    }
}

async fn handle_enter(state: &mut AppState, ui_state: &mut UiState) {
    if ui_state.mode != Mode::Python {
        submit_current_line(state, ui_state).await;
        return;
    }

    if ui_state.current_input().trim().is_empty() {
        submit_current_line(state, ui_state).await;
        return;
    }

    match state
        .python
        .check_input_completeness(ui_state.current_input())
    {
        Ok(InputCompleteness::Incomplete) => insert_python_newline(ui_state),
        Ok(InputCompleteness::Complete) | Ok(InputCompleteness::Invalid) => {
            submit_current_line(state, ui_state).await;
        }
        Err(err) => {
            ui_state.push_output(
                OutputKind::SystemError,
                &format!("error checking python input completeness: {err}"),
            );
            submit_current_line(state, ui_state).await;
        }
    }
}

fn insert_python_newline(ui_state: &mut UiState) {
    if ui_state.mode != Mode::Python {
        return;
    }
    append_newline_with_indent(&mut ui_state.python_input);
    ui_state.history_index = None;
}

async fn submit_current_line(state: &mut AppState, ui_state: &mut UiState) {
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
        return;
    }

    if line.eq_ignore_ascii_case("exit") || line.eq_ignore_ascii_case("quit") {
        ui_state.should_quit = true;
        return;
    }

    let prompt = prompt_for(ui_state.mode);
    let input_kind = match ui_state.mode {
        Mode::Python => OutputKind::UserInputPython,
        Mode::Assistant => OutputKind::UserInputAssistant,
    };
    ui_state.push_output(input_kind, &format!("{prompt}{line}"));
    ui_state.push_history(&line);

    match ui_state.mode {
        Mode::Python => match state.python.run_user_input(&line) {
            Ok(UserRunResult::Evaluated(result)) => {
                if !result.stdout.is_empty() {
                    ui_state.push_output(OutputKind::PythonStdout, &result.stdout);
                }
                if !result.stderr.is_empty() {
                    ui_state.push_output(OutputKind::PythonStderr, &result.stderr);
                }
                ui_state.push_output(OutputKind::PythonValue, &result.value_repr);
            }
            Ok(UserRunResult::Executed(result)) => {
                if !result.stdout.is_empty() {
                    ui_state.push_output(OutputKind::PythonStdout, &result.stdout);
                }
                if !result.stderr.is_empty() {
                    ui_state.push_output(OutputKind::PythonStderr, &result.stderr);
                }
            }
            Ok(UserRunResult::Failed {
                stdout,
                stderr,
                exception,
            }) => {
                if !stdout.is_empty() {
                    ui_state.push_output(OutputKind::PythonStdout, &stdout);
                }
                if !stderr.is_empty() {
                    ui_state.push_output(OutputKind::PythonStderr, &stderr);
                }
                ui_state.push_output(OutputKind::PythonTraceback, &exception.traceback);
            }
            Err(err) => {
                ui_state.push_output(OutputKind::SystemError, &format!("error: {err}"));
            }
        },
        Mode::Assistant => {
            let Some(provider) = &state.llm else {
                ui_state.push_output(
                    OutputKind::SystemError,
                    "Assistant unavailable: missing GEMINI_API_KEY. Configure it in your shell or .env file (example: GEMINI_API_KEY=your_key).",
                );
                return;
            };

            match run_question(provider, &state.python, &line, &state.agent_config).await {
                Ok(answer) => ui_state.push_output(OutputKind::AssistantText, &answer.text),
                Err(err) => ui_state.push_output(
                    OutputKind::SystemError,
                    &format!("Assistant request failed: {err}"),
                ),
            }
        }
    }
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

    let mut lines = Vec::with_capacity(ui_state.scrollback.len().max(1));
    if ui_state.scrollback.is_empty() {
        lines.push(Line::from(Span::styled(
            "Welcome to PyAIChat. TAB toggles Python/AI mode.",
            ui_state.theme.output_style(OutputKind::SystemInfo),
        )));
    } else {
        for entry in &ui_state.scrollback {
            lines.push(Line::from(Span::styled(
                entry.text.clone(),
                ui_state.theme.output_style(entry.kind),
            )));
        }
    }

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
            Span::styled(prompt, ui_state.theme.prompt_style(ui_state.mode))
        } else {
            Span::styled(
                prompt_padding.clone(),
                ui_state.theme.prompt_style(ui_state.mode),
            )
        };
        rendered_lines.push(Line::from(vec![
            prompt_span,
            Span::styled(line, ui_state.theme.input_block_style()),
        ]));
    }

    let input_widget = Paragraph::new(rendered_lines)
        .block(
            Block::default()
                .padding(Padding::new(1, 1, 1, 1))
                .style(ui_state.theme.input_block_style()),
        )
        .wrap(Wrap { trim: false })
        .scroll((input_scroll, 0));
    frame.render_widget(input_widget, chunks[1]);

    let mode_text = match ui_state.mode {
        Mode::Python => "Mode: Python",
        Mode::Assistant => "Mode: AI Assistant",
    };
    let status_left = Paragraph::new(mode_text).style(ui_state.theme.status_style());
    frame.render_widget(status_left, chunks[2]);

    let status_right = Paragraph::new("PyAiChat")
        .style(ui_state.theme.status_style())
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

fn split_output_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }

    text.lines().collect()
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

#[cfg(test)]
mod tests {
    use super::{
        Mode, append_newline_with_indent, input_cursor_position, last_line_indent, prompt_for,
        resolve_color_enabled_with, split_output_lines, toggle_mode,
    };

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
}
