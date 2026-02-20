mod agent;
mod cli;
mod config;
mod http;
mod llm;
mod python;

use agent::AgentConfig;
use anyhow::Result;
use clap::Parser;
use cli::{AppState, CliArgs, Mode, run_repl};
use config::AppConfig;
use http::{client::HttpClient, debug::HttpDebugConfig};
use llm::gemini::GeminiProvider;
use python::PythonSession;

#[tokio::main]
async fn main() -> Result<()> {
    let args = CliArgs::parse();
    let config = AppConfig::load()?;
    let python = PythonSession::initialize()?;
    let http = HttpClient::new(
        reqwest::Client::new(),
        HttpDebugConfig::from_verbose(args.verbose),
    );
    let llm = GeminiProvider::new(
        http,
        config.gemini_api_key.clone(),
        config.gemini_model.clone(),
        config.gemini_base_url.clone(),
    )
    .ok();

    let mut app_state = AppState {
        mode: Mode::Python,
        python,
        llm,
        agent_config: AgentConfig::default(),
        theme_config: config.theme.clone(),
    };

    run_repl(&mut app_state).await
}
