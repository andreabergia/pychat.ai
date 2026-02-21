# User Guide

PyChat.ai lets you run Python and ask questions about the current runtime in the same session.

## Install and Run

1. Ensure Rust and Python are installed.
2. From the repo root, run:

```bash
cargo run
```

3. To enable assistant responses, set:

```bash
export GEMINI_API_KEY=your_key
cargo run
```

You can also put `GEMINI_API_KEY` in `.env`.

## First Session

1. Start in Python mode.
2. Run code, for example:

```python
x = [1, 2, 3]
sum(x)
```

3. Press `Tab` to switch to assistant mode.
4. Ask a question such as: `what is x and what can I do with it?`
5. Press `Tab` again to return to Python mode.

## Commands

- `/help` show command list
- `/mode [py|ai]` show or switch mode
- `/clear` clear timeline output
- `/history [n]` show history
- `/trace` print current trace file path
- `/inspect <expr>` print structured inspection JSON
- `/last_error` print last Python exception traceback
- `/include <file.py>` execute a Python file in-session
- `/run <file>` alias for include, no extension restriction
- `/show_source <name>` show source for function/class/module names
- `/steps [on|off]` show or hide assistant tool-step output

## Config File

Path precedence:

1. `--config <path>`
2. `$XDG_CONFIG_HOME/pychat.ai/config.toml`
3. `~/.config/pychat.ai/config.toml`

Example:

```toml
gemini_model = "gemini-3-flash-preview"
startup_file = "startup.py"

[theme]
name = "light"

[theme.styles.python_prompt]
fg = "#1F6FEB"
modifiers = ["bold"]
```

Full reference: `docs/config-reference.md`

Startup behavior:

- `startup_file` executes before the REPL starts.
- Relative `startup_file` paths are resolved relative to the config file directory.
- Without `--config`, `startup.py` in the config directory is auto-executed if it exists.
- With `--config`, implicit `startup.py` discovery is disabled.

## Traces

Each session writes a trace log under:

- `$XDG_STATE_HOME/pychat.ai/traces`, or
- `~/.local/state/pychat.ai/traces`

Use `/trace` to get the exact path for the active session.

## Common Issues

- Assistant says unavailable: set `GEMINI_API_KEY`
- Config load fails: verify TOML shape and key names
- Python import/runtime errors: use `/last_error` for traceback
