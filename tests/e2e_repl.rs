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
    p.expect(Regex(
        "Assistant unavailable: missing GEMINI_API_KEY\\. Configure it in your shell or \\.env file",
    ))
    .expect("missing key guidance");
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
    p.expect(Regex(
        "Assistant unavailable: missing GEMINI_API_KEY\\. Configure it in your shell or \\.env file",
    ))
    .expect("missing key guidance");
    p.send_line("quit").expect("quit line");
    p.expect(expectrl::Eof).expect("process exits");
}

#[test]
fn python_mode_expression_echoes_result() {
    let mut p = spawn(binary_path()).expect("spawn binary");
    p.expect(Regex("py> ")).expect("startup prompt");
    p.send_line("1 + 2").expect("expression input");
    p.expect(Regex("3\\r?\\n")).expect("evaluated result");
    p.expect(Regex("py> ")).expect("python prompt persists");
    p.send_line("exit").expect("exit line");
    p.expect(expectrl::Eof).expect("process exits");
}

#[test]
fn python_mode_error_prints_full_traceback() {
    let mut p = spawn(binary_path()).expect("spawn binary");
    p.expect(Regex("py> ")).expect("startup prompt");
    p.send_line("1 / 0").expect("failing expression");
    p.expect(Regex("Traceback \\(most recent call last\\):"))
        .expect("traceback header");
    p.expect(Regex("ZeroDivisionError: division by zero"))
        .expect("exception type and message");
    p.expect(Regex("py> ")).expect("python prompt persists");
    p.send_line("quit").expect("quit line");
    p.expect(expectrl::Eof).expect("process exits");
}

#[test]
fn python_mode_state_continues_across_inputs() {
    let mut p = spawn(binary_path()).expect("spawn binary");
    p.expect(Regex("py> ")).expect("startup prompt");
    p.send_line("x = 99").expect("set variable");
    p.expect(Regex("py> ")).expect("prompt after statement");
    p.send_line("x").expect("read variable");
    p.expect(Regex("99\\r?\\n"))
        .expect("state continuity value");
    p.expect(Regex("py> ")).expect("prompt after expression");
    p.send_line("quit").expect("quit line");
    p.expect(expectrl::Eof).expect("process exits");
}

fn binary_path() -> String {
    std::env::var("CARGO_BIN_EXE_pyaichat").unwrap_or_else(|_| "target/debug/pyaichat".to_string())
}
