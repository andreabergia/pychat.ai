# Python Build Reproducibility

## Why this exists

PyChat.ai embeds Python via PyO3. PyO3 links the Rust binary against the Python interpreter used at build
time, which can lead to machine-specific absolute paths (for example, Homebrew Python paths on macOS).

To make local development and CI builds reproducible, this project pins a Python version and installs a
project-local uv-managed runtime that is used explicitly via `PYO3_PYTHON`.

Phase 1 covers reproducible build inputs only. Portable packaging (`dist/`, loader path patching) is planned
separately.

## Pinned Python workflow

The pinned Python version lives in `.python-version`.

Install the managed runtime into `./.local/python`:

```bash
scripts/python/install-managed-python.sh
```

Resolve the interpreter path used for PyO3 builds:

```bash
scripts/python/resolve-python.sh
```

Run all project checks with the pinned interpreter:

```bash
scripts/dev/checks-with-pinned-python.sh
```

## Using `PYO3_PYTHON` directly

If you want to run a single command manually with the pinned interpreter:

```bash
PYTHON_BIN="$(scripts/python/resolve-python.sh)"
PYTHONHOME="$(cd "$(dirname "$PYTHON_BIN")/.." && pwd)" \
PYO3_PYTHON="$PYTHON_BIN" \
cargo run
```

This ensures PyO3 binds to the project-managed Python instead of a system/Homebrew Python and that the
embedded runtime can find the pinned stdlib.

## Diagnostics and troubleshooting

Print PyO3 interpreter configuration:

```bash
scripts/dev/pyo3-config-check.sh
```

Inspect dynamic linkage of the built binary:

macOS:

```bash
otool -L target/debug/pychat_ai
```

Linux:

```bash
ldd target/debug/pychat_ai
```

In Phase 1, absolute Python library paths may still appear in the binary. Eliminating those for portable
distribution is Phase 2 work.

## Phase 2: macOS portable `dist/` packaging

Phase 2 adds a macOS packaging workflow that builds a relocatable `dist/` folder containing:

- a wrapper launcher (`dist/pychat_ai`)
- the real Rust binary (`dist/bin/pychat_ai-bin`)
- a bundled Python runtime copied from the uv-managed pinned interpreter (`dist/python/runtime/...`)

The packaging script patches the Rust binary's `libpython` linkage to a loader-relative path and rewrites the
bundled `libpython` install name so the package can run after being moved to a different path.

Build and package on macOS:

```bash
scripts/dist/package-macos.sh
```

Smoke-test relocatability by copying `dist/` to a temporary path and running the packaged launcher there:

```bash
scripts/dist/smoke-macos.sh
```

The packaged launcher sets:

- `PYTHONHOME=<dist>/python/runtime`
- `PYTHONNOUSERSITE=1`

This ensures the embedded interpreter resolves the bundled stdlib/runtime instead of user or system Python
locations.

### Packaging verification (macOS)

Inspect the packaged binary linkage (should use `@loader_path`, not `/opt/homebrew/...` or repo-local
`.local/python/...`):

```bash
otool -L dist/bin/pychat_ai-bin
```

Inspect the bundled `libpython` install ID (should be non-absolute):

```bash
otool -D dist/python/runtime/lib/libpython*.dylib
```

Run the non-interactive embedded-Python startup smoke directly:

```bash
dist/pychat_ai --smoke-python
```
