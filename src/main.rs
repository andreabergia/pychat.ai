mod cli;
mod python;

use anyhow::Result;
use cli::{AppState, Mode, run_repl};
use python::PythonSession;

fn main() -> Result<()> {
    let python = PythonSession::initialize()?;
    let mut app_state = AppState {
        mode: Mode::Python,
        python,
    };

    run_repl(&mut app_state)
}
