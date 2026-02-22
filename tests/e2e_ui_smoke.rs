#![cfg(unix)]

use expectrl::{Eof, Error as ExpectError, Session};
use serial_test::serial;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

const EXPECT_TIMEOUT: Duration = Duration::from_secs(4);
const EXPECT_RETRIES: usize = 3;

#[test]
#[serial]
fn startup_renders_python_prompt_and_status() {
    let (mut session, _config_home, state_home) = spawn_app();

    expect_text(&mut session, "py> ");

    exit_repl(&mut session);
    let (trace_path, _content) = read_trace_file(&state_home);
    assert!(
        trace_path.exists(),
        "trace file should exist after interactive session"
    );
}

#[test]
#[serial]
fn tab_toggles_prompt_between_python_and_assistant() {
    let (mut session, _config_home, state_home) = spawn_app();

    expect_text(&mut session, "py> ");

    send_tab(&mut session);
    submit_line(&mut session, "/mode");

    send_tab(&mut session);
    submit_line(&mut session, "/mode");

    exit_repl(&mut session);
    let (_trace_path, content) = read_trace_file(&state_home);
    assert!(
        content.contains("mode: ai"),
        "first TAB should switch mode to assistant"
    );
    assert!(
        content.contains("mode: py"),
        "second TAB should switch mode back to python"
    );
}

#[test]
#[serial]
fn ctrl_t_toggles_show_agent_thinking_indicator() {
    let (mut session, _config_home, state_home) = spawn_app();

    expect_text(&mut session, "py> ");

    send_ctrl_t(&mut session);
    submit_line(&mut session, "/steps");

    exit_repl(&mut session);
    let (_trace_path, content) = read_trace_file(&state_home);
    assert!(
        content.contains("steps: on"),
        "after Ctrl-T from default On->Off, /steps toggle should report steps: on"
    );
}

#[test]
#[serial]
fn ctrl_c_exits_active_tui_session() {
    let (mut session, _config_home, state_home) = spawn_app();

    expect_text(&mut session, "py> ");

    send_ctrl_c(&mut session);
    let _ = session.expect(Eof);
    thread::sleep(Duration::from_millis(25));

    let (trace_path, _content) = read_trace_file(&state_home);
    assert!(
        trace_path.exists(),
        "trace file should exist after Ctrl-C exit"
    );
}

#[test]
#[serial]
fn ctrl_d_exits_active_tui_session() {
    let (mut session, _config_home, state_home) = spawn_app();

    expect_text(&mut session, "py> ");

    send_ctrl_d(&mut session);
    let _ = session.expect(Eof);
    thread::sleep(Duration::from_millis(25));

    let (trace_path, _content) = read_trace_file(&state_home);
    assert!(
        trace_path.exists(),
        "trace file should exist after Ctrl-D exit"
    );
}

#[test]
#[serial]
fn trace_command_prints_session_trace_path_and_stays_interactive() {
    let (mut session, _config_home, state_home) = spawn_app();

    expect_text(&mut session, "py> ");

    submit_line(&mut session, "/trace");

    exit_repl(&mut session);
    let (trace_path, content) = read_trace_file(&state_home);
    let trace_path_text = trace_path.display().to_string();
    assert!(
        content.contains("/trace"),
        "trace command invocation should be logged"
    );
    assert!(
        content.contains(&trace_path_text),
        "/trace should output the concrete current trace file path"
    );
}

fn spawn_app() -> (Session, TempDir, TempDir) {
    let config_home = tempfile::tempdir().expect("create XDG_CONFIG_HOME tempdir");
    let state_home = tempfile::tempdir().expect("create XDG_STATE_HOME tempdir");

    let mut command = Command::new(binary_path());
    command
        .env("NO_COLOR", "1")
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("XDG_STATE_HOME", state_home.path())
        .env_remove("GEMINI_API_KEY");

    let mut session = Session::spawn(command).expect("spawn pychat.ai in PTY");
    session.set_expect_timeout(Some(EXPECT_TIMEOUT));

    (session, config_home, state_home)
}

fn binary_path() -> String {
    std::env::var("CARGO_BIN_EXE_pychat_ai")
        .unwrap_or_else(|_| "target/debug/pychat_ai".to_string())
}

fn send_tab(session: &mut Session) {
    session.send([b'\t']).expect("send TAB");
}

fn send_ctrl_t(session: &mut Session) {
    session.send([0x14]).expect("send Ctrl-T");
}

fn send_ctrl_c(session: &mut Session) {
    session.send([0x03]).expect("send Ctrl-C");
}

fn send_ctrl_d(session: &mut Session) {
    session.send([0x04]).expect("send Ctrl-D");
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
