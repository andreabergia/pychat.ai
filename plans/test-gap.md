# Test Gap List

Coverage snapshot source: existing suite in `tests/` plus unit tests in `src/cli/repl.rs`, `src/cli/commands.rs`, `src/python/interpreter.rs`, and `src/agent/loop_impl.rs`.

- Flow: Show help text from live REPL command execution
  Mode/Surface: Command mode (`/help`)
  Gap: Command parsing is tested, but live command execution output for `/help` is not asserted.
  Suggested test type: integration

- Flow: Clear timeline and verify post-clear render state
  Mode/Surface: Command mode (`/clear`)
  Gap: No test asserts timeline content is removed and remains usable after clear.
  Suggested test type: integration

- Flow: Display command/input history from live REPL command
  Mode/Surface: Command mode (`/history`, `/history n`)
  Gap: History formatter unit logic is tested, but command execution and rendered output path are not.
  Suggested test type: integration

- Flow: Inspect expression through REPL command and print formatted JSON
  Mode/Surface: Command mode (`/inspect <expr>`)
  Gap: Capability internals are tested, but command wiring and timeline output formatting are not.
  Suggested test type: integration

- Flow: Show last Python traceback through command
  Mode/Surface: Command mode (`/last_error`)
  Gap: No command-level test for both branches: no stored error and stored traceback shown.
  Suggested test type: integration

- Flow: Show source success path for safe target
  Mode/Surface: Command mode (`/show_source <name>`)
  Gap: Unsafe-input rejection is tested, but successful source retrieval/rendering is not.
  Suggested test type: integration

- Flow: Run command-mode status query without arguments
  Mode/Surface: Command mode (`/mode`)
  Gap: No test for the read-only status output branch (`mode: py` or `mode: ai`).
  Suggested test type: integration

- Flow: Explicitly enable assistant steps via command
  Mode/Surface: Command mode (`/steps on`)
  Gap: Toggle and `off` are tested, but explicit `on` command branch is not directly asserted.
  Suggested test type: unit

- Flow: Assistant mode successful response path with provider enabled
  Mode/Surface: Assistant mode normal question flow
  Gap: Current tests cover missing-provider behavior and seeded harness state, but not full live happy-path request/response in REPL loop.
  Suggested test type: e2e

- Flow: Assistant mode failure path and recovery to next prompt
  Mode/Surface: Assistant mode provider/network/model failure
  Gap: REPL-level behavior when `run_question_with_events` returns error is not directly asserted for continued interactivity.
  Suggested test type: integration

- Flow: Python multiline incomplete input submission behavior
  Mode/Surface: Python mode Enter handling
  Gap: Input completeness engine is tested, but REPL key handling that inserts newline for incomplete input is not covered end-to-end.
  Suggested test type: integration

- Flow: History navigation with Up/Down arrows across modes
  Mode/Surface: Keyboard interaction in input area
  Gap: `history_prev/history_next` behavior has no direct key-driven integration test coverage.
  Suggested test type: integration

- Flow: Quit via Ctrl-C and Ctrl-D from active TUI session
  Mode/Surface: Session lifecycle controls
  Gap: Exit via typed `quit` is exercised indirectly; Ctrl-C and Ctrl-D exit paths are not PTY-verified.
  Suggested test type: e2e

- Flow: Recovery after Python execution failure to successful next command
  Mode/Surface: Python mode failure and continue
  Gap: Exception payloads are tested, but user-facing recovery flow in the live REPL timeline is not explicitly asserted.
  Suggested test type: integration

- Flow: Include command execution failure branch (`include failed: ...`)
  Mode/Surface: Command mode (`/include <file.py>`)
  Gap: Missing file and invalid extension are tested, but failure during execution of a readable `.py` file is not directly asserted.
  Suggested test type: integration
