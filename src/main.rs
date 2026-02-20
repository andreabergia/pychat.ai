mod agent;
mod cli;
mod config;
mod http;
mod llm;
mod python;
mod trace;

use agent::AgentConfig;
use anyhow::Result;
use clap::Parser;
use cli::{AppState, CliArgs, Mode, run_repl};
use config::AppConfig;
use http::client::HttpClient;
use llm::gemini::GeminiProvider;
use python::PythonSession;
use std::time::{SystemTime, UNIX_EPOCH};
use trace::SessionTrace;

#[tokio::main]
async fn main() -> Result<()> {
    let args = CliArgs::parse();
    let config = if let Some(path) = args.config.as_deref() {
        AppConfig::load_with_path(Some(path))?
    } else {
        AppConfig::load()?
    };
    let python = PythonSession::initialize()?;
    let session_id = generate_session_id();
    let trace = SessionTrace::create(&session_id)?;
    let http = HttpClient::new(reqwest::Client::new()).with_trace(trace.clone());
    let llm = GeminiProvider::new(
        http,
        config.gemini_api_key.clone(),
        config.gemini_model.clone(),
        config.gemini_base_url.clone(),
    )
    .ok();

    let mut app_state = AppState {
        mode: Mode::Python,
        session_id,
        python,
        llm,
        agent_config: AgentConfig::default(),
        theme_config: config.theme.clone(),
        trace,
    };

    run_repl(&mut app_state).await
}

fn generate_session_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    format!("{millis:x}-{:x}", std::process::id())
}
