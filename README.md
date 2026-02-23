# PyChat.ai

PyChat.ai is an interactive Python REPL, with a built-in LLM assistant that can inspect your live runtime state.

## Important

- This is an **MVP/prototype** at the moment, to explore the idea. It is not a full product
- This is **very insecure** - the LLM can run arbitrary code! Do **not** use it outside of a sandbox!

## What You Can Do

- Run Python code
- Switch between Python mode and assistant mode with `Tab`
- Ask assistant questions grounded in the current runtime
- Use slash commands for quick inspection and session operations

## License

[AGPL v3.0](LICENSE)

## Quick Start

### Requirements

- Rust toolchain
- `uv` (recommended for reproducible Python builds)
- Python installed locally (only needed if you are not using the pinned `uv` workflow)
- `GEMINI_API_KEY` if you want assistant responses

### Reproducible Python Build (Recommended)

PyChat.ai embeds Python via PyO3, so the Rust binary links against the Python interpreter chosen at build time.
For consistent local builds and CI, use the pinned project-managed Python runtime:

```bash
scripts/python/install-managed-python.sh
```

Run checks with the pinned interpreter:

```bash
scripts/dev/checks-with-pinned-python.sh
```

For PyO3 interpreter/linking diagnostics:

```bash
scripts/dev/pyo3-config-check.sh
```

See `docs/build-python.md` for details.

On macOS, an experimental portable `dist/` packaging workflow is also available (`scripts/dist/package-macos.sh`
and `scripts/dist/smoke-macos.sh`). See `docs/build-python.md` for the packaging steps and verification checks.

### Run

```bash
cargo run
```

With assistant enabled:

```bash
GEMINI_API_KEY=your_key cargo run
```

You can also set `GEMINI_API_KEY` in `.env`.

For reproducible local runs with the pinned interpreter:

```bash
PYTHON_BIN="$(scripts/python/resolve-python.sh)"
PYTHONHOME="$(cd "$(dirname "$PYTHON_BIN")/.." && pwd)" \
PYO3_PYTHON="$PYTHON_BIN" \
cargo run
```

## Basic Usage

- Enter Python code and press `Enter`
- Press `Tab` to switch modes
- Type `/help` to see commands
- Type `exit` or `quit` to leave

## Configuration

Optional config path resolution:

1. `--config /path/to/config.toml`
2. `$XDG_CONFIG_HOME/pychat.ai/config.toml`
3. `~/.config/pychat.ai/config.toml`

Startup script behavior:

- Without `--config`, PyChat.ai also auto-runs `<config-dir>/startup.py` when present.
- If `startup_file` is set in config, that script is executed before REPL startup.

## Docs

- User guide: `docs/user-guide.md`
- Config reference: `docs/config-reference.md`
- Command reference: `docs/command-reference.md`
- Contributing: `docs/contributing.md`
- Architecture plan: `docs/architecture.md`
- Python build reproducibility: `docs/build-python.md`
