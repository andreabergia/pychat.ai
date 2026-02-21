## UI Rendering Test Infrastructure Rearchitecture (pyaichat, inspired by mahgit)

### Summary
We will adopt a layered UI test pyramid for `pyaichat`:
1. deterministic integration-style rendering tests via `ratatui::TestBackend` (mahgit-style harness),
2. selective snapshot coverage for stable UI regions,
3. a small PTY E2E smoke layer for true binary behavior.

This requires a **lib+bin split** first, because `pyaichat` is currently binary-only (`src/main.rs:1`) and cannot expose reusable test harness APIs to integration tests.

### Why this direction (based on repo analysis)
- `pyaichat` already has strong module unit tests for timeline rendering and UI helpers (`src/cli/timeline.rs:303`, `src/cli/repl.rs:1228`) but no reusable terminal-buffer integration harness and only one binary smoke test (`tests/e2e_repl.rs:3`).
- `mahgit` provides a transferable pattern: reusable `TestApp` wrapper over `Terminal<TestBackend>` (`../mahgit/tests/test_backend_utils.rs:5`) used by integration tests for real UI state assertions (`../mahgit/tests/user_interface_test.rs:107`).

## Public API / Interface Changes
1. Add a library target and keep binary entrypoint thin.
- New `src/lib.rs` exporting current modules (`agent`, `cli`, `config`, `http`, `llm`, `python`, `trace`) and a `pub async fn run(args: cli::CliArgs) -> anyhow::Result<()>`.
- `src/main.rs` becomes argument parse + call into `pyaichat::run(...)`.

2. Add a test-support feature for integration-only UI harness access.
- In `Cargo.toml`, add:
  - `[features]`
  - `test-support = []`
- Expose a controlled test module from `cli::repl` behind `#[cfg(feature = "test-support")]`.

3. Add test harness interface (feature-gated).
- New module (for example) `src/cli/repl_test_support.rs` with:
  - `pub struct UiHarness`
  - `pub fn new_harness(width: u16, height: u16, state: AppState) -> Result<UiHarness, ...>`
  - methods: `send_key`, `send_mouse`, `render`, `buffer_text`, `buffer_snapshot`, `ui_state_view`.
- Keep production internals private; test-support is the only external seam.

## Implementation Plan

### Phase 1: Structural Refactor (enable integration testing)
1. Create `src/lib.rs` and move startup orchestration out of `src/main.rs:20`.
2. Keep existing behavior unchanged by adding regression check:
- existing `tests/e2e_repl.rs` remains green.
3. Update imports in unit tests if needed (minimal churn).

### Phase 2: Build Mahgit-style UI Harness
1. Introduce feature-gated harness around `draw_ui` path (`src/cli/repl.rs:900`).
2. Use `Terminal<TestBackend>` and expose buffer extraction helpers similar to `mahgit`’s `buffer_to_string` approach.
3. Add deterministic setup helpers:
- fixed session id,
- `NO_COLOR=1` defaults for snapshot stability,
- test temp dirs for trace/config isolation.

### Phase 3: Add Integration UI Test Suite (Timeline + Input first)
Create `tests/ui_rendering/` with:
1. `common.rs`:
- harness builder,
- key event helpers,
- snapshot normalizer (strip trailing spaces, normalize line endings).
2. `timeline_input_render_test.rs`:
- initial welcome render appears,
- prompt mode text changes (`py>`, `ai>`, `cmd>`),
- multiline input scroll/cursor behavior,
- status bar text and session id placement,
- assistant-thinking block on/off behavior.
3. `timeline_scroll_mouse_test.rs`:
- scroll offset behavior matches expectations from unit logic.
4. Assertions:
- semantic asserts for behavior/state,
- `insta` snapshots for selected regions (timeline block and status line), not full screen by default.

### Phase 4: Strengthen PTY E2E Smoke Layer (expectrl)
1. Add `tests/e2e_ui_smoke.rs`:
- start binary, verify visible prompt,
- TAB toggles Python/Assistant prompt,
- Ctrl-T toggles “Show agent thinking” indicator,
- `/trace` writes expected path output.
2. Mark PTY tests serial and robust against timing flake (bounded retries/timeouts).
3. Keep E2E count intentionally small (2-4 tests).

### Phase 5: Quality Gates and Dev UX
1. Add test commands to README/AGENTS-aligned workflow:
- `cargo test --lib`
- `cargo test --features test-support --test 'ui_*'`
- `cargo test --test e2e_* -- --test-threads=1`
2. Ensure full required checks before completion:
- formatter, linter, tests.

### Phase 6: Coverage Expansion Review
1. Run a post-implementation gap analysis across all user-facing flows in Python mode, Assistant mode, and command mode, including failure and recovery paths.
2. Produce `plans/test-gap-analysis.md` with:
- covered flows,
- uncovered flows,
- risk level for each uncovered flow (`high`, `medium`, `low`),
- proposed test type for each gap (`unit`, `integration`, `e2e`) and priority order.
3. Review the gap analysis and create a prioritized follow-up backlog (Phase 7) to implement missing tests, starting from high-risk gaps.
4. Acceptance criteria:
- no `high`-risk user-facing flow remains untested,
- all accepted follow-up test items have owner + priority + target milestone.

## Test Cases and Scenarios

### Integration Harness Tests (new)
1. Empty timeline shows welcome message.
2. Prompt token switching for Python/Assistant/Command.
3. Input area handles multiline and cursor row/col rendering.
4. Timeline manual scroll is preserved across new output renders.
5. Mouse wheel affects timeline only when cursor is in timeline area.
6. Status line includes mode + session id consistently.
7. Assistant thinking block visibility toggles retroactively.

### Snapshot Scenarios (new)
1. Baseline render (empty session).
2. Render after mixed entries (python stdout/stderr/value + assistant turn).
3. Render with thinking block shown and hidden.

### E2E Scenarios (new)
1. Binary starts and shows prompt.
2. TAB changes mode prompt.
3. Ctrl‑T toggles step visibility indicator.
4. `/trace` prints concrete file path and app remains interactive.

## Commit Plan (conventional commits, short/focused)
1. `refactor(core): split pyaichat into lib and bin entrypoint`
2. `test(ui): add feature-gated ratatui test harness`
3. `test(ui): add integration rendering tests with semantic assertions`
4. `test(ui): add curated insta snapshots for timeline and status`
5. `test(e2e): add expectrl ui smoke flows`
6. `docs(testing): document ui test commands and layers`

## Assumptions and Defaults
1. We proceed with **lib+bin now**.
2. We use **layered pyramid** (integration-heavy + small E2E smoke).
3. We use **semantic + snapshot** assertions, with snapshots scoped to stable regions.
4. Phase 1 scope is **Timeline + Input** first; other surfaces follow after baseline harness is stable.
5. No backward-compatibility constraints are required for this unreleased project.
