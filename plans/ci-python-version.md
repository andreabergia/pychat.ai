# CI + Python Version Reproducibility (Phase 1)

## Summary

Implement Phase 1 of `todos/python-version.md`:

- Pin Python to an exact `3.14.x` patch via `.python-version`
- Install a uv-managed CPython into a project-local directory
- Build/test with `PYO3_PYTHON` pointing at that interpreter
- Add helper scripts and docs for local contributor workflows
- Add GitHub Actions CI validating the pinned-Python flow on macOS + Linux

Out of scope:

- Phase 2 packaging/bundling (`dist/`, `install_name_tool`, `@loader_path`)
- Runtime-selectable Python versions
- `.app` packaging

## Current State (Verified)

- `pyo3` is used with `auto-initialize` in `Cargo.toml`
- No `build.rs` exists
- No `.github/workflows/` exists yet
- No `.python-version` / `uv.toml` pinning exists
- Current local binary links to an absolute Homebrew Python path

## Implementation Scope

### 1. Python pinning and local install convention

- Add repo-root `.python-version` with an exact `3.14.x` patch
- Default to latest stable `3.14.x` at implementation time; fallback `3.14.3`
- Use project-local uv install path: `./.local/python`
- Resolve interpreter dynamically (prefer `./.local/python/bin/python3`)

### 2. Helper scripts

Add executable scripts:

- `scripts/python/install-managed-python.sh`
  - Reads `.python-version`
  - Runs `uv python install --install-dir ./.local/python "$(cat .python-version)"`
  - Verifies interpreter and prints version/path
- `scripts/python/resolve-python.sh`
  - Outputs the interpreter path for `PYO3_PYTHON`
  - Fails with actionable guidance if missing
- `scripts/dev/checks-with-pinned-python.sh`
  - Exports `PYO3_PYTHON` from resolver
  - Runs `cargo fmt --all --check`
  - Runs `cargo clippy --all-targets --all-features -- -D warnings`
  - Runs `cargo test --all-features`
- `scripts/dev/pyo3-config-check.sh`
  - Exports `PYO3_PYTHON`
  - Runs `PYO3_PRINT_CONFIG=1 cargo check`

Script conventions:

- `#!/usr/bin/env bash`
- `set -euo pipefail`
- Resolve repo root from script location

### 3. `.gitignore` updates

Add local tool/runtime ignores:

- `.local/`

Do not ignore `.python-version`.

### 4. Documentation updates

- Update `README.md` with reproducible Python build workflow using uv + scripts
- Update `docs/contributing.md` to use pinned Python scripts for checks
- Add `docs/build-python.md` covering:
  - problem statement
  - `.python-version` + uv local install workflow
  - `PYO3_PYTHON` usage
  - verification commands (`otool -L` / `ldd`)
  - note that Phase 2 packaging is deferred

### 5. GitHub Actions CI

Add `.github/workflows/ci.yml`:

- Triggers: `push`, `pull_request`
- Matrix: `ubuntu-latest`, `macos-latest`
- Steps:
  - checkout
  - install Rust (stable + fmt + clippy)
  - install `uv`
  - run `scripts/python/install-managed-python.sh`
  - run `scripts/dev/checks-with-pinned-python.sh`
- macOS-only extra step:
  - `otool -L target/debug/pychat_ai`

### 6. Acceptance criteria

- `.python-version` exists and pins exact `3.14.x`
- uv-managed local CPython installs under `./.local/python`
- Checks run using `PYO3_PYTHON` set to pinned interpreter
- Docs updated for local + contributor workflows
- CI validates the pinned Python flow on macOS + Linux
- `scripts/dev/pyo3-config-check.sh` is available for troubleshooting

## Public interface changes

No Rust API changes.

New developer-facing interfaces:

- `.python-version`
- `scripts/python/install-managed-python.sh`
- `scripts/python/resolve-python.sh`
- `scripts/dev/checks-with-pinned-python.sh`
- `scripts/dev/pyo3-config-check.sh`
- `.github/workflows/ci.yml`

## Test scenarios

Local/manual:

1. Resolver fails clearly before install
2. Install script installs pinned Python into `./.local/python`
3. Resolver returns interpreter path
4. Checks script runs fmt/clippy/test with pinned interpreter
5. PyO3 config check prints config using pinned interpreter
6. `otool -L target/debug/pychat_ai` on macOS shows linkage for inspection (absolute path acceptable in Phase 1)

CI:

1. `ubuntu-latest` passes
2. `macos-latest` passes
3. macOS logs include `otool -L` output

Regression:

1. Existing `cargo run` path still works (less reproducible)
2. No runtime behavior regressions

## Commit plan

1. `chore(build): pin python version and add local uv setup scripts`
2. `docs: document reproducible python build workflow`
3. `ci: add pinned-python checks workflow`

## Verification before done

- `cargo fmt --all --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-features`
- `scripts/dev/pyo3-config-check.sh`
- `otool -L target/debug/pychat_ai` (macOS)

## Assumptions and defaults

- Target todo: `todos/python-version.md`
- Phase 1 only
- Python line: `3.14.x`, exact patch pinned in repo
- CI provider: GitHub Actions
- CI coverage: macOS + Linux
- Helper scripts preferred over `build.rs`
- Future Phase 2 default packaging target remains arm64 folder distribution
