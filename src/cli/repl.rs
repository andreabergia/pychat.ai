use crate::python::PythonSession;
use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::{Cmd, Editor, EventHandler, KeyEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Python,
    Assistant,
}

pub struct AppState {
    pub mode: Mode,
    pub python: PythonSession,
}

pub fn prompt_for(mode: Mode) -> &'static str {
    match mode {
        Mode::Python => "py> ",
        Mode::Assistant => "ai> ",
    }
}

pub fn run_repl(state: &mut AppState) -> Result<()> {
    let mut rl = Editor::<(), rustyline::history::DefaultHistory>::new()?;
    rl.bind_sequence(
        KeyEvent::from('\t'),
        EventHandler::Simple(Cmd::Interrupt),
    );

    loop {
        match rl.readline(prompt_for(state.mode)) {
            Ok(line) => {
                let line = line.trim();
                if line.eq_ignore_ascii_case("exit") || line.eq_ignore_ascii_case("quit") {
                    break;
                }

                if line.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(line);

                handle_line(state, line);
            }
            Err(ReadlineError::Interrupted) => {
                state.mode = toggle_mode(state.mode);
                continue;
            }
            Err(ReadlineError::Eof) => break,
            Err(err) => return Err(err.into()),
        }
    }

    Ok(())
}

fn toggle_mode(mode: Mode) -> Mode {
    match mode {
        Mode::Python => Mode::Assistant,
        Mode::Assistant => Mode::Python,
    }
}

fn handle_line(state: &mut AppState, line: &str) {
    match state.mode {
        Mode::Python => {
            if let Err(err) = state.python.run_line(line) {
                println!("error: {err}");
            }
        }
        Mode::Assistant => {
            println!("Assistant placeholder: not implemented yet.");
        }
    }
}
