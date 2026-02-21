pub mod agent;
pub mod cli;
pub mod config;
pub mod http;
pub mod llm;
pub mod python;
pub mod trace;

use agent::AgentConfig;
use anyhow::{Result, anyhow, bail};
use cli::{AppState, CliArgs, Mode, run_repl};
use config::AppConfig;
use http::client::HttpClient;
use llm::gemini::GeminiProvider;
use python::{PythonSession, UserRunResult};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use trace::SessionTrace;

pub async fn run(args: CliArgs) -> Result<()> {
    let config = if let Some(path) = args.config.as_deref() {
        AppConfig::load_with_path(Some(path))?
    } else {
        AppConfig::load()?
    };
    let python = PythonSession::initialize()?;
    let startup_message = run_startup_script_if_configured(&python, &config)?;
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
        startup_message,
        trace,
    };

    run_repl(&mut app_state).await
}

fn run_startup_script_if_configured(
    python: &PythonSession,
    config: &AppConfig,
) -> Result<Option<String>> {
    let Some(path) = startup_script_path(config)? else {
        return Ok(None);
    };

    let source = fs::read_to_string(&path).map_err(|err| {
        anyhow!(
            "Failed to load startup file {}: unable to read file: {err}",
            path.display()
        )
    })?;

    match python.run_exec_input(&source)? {
        UserRunResult::Executed(_) => Ok(Some(format!(
            "Startup file {} was executed",
            path.display()
        ))),
        UserRunResult::Failed { exception, .. } => {
            bail!(
                "Failed to execute startup file {}:\n{}",
                path.display(),
                exception.traceback
            )
        }
        UserRunResult::Evaluated(_) => {
            bail!("internal error: startup script unexpectedly evaluated expression")
        }
    }
}

fn startup_script_path(config: &AppConfig) -> Result<Option<PathBuf>> {
    if let Some(path) = &config.startup_file {
        return Ok(Some(path.clone()));
    }

    if config.config_is_explicit {
        return Ok(None);
    }

    let config_dir = config.config_path.parent().ok_or_else(|| {
        anyhow!(
            "Failed to resolve startup file path from config {}: missing parent directory",
            config.config_path.display()
        )
    })?;
    let implicit_startup = config_dir.join("startup.py");
    if is_regular_file(&implicit_startup) {
        Ok(Some(implicit_startup))
    } else {
        Ok(None)
    }
}

fn is_regular_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn generate_session_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    format!("{millis:x}-{:x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::{generate_session_id, run_startup_script_if_configured, startup_script_path};
    use crate::config::{AppConfig, ThemeConfig};
    use crate::python::PythonSession;
    use std::fs;
    use std::path::PathBuf;

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

    #[test]
    fn startup_script_path_uses_implicit_startup_when_not_explicit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("pychat.ai");
        fs::create_dir_all(&config_dir).expect("create config dir");
        let startup = config_dir.join("startup.py");
        fs::write(&startup, "x = 1\n").expect("write startup.py");

        let cfg = AppConfig {
            config_path: config_dir.join("config.toml"),
            config_is_explicit: false,
            gemini_api_key: None,
            gemini_model: "model".to_string(),
            gemini_base_url: "https://example.com".to_string(),
            startup_file: None,
            theme: ThemeConfig::default(),
        };

        let selected = startup_script_path(&cfg).expect("select startup");
        assert_eq!(selected, Some(startup));
    }

    #[test]
    fn startup_script_path_skips_implicit_startup_when_config_is_explicit() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("pychat.ai");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(config_dir.join("startup.py"), "x = 1\n").expect("write startup.py");

        let cfg = AppConfig {
            config_path: config_dir.join("config.toml"),
            config_is_explicit: true,
            gemini_api_key: None,
            gemini_model: "model".to_string(),
            gemini_base_url: "https://example.com".to_string(),
            startup_file: None,
            theme: ThemeConfig::default(),
        };

        let selected = startup_script_path(&cfg).expect("select startup");
        assert_eq!(selected, None);
    }

    #[test]
    fn run_startup_script_executes_and_reports_message() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let startup_path = tmp.path().join("startup.py");
        fs::write(&startup_path, "answer = 42\n").expect("write startup file");

        let cfg = AppConfig {
            config_path: tmp.path().join("config.toml"),
            config_is_explicit: true,
            gemini_api_key: None,
            gemini_model: "model".to_string(),
            gemini_base_url: "https://example.com".to_string(),
            startup_file: Some(startup_path.clone()),
            theme: ThemeConfig::default(),
        };
        let python = PythonSession::initialize().expect("python session");

        let message = run_startup_script_if_configured(&python, &cfg).expect("startup runs");
        assert_eq!(
            message,
            Some(format!(
                "Startup file {} was executed",
                startup_path.display()
            ))
        );
        assert_eq!(
            python.eval_expr("answer").expect("read answer").value_repr,
            "42"
        );
    }

    #[test]
    fn run_startup_script_fails_on_python_exception() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let startup_path = tmp.path().join("startup.py");
        fs::write(&startup_path, "1 / 0\n").expect("write startup file");

        let cfg = AppConfig {
            config_path: PathBuf::from("config.toml"),
            config_is_explicit: true,
            gemini_api_key: None,
            gemini_model: "model".to_string(),
            gemini_base_url: "https://example.com".to_string(),
            startup_file: Some(startup_path.clone()),
            theme: ThemeConfig::default(),
        };
        let python = PythonSession::initialize().expect("python session");

        let err = run_startup_script_if_configured(&python, &cfg).expect_err("startup fails");
        assert!(err.to_string().contains("Failed to execute startup file"));
        assert!(err.to_string().contains("ZeroDivisionError"));
    }
}
