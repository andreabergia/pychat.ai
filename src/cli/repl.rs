use crate::agent::{AgentConfig, run_question};
use crate::llm::gemini::GeminiProvider;
use crate::python::{PythonSession, UserRunResult};
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
                .fg(Color::Indexed(41))
                .add_modifier(Modifier::BOLD),
            Mode::Assistant => Style::default()
                .fg(Color::Indexed(33))
                .add_modifier(Modifier::BOLD),
        }
    }

    fn output_style(&self, kind: OutputKind) -> Style {
        if !self.enabled {
            return Style::default();
        }

        match kind {
            OutputKind::UserInputPython => Style::default().fg(Color::Indexed(29)),
            OutputKind::UserInputAssistant => Style::default().fg(Color::Indexed(24)),
            OutputKind::PythonValue => Style::default().fg(Color::Indexed(220)),
            OutputKind::PythonStdout => Style::default().fg(Color::Indexed(252)),
            OutputKind::PythonStderr => Style::default().fg(Color::Indexed(208)),
            OutputKind::PythonTraceback => Style::default()
                .fg(Color::Indexed(196))
                .add_modifier(Modifier::BOLD),
            OutputKind::AssistantText => Style::default().fg(Color::Indexed(118)),
            OutputKind::SystemInfo => Style::default().fg(Color::Indexed(245)),
            OutputKind::SystemError => Style::default()
                .fg(Color::Indexed(203))
                .add_modifier(Modifier::BOLD),
        }
    }

    fn status_style(&self) -> Style {
        if self.enabled {
            Style::default().fg(Color::Indexed(244))
        } else {
            Style::default()
        }
    }

    fn input_block_style(&self) -> Style {
        if self.enabled {
            Style::default().bg(Color::Indexed(236))
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
            submit_current_line(state, ui_state).await;
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
        KeyCode::Char(ch) => {
            ui_state.current_input_mut().push(ch);
            ui_state.history_index = None;
        }
        _ => {}
    }
}

async fn submit_current_line(state: &mut AppState, ui_state: &mut UiState) {
    let line = ui_state.current_input_mut().trim().to_string();
    ui_state.current_input_mut().clear();

    if line.is_empty() {
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(3),
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

    let prompt = prompt_for(ui_state.mode);
    let input = ui_state.current_input();
    let input_line = Line::from(vec![
        Span::styled(prompt, ui_state.theme.prompt_style(ui_state.mode)),
        Span::styled(input, ui_state.theme.input_block_style()),
    ]);
    let input_widget = Paragraph::new(input_line)
        .block(
            Block::default()
                .padding(Padding::new(1, 1, 1, 1))
                .style(ui_state.theme.input_block_style()),
        )
        .wrap(Wrap { trim: false });
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

    let cursor_x = chunks[1]
        .x
        .saturating_add(1)
        .saturating_add(u16::try_from(prompt.chars().count()).unwrap_or(u16::MAX))
        .saturating_add(u16::try_from(input.chars().count()).unwrap_or(u16::MAX));
    let cursor_y = chunks[1].y.saturating_add(1);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn split_output_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }

    text.lines().collect()
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
    use super::{Mode, prompt_for, resolve_color_enabled_with, split_output_lines, toggle_mode};

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
}
