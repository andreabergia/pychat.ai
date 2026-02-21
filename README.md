# PyChat.ai

Minimal Python REPL with a conversational assistant that can inspect live runtime state.

## Configuration

Use `--config /path/to/config.toml` to load a specific config file.

PyChat.ai reads optional config from:

1. `$XDG_CONFIG_HOME/pychat.ai/config.toml`
2. `~/.config/pychat.ai/config.toml` when `XDG_CONFIG_HOME` is not set

The default probe path is used only when `--config` is not provided.

Unknown keys and invalid values fail startup with an actionable error.

### Theme

```toml
[theme]
name = "default" # default | light | high-contrast

[theme.styles.python_prompt]
fg = "#1F6FEB"
modifiers = ["bold"]

[theme.styles.input_block]
bg = "#F6F8FA"
fg = "#24292F"
```

Token names:

- `python_prompt`
- `assistant_prompt`
- `user_input_python`
- `user_input_assistant`
- `python_value`
- `python_stdout`
- `python_stderr`
- `python_traceback`
- `assistant_text`
- `assistant_waiting`
- `assistant_progress_request`
- `assistant_progress_result`
- `system_info`
- `system_error`
- `status`
- `input_block`

Modifier names:

- `bold`
- `dim`
- `italic`
- `underlined`
- `slow_blink`
- `rapid_blink`
- `reversed`
- `hidden`
- `crossed_out`

### Config Precedence

For `gemini_api_key`:

- config file < environment (`GEMINI_API_KEY`)
- `.env` is supported for `GEMINI_API_KEY` (shell environment still wins over `.env`)

For `gemini_model` and `gemini_base_url`:

- defaults < config file

Color enablement precedence is unchanged:

- `PYCHAT_AI_FORCE_COLOR` truthy value forces color on
- else `NO_COLOR` disables color
- else TTY decides
