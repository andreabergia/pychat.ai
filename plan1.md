# Phase 1 Plan — Rust Host Foundation

## Summary
Build the first runnable PyAIChat shell: a Rust REPL with two modes, `TAB` mode toggle, and embedded Python initialization. Phase 1 is foundation-only and intentionally defers runtime capabilities and LLM work.

## Locked Decisions
- Scope: init-focused Phase 1 (no full runtime API surface yet)
- REPL behavior: `TAB` always toggles mode (no completion in Phase 1)
- Prompt format: `py>` and `ai>`
- Assistant mode submit: fixed placeholder response
- Structure: modular layout now (`src/cli`, `src/python`)
- Python strategy: system Python now, vendored runtime deferred
- Runtime style: synchronous entrypoint in Phase 1
- Tests: automated PTY E2E required (`expectrl`)
- Rust baseline: Edition 2024

## Deliverables
1. Rust crate bootstrapped with PyO3 + rustyline + anyhow.
2. REPL loop with explicit `Mode` state (`Python`, `Assistant`).
3. `TAB` handler that flips mode and updates prompt.
4. Embedded Python interpreter initialized at startup and retained in app state.
5. Assistant input path returns fixed "not implemented" placeholder.
6. PTY E2E tests validating startup prompt, mode toggling, assistant placeholder, and clean exit.

## Proposed Layout
```text
pyaichat/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── cli/
│   │   ├── mod.rs
│   │   └── repl.rs
│   └── python/
│       ├── mod.rs
│       └── interpreter.rs
└── tests/
    └── e2e_repl.rs
```

## Public Types and Contracts
- `enum Mode { Python, Assistant }`
- `struct AppState { mode: Mode, python: PythonSession }`
- `struct PythonSession` with init and startup health check
- `fn prompt_for(mode: Mode) -> &'static str`
- Assistant submit handler contract returning placeholder output

## Acceptance Criteria
- `cargo test` passes, including PTY E2E tests.
- App starts with `py>` prompt.
- Pressing `TAB` switches `py>` <-> `ai>` deterministically.
- Submitting input in `ai>` prints placeholder response.
- Embedded Python session is initialized and kept in state for later phases.

## Explicitly Deferred
- Python exec/eval API completeness
- Exception capture interface
- Capability system (`list_globals`, `get_type`, etc.)
- LLM provider integration
- Agent loop/tool orchestration
- Vendored Python packaging implementation
