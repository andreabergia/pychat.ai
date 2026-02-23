# Phase 2 Plan: macOS Portable `dist/` Packaging for Embedded Python

## Summary

Implement Phase 2 of `todos/python-version.md` on macOS (arm64, simple folder distribution) by packaging a self-contained `dist/` directory that includes:

- a wrapper launcher (`dist/pychat_ai`)
- the real Rust binary (`dist/bin/pychat_ai-bin`)
- a bundled uv-managed Python runtime (`dist/python/runtime/...`)

The package will run from a moved path on macOS without relying on Homebrew/system Python by:

- building against the pinned uv-managed Python (already done in Phase 1)
- copying the runtime into `dist/`
- patching Mach-O load paths with `install_name_tool`
- launching through a wrapper that sets `PYTHONHOME`

CI will add a macOS packaging smoke job that verifies linkage and runs a real embedded-Python startup smoke via a new `--smoke-python` CLI flag.

## Goals / Success Criteria

- `scripts/dist/package-macos.sh` produces a relocatable `dist/` folder on macOS
- `otool -L dist/bin/pychat_ai-bin` shows no Homebrew/system Python absolute path
- moving `dist/` to a different path still works
- packaged launcher initializes embedded Python successfully (`--smoke-python`)
- CI validates packaging + smoke on `macos-latest`

## Out of Scope (Explicit)

- `.app` bundle packaging
- code signing / notarization
- universal binaries (Intel + Apple Silicon)
- Linux/Windows packaging
- removing the wrapper (self-configuring binary path detection)
- runtime-selectable Python versions

## Current State (Grounded)

- Phase 1 scripts/docs/CI exist and work with uv-managed Python under `.local/python`
- `PYO3_PYTHON` + `PYTHONHOME` are currently needed for reliable embedded Python stdlib resolution
- current build links to uv-managed `libpython3.14.dylib` via absolute path
- uv-managed macOS runtime layout is `./.local/python/cpython-<ver>-macos-aarch64-none/...`
- `libpython3.14.dylib` install name is currently absolute (needs patching for packaging)
- no non-interactive “initialize Python then exit” CLI path exists yet

## Important Public API / Interface Changes

### New CLI flag

Add a new CLI flag to support packaging smoke tests:

- `--smoke-python`
  - Initializes the embedded Python session (same Python init path used by the app)
  - Prints a short success line (including interpreter version/path metadata if helpful)
  - Exits `0` on success, non-zero on failure
  - Does not start the TUI/REPL

This is a deliberate small public CLI addition to make packaging validation reliable in CI and local scripts.

### New packaging scripts

Add developer-facing packaging entrypoints:

- `scripts/dist/package-macos.sh`
- `scripts/dist/smoke-macos.sh`

### New packaged output contract

Simple folder distribution layout:

- `dist/pychat_ai` (wrapper launcher script)
- `dist/bin/pychat_ai-bin` (real binary)
- `dist/python/runtime/...` (copied uv-managed runtime)

## Implementation Plan (Decision Complete)

### 1. Add `--smoke-python` CLI path (Rust)

#### Files

- `src/cli/args.rs`
- `src/main.rs` and/or the top-level `run` path in library code (where CLI dispatch happens)
- tests for arg parsing and smoke behavior

#### Behavior

- Add `smoke_python: bool` to `CliArgs`
- If `--smoke-python` is present:
  - initialize Python session (`PythonSession::initialize()` or equivalent actual path used by app)
  - perform a minimal sanity action (e.g., evaluate a trivial expression or inspect the interpreter version)
  - print one concise success line to stdout (machine-readable enough for scripts, e.g. starts with `smoke-python: ok`)
  - exit immediately without TUI startup

#### Constraints

- Must not require API keys / network / TTY
- Must be safe to run in CI repeatedly
- Must return nonzero if Python initialization fails

### 2. Add macOS packaging script: `scripts/dist/package-macos.sh`

#### Purpose

Build and assemble a relocatable macOS `dist/` folder using the pinned uv-managed Python runtime.

#### Inputs / Preconditions

- macOS only (`uname == Darwin`)
- pinned Python installed via `scripts/python/install-managed-python.sh`
- `install_name_tool` and `otool` available (macOS system tools)
- Rust toolchain installed

#### Exact packaging flow

1. Resolve pinned Python executable via `scripts/python/resolve-python.sh`
2. Derive `PYTHONHOME` from resolved executable:
   - `PYTHONHOME="$(cd "$(dirname "$PYTHON_BIN")/.." && pwd)"`
3. Export both:
   - `PYO3_PYTHON="$PYTHON_BIN"`
   - `PYTHONHOME="$PYTHONHOME"`
4. Build release binary:
   - `cargo build --release`
5. Clean and recreate `dist/`
6. Create folder layout:
   - `dist/bin/`
   - `dist/python/`
7. Copy binary:
   - `target/release/pychat_ai` -> `dist/bin/pychat_ai-bin`
8. Copy full uv-managed runtime directory (the resolved `PYTHONHOME`) to:
   - `dist/python/runtime`
   - Copy recursively preserving symlinks/permissions (`rsync -a` or `cp -a`; pick one and use consistently)
9. Identify bundled libpython path:
   - `dist/python/runtime/lib/libpython3.14.dylib` (derive version dynamically from `python3-config` or by `find`, do not hardcode `3.14` in script logic)
10. Patch Mach-O linkage:
   - Patch binary dependency load path:
     - from absolute uv path to `@loader_path/../python/runtime/lib/libpython3.14.dylib`
     - on `dist/bin/pychat_ai-bin`
   - Patch bundled dylib install name (`id`) so it is not absolute:
     - set to `@loader_path/libpython3.14.dylib`
     - on `dist/python/runtime/lib/libpython3.14.dylib`
11. Create wrapper launcher `dist/pychat_ai` (executable shell script):
   - resolve `DIST_DIR` from script location
   - export `PYTHONHOME="$DIST_DIR/python/runtime"`
   - optionally export `PYTHONNOUSERSITE=1` (recommended for reproducibility)
   - `exec "$DIST_DIR/bin/pychat_ai-bin" "$@"`
12. Verify package (basic checks inside script):
   - `otool -L dist/bin/pychat_ai-bin`
   - fail if output contains `/opt/homebrew` or repo-local `.local/python` absolute path
   - run `dist/pychat_ai --smoke-python`

#### Script output

- Prints packaging summary and final `dist/` path
- Prints `otool -L` output for visibility
- Exits nonzero on any failed patch/verification step

### 3. Add macOS smoke script: `scripts/dist/smoke-macos.sh`

#### Purpose

Validate relocatability after moving `dist/`.

#### Flow

1. Ensure `dist/pychat_ai` exists
2. Create temp dir
3. Copy `dist/` to a different path (e.g., temp dir)
4. Run:
   - `./<moved>/dist/pychat_ai --smoke-python`
5. Run linkage inspection:
   - `otool -L ./<moved>/dist/bin/pychat_ai-bin`
6. Fail if any Homebrew/repo-local absolute Python path appears

This script is the local equivalent of the CI packaging smoke.

### 4. CI: add macOS packaging smoke job

#### File

- `.github/workflows/ci.yml` (extend existing workflow)

#### Add new job (separate from existing checks)

- job name: `package-macos` (or similar)
- `runs-on: macos-latest`
- steps:
  1. checkout
  2. install rust toolchain (stable)
  3. install uv
  4. install pinned Python (`scripts/python/install-managed-python.sh`)
  5. package (`scripts/dist/package-macos.sh`)
  6. moved-path smoke (`scripts/dist/smoke-macos.sh`)

#### CI assertions

- `scripts/dist/package-macos.sh` itself enforces no absolute Homebrew/repo-local python linkage in packaged binary
- `--smoke-python` must succeed from moved `dist/`

### 5. Tests (Rust + packaging validation)

#### Rust tests

Add/Update:

- `src/cli/args.rs` parse tests for `--smoke-python`
- integration test in `tests/` for `--smoke-python` (recommended)
  - invoke binary with `--smoke-python`
  - assert success and output contains `smoke-python: ok`
  - note: this test should run under the existing Phase 1 checks script, which exports `PYO3_PYTHON` + `PYTHONHOME`

#### Packaging tests (script-level)

Validated via:

- `scripts/dist/package-macos.sh`
- `scripts/dist/smoke-macos.sh`
- CI macOS packaging job

### 6. Docs updates

#### `docs/build-python.md`

Extend with Phase 2 section:

- packaging command
- `dist/` layout
- wrapper launcher rationale (`PYTHONHOME`)
- `install_name_tool` patching behavior
- verification with `otool -L`
- moved-path smoke command

#### `README.md` (light touch)

Add a short “macOS packaging (experimental)” note pointing to `docs/build-python.md`

### 7. Acceptance Criteria (Phase 2)

Implementation is done when:

- `--smoke-python` exists and initializes embedded Python without TUI
- `scripts/dist/package-macos.sh` creates `dist/` on macOS
- `dist/pychat_ai --smoke-python` succeeds
- after moving `dist/`, `dist/pychat_ai --smoke-python` still succeeds
- `otool -L dist/bin/pychat_ai-bin` shows loader-relative Python linkage (no `/opt/homebrew/...`, no repo `.local/python/...`)
- bundled `libpython` install name is non-absolute (`@loader_path/...`)
- macOS CI packaging smoke job passes
- docs explain Phase 2 packaging and smoke workflow

## Detailed `dist/` Layout (Finalized)

```text
dist/
├── pychat_ai                 # wrapper launcher (shell script)
├── bin/
│   └── pychat_ai-bin         # real Rust binary (patched)
└── python/
    └── runtime/              # full uv-managed Python home copy
        ├── bin/
        ├── lib/
        │   ├── libpython3.14.dylib
        │   └── python3.14/...
        └── ...
```

## Edge Cases / Failure Modes to Handle

- `package-macos.sh` run on non-macOS -> fail with clear message
- `install_name_tool` patch target not found -> fail with diagnostics (`otool -L` dump)
- resolved Python runtime version mismatch with `.python-version` -> fail early
- missing `libpython*.dylib` in runtime copy -> fail early
- `--smoke-python` succeeds locally but fails in packaged launcher -> script should print `PYTHONHOME`, binary path, and `otool -L` for debugging
- moved-path smoke accidentally runs original `dist/` -> script must use temp copy path explicitly

## Commit Plan (Conventional Commits, multiple logical commits)

1. `feat(cli): add smoke-python startup check flag`
2. `feat(dist): add macos packaging and relocatability smoke scripts`
3. `ci: add macos packaging smoke job`
4. `docs: document macos dist packaging workflow`

## Verification Checklist (for implementation phase)

- `scripts/dev/checks-with-pinned-python.sh`
- `cargo test --all-features -- --nocapture smoke_python` (or equivalent targeted run) under pinned env
- `scripts/dist/package-macos.sh`
- `scripts/dist/smoke-macos.sh`
- `otool -L dist/bin/pychat_ai-bin`
- `otool -D dist/python/runtime/lib/libpython*.dylib`

## Assumptions and Defaults Chosen

- Phase 2 target is macOS first, arm64 only
- Distribution format is a simple folder `dist/`, not `.app`
- Launcher approach is a wrapper script (not binary self-configuration)
- CI adds a macOS packaging smoke job only (no artifact upload)
- Reliable packaging smoke uses a new `--smoke-python` CLI flag
- Signing/notarization is deferred to a later phase
