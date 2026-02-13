use expectrl::{Regex, spawn};

#[test]
fn repl_starts_with_python_prompt() {
    let mut p = spawn(binary_path()).expect("spawn binary");
    p.expect(Regex("py> ")).expect("startup prompt");
    p.send_line("exit").expect("exit line");
    p.expect(expectrl::Eof).expect("process exits");
}

#[test]
fn tab_toggles_mode_both_directions() {
    let mut p = spawn(binary_path()).expect("spawn binary");
    p.expect(Regex("py> ")).expect("startup prompt");
    p.send("\t").expect("tab to assistant");
    p.expect(Regex("ai> ")).expect("assistant prompt");
    p.send("\t").expect("tab to python");
    p.expect(Regex("py> ")).expect("python prompt again");
    p.send_line("exit").expect("exit line");
    p.expect(expectrl::Eof).expect("process exits");
}

#[test]
fn assistant_mode_returns_placeholder() {
    let mut p = spawn(binary_path()).expect("spawn binary");
    p.expect(Regex("py> ")).expect("startup prompt");
    p.send("\t").expect("tab to assistant");
    p.expect(Regex("ai> ")).expect("assistant prompt");
    p.send_line("what can you do?").expect("assistant query");
    p.expect(Regex("Assistant placeholder: not implemented yet\\."))
        .expect("placeholder response");
    p.expect(Regex("ai> ")).expect("assistant prompt persists");
    p.send_line("quit").expect("quit line");
    p.expect(expectrl::Eof).expect("process exits");
}

#[test]
fn tab_toggle_preserves_current_input_line() {
    let mut p = spawn(binary_path()).expect("spawn binary");
    p.expect(Regex("py> ")).expect("startup prompt");
    p.send("what is this").expect("type partial input");
    p.send("\t").expect("tab to assistant");
    p.expect(Regex("ai> ")).expect("assistant prompt");
    p.send("\n").expect("submit preserved input");
    p.expect(Regex("Assistant placeholder: not implemented yet\\."))
        .expect("placeholder response");
    p.send_line("quit").expect("quit line");
    p.expect(expectrl::Eof).expect("process exits");
}

fn binary_path() -> String {
    std::env::var("CARGO_BIN_EXE_pyaichat").unwrap_or_else(|_| "target/debug/pyaichat".to_string())
}
