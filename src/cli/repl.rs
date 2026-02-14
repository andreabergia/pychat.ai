use crate::llm::{
    ASSISTANT_SYSTEM_PROMPT,
    gemini::GeminiProvider,
    provider::{AssistantInput, LlmProvider},
};
use crate::python::{PythonSession, UserRunResult};
use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::{
    Cmd, ConditionalEventHandler, Editor, Event, EventContext, EventHandler, KeyEvent, RepeatCount,
};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Python,
    Assistant,
}

pub struct AppState {
    pub mode: Mode,
    pub python: PythonSession,
    pub llm: Option<GeminiProvider>,
}

#[derive(Default)]
struct TabCaptureState {
    initial: Option<(String, String)>,
}

struct TabModeToggleHandler {
    state: Arc<Mutex<TabCaptureState>>,
}

impl ConditionalEventHandler for TabModeToggleHandler {
    fn handle(
        &self,
        _evt: &Event,
        _n: RepeatCount,
        _positive: bool,
        ctx: &EventContext,
    ) -> Option<Cmd> {
        let line = ctx.line();
        let pos = ctx.pos();
        let (left, right) = line.split_at(pos);

        if let Ok(mut state) = self.state.lock() {
            state.initial = Some((left.to_string(), right.to_string()));
        }

        Some(Cmd::Interrupt)
    }
}

pub fn prompt_for(mode: Mode) -> &'static str {
    match mode {
        Mode::Python => "py> ",
        Mode::Assistant => "ai> ",
    }
}

pub async fn run_repl(state: &mut AppState) -> Result<()> {
    let mut rl = Editor::<(), rustyline::history::DefaultHistory>::new()?;
    let tab_capture_state = Arc::new(Mutex::new(TabCaptureState::default()));
    rl.bind_sequence(
        KeyEvent::from('\t'),
        EventHandler::Conditional(Box::new(TabModeToggleHandler {
            state: Arc::clone(&tab_capture_state),
        })),
    );

    loop {
        let initial = {
            let mut capture = tab_capture_state
                .lock()
                .expect("tab capture mutex poisoned");
            capture.initial.take()
        };

        let line_result = if let Some((left, right)) = initial {
            rl.readline_with_initial(prompt_for(state.mode), (&left, &right))
        } else {
            rl.readline(prompt_for(state.mode))
        };

        match line_result {
            Ok(line) => {
                let line = line.trim();
                if line.eq_ignore_ascii_case("exit") || line.eq_ignore_ascii_case("quit") {
                    break;
                }

                if line.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(line);

                handle_line(state, line).await;
            }
            Err(ReadlineError::Interrupted) => {
                let should_toggle = {
                    let capture = tab_capture_state
                        .lock()
                        .expect("tab capture mutex poisoned");
                    capture.initial.is_some()
                };

                if should_toggle {
                    state.mode = toggle_mode(state.mode);
                    repaint_previous_line()?;
                }
                continue;
            }
            Err(ReadlineError::Eof) => break,
            Err(err) => return Err(err.into()),
        }
    }

    Ok(())
}

fn repaint_previous_line() -> Result<()> {
    // readline writes a newline after interrupt; move up and clear it so the next
    // prompt redraw keeps the user on the same logical line.
    let mut stdout = io::stdout().lock();
    stdout.write_all(b"\x1b[1A\x1b[2K\r")?;
    stdout.flush()?;
    Ok(())
}

fn toggle_mode(mode: Mode) -> Mode {
    match mode {
        Mode::Python => Mode::Assistant,
        Mode::Assistant => Mode::Python,
    }
}

async fn handle_line(state: &mut AppState, line: &str) {
    match state.mode {
        Mode::Python => match state.python.run_user_input(line) {
            Ok(UserRunResult::Evaluated(result)) => {
                if !result.stdout.is_empty() {
                    print!("{}", result.stdout);
                }
                if !result.stderr.is_empty() {
                    eprint!("{}", result.stderr);
                }
                println!("{}", result.value_repr);
            }
            Ok(UserRunResult::Executed(result)) => {
                if !result.stdout.is_empty() {
                    print!("{}", result.stdout);
                }
                if !result.stderr.is_empty() {
                    eprint!("{}", result.stderr);
                }
            }
            Ok(UserRunResult::Failed {
                stdout,
                stderr,
                exception,
            }) => {
                if !stdout.is_empty() {
                    print!("{stdout}");
                }
                if !stderr.is_empty() {
                    eprint!("{stderr}");
                }
                eprint!("{}", exception.traceback);
            }
            Err(err) => {
                println!("error: {err}");
            }
        },
        Mode::Assistant => {
            let Some(provider) = &state.llm else {
                println!(
                    "Assistant unavailable: missing GEMINI_API_KEY. Configure it in your shell or .env file (example: GEMINI_API_KEY=your_key)."
                );
                return;
            };

            match provider
                .generate(AssistantInput {
                    user_message: line.to_string(),
                    system_instruction: Some(ASSISTANT_SYSTEM_PROMPT.trim().to_string()),
                })
                .await
            {
                Ok(output) => println!("{}", output.text),
                Err(err) => println!("Assistant request failed: {err}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Mode, prompt_for, toggle_mode};

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
}
