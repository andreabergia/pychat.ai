use std::process::Command;

#[test]
fn smoke_python_flag_initializes_python_and_exits() {
    let output = Command::new(binary_path())
        .arg("--smoke-python")
        .env_remove("GEMINI_API_KEY")
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
