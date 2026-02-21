# Config Reference

This document describes the configuration format currently supported by PyChat.ai.

## Config File Locations

PyChat.ai reads config in this order:

1. `--config /path/to/config.toml`
2. `$XDG_CONFIG_HOME/pychat.ai/config.toml`
3. `~/.config/pychat.ai/config.toml`

If `--config` is provided, that file is required and startup fails if it is missing.

## Top-Level Keys

- `gemini_api_key`: optional string
- `gemini_model`: optional string
- `gemini_base_url`: optional string
- `theme`: optional table

Unknown keys fail startup.

## Precedence Rules

- `gemini_api_key`: `GEMINI_API_KEY` environment variable overrides config file.
- `gemini_model`: config file overrides built-in default.
- `gemini_base_url`: config file overrides built-in default.

Current defaults:

- `gemini_model = "gemini-3-flash-preview"`
- `gemini_base_url = "https://generativelanguage.googleapis.com"`

`.env` loading is supported. In practice, only `GEMINI_API_KEY` is consumed from environment.

## Theme

```toml
[theme]
name = "default" # default | light | high-contrast

[theme.styles.python_prompt]
fg = "#1F6FEB"
modifiers = ["bold"]
```

### `theme.name`

Allowed values:

- `default`
- `light`
- `high-contrast`

### `theme.styles.<token>`

Supported style fields:

- `fg`: hex color, format `#RRGGBB`
- `bg`: hex color, format `#RRGGBB`
- `modifiers`: list of modifier names

Supported tokens:

- `python_prompt`
- `assistant_prompt`
- `command_prompt`
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

Supported modifiers:

- `bold`
- `dim`
- `italic`
- `underlined`
- `slow_blink`
- `rapid_blink`
- `reversed`
- `hidden`
- `crossed_out`

## Color Control (Environment)

Color output behavior:

1. If `PYCHAT_AI_FORCE_COLOR` is set to `1`, `true`, `yes`, or `on` (case-insensitive), color is forced on.
2. Else, if `NO_COLOR` is set (any value), color is disabled.
3. Else, color is enabled only when stdout is a TTY.

## Example

```toml
gemini_model = "gemini-3-flash-preview"
gemini_base_url = "https://generativelanguage.googleapis.com"

[theme]
name = "light"

[theme.styles.python_prompt]
fg = "#1F6FEB"
modifiers = ["bold"]

[theme.styles.input_block]
bg = "#F6F8FA"
fg = "#24292F"
```
