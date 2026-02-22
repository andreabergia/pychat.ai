#![cfg(unix)]

use expectrl::{Eof, Error as ExpectError, Session};
use serial_test::serial;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;
use wiremock::matchers::{body_string_contains, method, path as path_matcher, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const EXPECT_TIMEOUT: Duration = Duration::from_secs(4);
const EXPECT_RETRIES: usize = 3;

#[test]
#[serial]
fn assistant_mode_happy_path_with_mock_provider_writes_response_and_stays_interactive() {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let server = rt.block_on(MockServer::start());
    rt.block_on(async {
        Mock::given(method("POST"))
            .and(path_matcher("/v1beta/models/gemini-test:generateContent"))
            .and(query_param("key", "test-key"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(
                    r#"{
                        "candidates": [
                            {"finishReason":"STOP","content":{"parts":[{"text":"Mock assistant says hello"}]}}
                        ]
                    }"#,
                    "application/json",
                ),
            )
            .mount(&server)
            .await;
    });

    let (mut session, _config_home, state_home, _cfg_dir) = spawn_app_with_mock_provider(&server);
    expect_text(&mut session, "py> ");

    submit_line(&mut session, "/mode ai");
    submit_line(&mut session, "hello assistant");
    thread::sleep(Duration::from_millis(250));

    exit_repl(&mut session);
    let (_trace_path, content) = read_trace_file(&state_home);
    assert!(
        content.contains("hello assistant"),
        "trace content:\n{content}"
    );
    assert!(
        content.contains("Mock assistant says hello"),
        "trace content:\n{content}"
    );
    assert!(
        !content.contains("Assistant unavailable: missing GEMINI_API_KEY"),
        "provider should be enabled by config"
    );
}

#[test]
#[serial]
fn assistant_mode_degraded_failure_then_recovery_allows_next_prompt() {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let server = rt.block_on(MockServer::start());
    rt.block_on(async {
        Mock::given(method("POST"))
            .and(path_matcher("/v1beta/models/gemini-test:generateContent"))
            .and(query_param("key", "test-key"))
            .and(body_string_contains("first question"))
            .respond_with(ResponseTemplate::new(500).set_body_string("provider down"))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path_matcher("/v1beta/models/gemini-test:generateContent"))
            .and(query_param("key", "test-key"))
            .and(body_string_contains("second question"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(
                    r#"{
                        "candidates": [
                            {"finishReason":"STOP","content":{"parts":[{"text":"Recovered answer"}]}}
                        ]
                    }"#,
                    "application/json",
                ),
            )
            .expect(1)
            .mount(&server)
            .await;
    });

    let (mut session, _config_home, state_home, _cfg_dir) = spawn_app_with_mock_provider(&server);
    expect_text(&mut session, "py> ");

    submit_line(&mut session, "/mode ai");

    submit_line(&mut session, "first question");
    thread::sleep(Duration::from_millis(250));

    submit_line(&mut session, "second question");
    thread::sleep(Duration::from_millis(250));

    exit_repl(&mut session);
    let (_trace_path, content) = read_trace_file(&state_home);
    assert!(
        content.contains("first question"),
        "trace content:\n{content}"
    );
    assert!(
        content.contains("second question"),
        "trace content:\n{content}"
    );
    assert!(
        content.contains("Assistant request failed while reasoning")
            || content.contains("provider request failed with status 500"),
        "first prompt should produce a degraded provider failure message"
    );
    assert!(
        content.contains("Recovered answer"),
        "trace content:\n{content}"
    );
}

fn spawn_app_with_mock_provider(server: &MockServer) -> (Session, TempDir, TempDir, TempDir) {
    let config_home = tempfile::tempdir().expect("create XDG_CONFIG_HOME tempdir");
    let state_home = tempfile::tempdir().expect("create XDG_STATE_HOME tempdir");
    let cfg_dir = tempfile::tempdir().expect("config tempdir");
    let cfg_path = write_test_config(cfg_dir.path(), &server.uri());

    let mut command = Command::new(binary_path());
    command
        .arg("--config")
        .arg(&cfg_path)
        .env("NO_COLOR", "1")
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("XDG_STATE_HOME", state_home.path())
        .env("GEMINI_API_KEY", "test-key")
        .env("GEMINI_MODEL", "gemini-test")
        .env("GEMINI_BASE_URL", server.uri());

    let mut session = Session::spawn(command).expect("spawn pychat.ai in PTY");
    session.set_expect_timeout(Some(EXPECT_TIMEOUT));

    (session, config_home, state_home, cfg_dir)
}

fn write_test_config(dir: &Path, base_url: &str) -> PathBuf {
    let path = dir.join("config.toml");
    let content = format!(
        "gemini_api_key = \"test-key\"\n\
         gemini_model = \"gemini-test\"\n\
         gemini_base_url = \"{}\"\n",
        base_url
    );
    fs::write(&path, content).expect("write test config");
    path
}

fn binary_path() -> String {
    std::env::var("CARGO_BIN_EXE_pychat_ai")
        .unwrap_or_else(|_| "target/debug/pychat_ai".to_string())
}

fn submit_line(session: &mut Session, line: &str) {
    session.send(line).expect("send line text");
    session.send([b'\r']).expect("send Enter");
}

fn exit_repl(session: &mut Session) {
    submit_line(session, "quit");
    let _ = session.expect(Eof);
    thread::sleep(Duration::from_millis(25));
}

fn expect_text(session: &mut Session, text: &str) {
    for attempt in 1..=EXPECT_RETRIES {
        match session.expect(text) {
            Ok(_) => return,
            Err(ExpectError::ExpectTimeout) if attempt < EXPECT_RETRIES => continue,
            Err(err) => panic!(
                "failed to match text {:?} on attempt {}: {}",
                text, attempt, err
            ),
        }
    }

    panic!("unreachable: retries exhausted without returning");
}

fn read_trace_file(state_home: &TempDir) -> (PathBuf, String) {
    let trace_dir = state_home.path().join("pychat.ai").join("traces");
    let mut entries = fs::read_dir(&trace_dir)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", trace_dir.display()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|err| panic!("failed to iterate {}: {err}", trace_dir.display()));
    assert_eq!(
        entries.len(),
        1,
        "expected exactly one trace file in {}",
        trace_dir.display()
    );
    let entry = entries.remove(0);
    let path = entry.path();
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    (path, content)
}
