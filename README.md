# PyAIChat

Minimal Python REPL with a conversational assistant that can inspect live runtime state.

## Configuration

PyAIChat reads optional config from:

1. `$XDG_CONFIG_HOME/pyaichat/config.toml`
2. `~/.config/pyaichat/config.toml` when `XDG_CONFIG_HOME` is not set

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

For Gemini settings (`GEMINI_API_KEY`, `GEMINI_MODEL`, `GEMINI_BASE_URL`):

- defaults < config file < environment

Color enablement precedence is unchanged:

- `PYAICHAT_FORCE_COLOR` truthy value forces color on
- else `NO_COLOR` disables color
- else TTY decides
