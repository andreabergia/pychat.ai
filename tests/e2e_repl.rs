use std::process::Command;

#[test]
fn binary_help_smoke_test() {
    let output = Command::new(binary_path())
        .arg("--help")
        .output()
        .expect("run --help");

    assert!(output.status.success(), "--help should exit successfully");

    let stdout = String::from_utf8(output.stdout).expect("help output is utf-8");
    assert!(
        stdout.contains("Minimal Python REPL with a conversational assistant"),
        "help output should include app description"
    );
}

fn binary_path() -> String {
    std::env::var("CARGO_BIN_EXE_pyaichat").unwrap_or_else(|_| "target/debug/pyaichat".to_string())
}
