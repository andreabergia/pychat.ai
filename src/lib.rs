pub mod agent;
pub mod cli;
pub mod config;
pub mod http;
pub mod llm;
pub mod python;
pub mod trace;

use agent::AgentConfig;
use anyhow::Result;
use cli::{AppState, CliArgs, Mode, run_repl};
use config::AppConfig;
use http::client::HttpClient;
use llm::gemini::GeminiProvider;
use python::PythonSession;
use std::time::{SystemTime, UNIX_EPOCH};
use trace::SessionTrace;

pub async fn run(args: CliArgs) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::generate_session_id;

    #[test]
    fn generated_session_id_has_expected_shape() {
        let session_id = generate_session_id();
        let mut parts = session_id.split('-');
        let ts = parts.next().expect("timestamp segment");
        let pid = parts.next().expect("pid segment");
        assert!(
            parts.next().is_none(),
            "session id should contain one delimiter"
        );
        assert!(!ts.is_empty(), "timestamp segment should not be empty");
        assert!(!pid.is_empty(), "pid segment should not be empty");
        assert!(
            ts.chars().all(|ch| ch.is_ascii_hexdigit()),
            "timestamp segment should be hex"
        );
        assert!(
            pid.chars().all(|ch| ch.is_ascii_hexdigit()),
            "pid segment should be hex"
        );
    }
}
