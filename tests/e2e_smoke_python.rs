use std::process::Command;
use tempfile::tempdir;

#[test]
fn smoke_python_flag_initializes_python_and_exits() {
    let home_dir = tempdir().expect("create temp home");
    let xdg_config_home = tempdir().expect("create temp xdg config home");

    let output = Command::new(binary_path())
        .arg("--smoke-python")
        .env_remove("GEMINI_API_KEY")
        .env("HOME", home_dir.path())
        .env("XDG_CONFIG_HOME", xdg_config_home.path())
        .output()
        .expect("run --smoke-python");

    assert!(
        output.status.success(),
        "--smoke-python should exit successfully"
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(
        stdout.contains("smoke-python: ok"),
        "smoke output should report success, got: {stdout:?}"
    );
}

fn binary_path() -> String {
    std::env::var("CARGO_BIN_EXE_pychat_ai")
        .unwrap_or_else(|_| "target/debug/pychat_ai".to_string())
}
