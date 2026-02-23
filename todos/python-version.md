---
id: python-version
created_at: 2026-02-22T11:50:22.612922Z
status: closed
summary: Choose Python runtime
---
Goal: make embedded Python builds reproducible and packaged binaries portable across machines (starting with macOS).

## Proposed strategy (recommended)

Use a pinned CPython installed via `uv` for build reproducibility, then bundle that Python runtime next to the Rust binary and patch loader paths to be relative (`@loader_path`) during packaging.

This gives us:
- Consistent build inputs in local dev + CI
- No dependency on Homebrew/system Python paths at runtime
- A distributable folder layout we control

## Scope split

### Phase 1: Reproducible build interpreter (dev/CI)

- Pin a Python version (e.g. `3.14.x`) in project tooling/docs
- Install that Python with `uv` in a project-local location
- Build with `PYO3_PYTHON=<project-local-python-executable>`
- Add a verification step that prints PyO3 config (`PYO3_PRINT_CONFIG=1 cargo check`) in docs/CI troubleshooting

Notes:
- `auto-initialize` only initializes Python; it does not affect static vs dynamic linking.
- A venv alone is not enough for portability. We need the actual runtime (`Python.framework` / `libpython`) + stdlib for distribution.

### Phase 2: Portable packaged runtime (macOS first)

- Define a bundle layout, e.g.:
  - `dist/pychat_ai`
  - `dist/python/Python.framework/...`
  - `dist/python/lib/python3.14/...` (stdlib, if not already in framework layout)
- Build `pychat_ai` against the pinned `uv` CPython
- Copy required Python runtime files into `dist/python`
- Patch the Rust binary’s Python dependency from an absolute path to `@loader_path/../python/...`
- Verify with `otool -L dist/pychat_ai` that no Homebrew path remains
- Run a smoke test on a machine/path without Homebrew Python in the same location

Notes:
- Do not rely on `./python-whatever` literal paths in the binary.
- Use macOS loader-relative paths (`@loader_path` or `@rpath`) instead.
- May require `install_name_tool` and (later) signing/notarization considerations.

## Runtime selection (future option)

If we still want user-selectable Python versions at runtime, treat that as a separate feature:
- Explicit CLI/config setting for a Python runtime path
- Validation and compatibility checks
- Clear fallback behavior

This is a different path than PyO3 compile-time linking and will likely require additional launcher/runtime-loading design decisions.

## Open questions

- What exact `uv` workflow do we want (`uv python install`, local toolchain path, checked-in helper script)?
- Which macOS versions/architectures do we target first (arm64 only vs universal)?
- Do we want a simple folder distribution first, or a `.app` bundle/package format?

## Acceptance criteria (initial implementation)

- Build script/docs produce a binary linked against a project-managed pinned Python, not Homebrew
- Packaged `dist/` folder runs after moving it to a different path on the same machine
- `otool -L` output shows `@loader_path`/`@rpath` for Python, not absolute `/opt/homebrew/...`
- Basic REPL startup smoke test passes from packaged output