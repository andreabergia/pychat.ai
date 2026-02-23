# Python Build Reproducibility

## Why this exists

PyChat.ai embeds Python via PyO3. PyO3 links the Rust binary against the Python interpreter used at build
time, which can lead to machine-specific absolute paths (for example, Homebrew Python paths on macOS).

To make local development and CI builds reproducible, this project pins a Python version and installs a
project-local uv-managed runtime that is used explicitly via `PYO3_PYTHON`.

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

Absolute Python library paths may still appear in the binary; the linkage checks above help inspect what the
current build produced.
