mod cli;
mod config;
mod llm;
mod python;

use anyhow::Result;
use cli::{AppState, Mode, run_repl};
use config::AppConfig;
use llm::gemini::GeminiProvider;
use python::PythonSession;

#[tokio::main]
async fn main() -> Result<()> {
    let config = AppConfig::from_env();
    let python = PythonSession::initialize()?;
    let llm = GeminiProvider::new(
        reqwest::Client::new(),
        config.gemini_api_key.clone(),
        config.gemini_model.clone(),
        config.gemini_base_url.clone(),
    )
    .ok();

    let mut app_state = AppState {
        mode: Mode::Python,
        python,
        llm,
    };

    run_repl(&mut app_state).await
}
