# Implement Config-File Theme Infrastructure (`dex` task `09ir77fu`)

## Summary
Add a TOML-based user config file in the XDG config location and integrate a configurable UI theme system that supports:
- Built-in presets: `default`, `light`, `high-contrast`
- Per-token style overrides (fg/bg/modifiers)
- Deterministic merge behavior: `preset base + overrides`
- Strict validation: unknown keys and invalid theme config fail startup with actionable errors

Keep existing color enablement precedence unchanged:
- `PYAICHAT_FORCE_COLOR` (truthy) forces color on
- else `NO_COLOR` disables color
- else TTY decides

Env vars continue to override file-based app config for existing LLM settings for now. Add a code TODO in `src/config.rs` stating: `TODO: we'll get rid of the env`.

## Public Interfaces / Types Changes

1. Config loading API (`src/config.rs`)
- Replace single-source `AppConfig::from_env()` with layered loading (still exposed as a single constructor):
  - Load optional TOML file from:
    1. `$XDG_CONFIG_HOME/pyaichat/config.toml`
    2. fallback `~/.config/pyaichat/config.toml` if `XDG_CONFIG_HOME` unset
  - Then load `.env` and process env vars using the simplest possible path (`dotenvy::dotenv().ok()` + `env::var`)
  - Apply precedence: defaults < file < env
- Add nested config sections:
  - `theme` section (optional)
  - existing Gemini fields remain in `AppConfig` and keep current env var names

2. TOML schema (canonical)
- Root:
  - `[theme]`
    - `name = "default" | "light" | "high-contrast"` (optional, default `default`)
  - `[theme.styles.<token>]`
    - `fg = "#RRGGBB"` (optional)
    - `bg = "#RRGGBB"` (optional)
    - `modifiers = ["bold", "italic", "underlined", "dim", "reversed", ...]` (optional)
- Example:
  ```toml
  [theme]
  name = "light"

  [theme.styles.python_prompt]
  fg = "#1F6FEB"
  modifiers = ["bold"]

  [theme.styles.input_block]
  bg = "#F6F8FA"
  fg = "#24292F"
  ```

3. Theme tokens (stable names)
Define a strongly typed token enum mapped from current UI style call sites:
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

4. Theme resolution layer
- Extract UI theming from `src/cli/repl.rs` into a dedicated module (e.g. `src/cli/theme.rs`)
- New resolver:
  - build preset palette/style map
  - apply user per-token overrides
  - emit ratatui `Style` for each token
- If colors disabled, preserve current behavior (unstyled/bold prompt baseline) regardless of configured palette.

## Implementation Plan (Decision-Complete)

1. Add dependencies
- Add `toml` crate for parsing.
- Add `dirs` crate (or `directories`) for cross-platform config dir resolution; use XDG-compatible lookup behavior as specified.

2. Build raw file-config structs
- Create serde-deserializable raw types:
  - `RawFileConfig { theme: Option<RawThemeConfig>, gemini_* optional }`
  - `RawThemeConfig { name: Option<String>, styles: Option<HashMap<String, RawStyleOverride>> }`
  - `RawStyleOverride { fg: Option<String>, bg: Option<String>, modifiers: Option<Vec<String>> }`
- Keep raw and validated structs separate.
- Use strict deserialization (`#[serde(deny_unknown_fields)]`) for root, `theme`, and style override structs so unknown keys fail immediately.

3. Add config-file discovery + read
- Implement `discover_config_path()`:
  - if `XDG_CONFIG_HOME` set: `<xdg>/pyaichat/config.toml`
  - else: `~/.config/pyaichat/config.toml`
- Missing file is not an error.
- Parse TOML when present; parse errors include file path and line/column context.
- If config location cannot be resolved (e.g., no usable home/config dir), fail fast at startup with a clear error.

4. Validate + normalize theme config
- Validate `theme.name` against preset enum (`ThemePreset`).
- Validate token names by parsing into strongly typed `ThemeToken` enum.
- Validate colors as strict hex (`#RRGGBB` only for v1).
- Validate modifiers against allowlist mapping to `ratatui::style::Modifier`.
- Build `ThemeConfig` (validated) ready for runtime use.
- Any invalid field returns startup error with:
  - config file path
  - key path (`theme.styles.python_prompt.fg`)
  - reason (`invalid hex color`, `unknown token`, etc.)

5. Merge config sources
- Preserve existing `.env` behavior.
- For gemini fields:
  - defaults -> file -> env
- Implementation note: keep env loading logic intentionally simple for now and add code TODO in `src/config.rs`: `TODO: we'll get rid of the env`.
- For theme:
  - file drives theme selection/overrides
  - color-enabled still determined exclusively by existing env/TTY logic in REPL

6. Refactor UI theme usage
- Replace hardcoded `Theme` style `match` in `repl.rs` with lookup by token from resolved theme object.
- Keep rendering semantics unchanged except style source.
- Keep `Theme::new(enabled)` entrypoint equivalent but backed by resolved preset + overrides from `AppConfig`.

7. Wire main startup
- `main.rs` passes loaded theme config into REPL state/init.
- Keep current CLI surface unchanged (explicitly no `--theme` flag).

8. Error UX
- On invalid config, startup exits with clear message:
  - `Failed to load config /path/config.toml: theme.styles.foo.fg: unknown token 'foo'`
- On missing config file, start normally.

9. Docs updates
- Update `.env.example` and/or README-equivalent docs to mention config file path and theme schema.
- Add short “Theme configuration” section with supported tokens, presets, and modifier names.

10. Commit split (logical)
- Commit 1: config-file loading infra + parsing/validation types/tests
- Commit 2: theme resolver module + REPL integration
- Commit 3: docs + polish/error message consistency

## Test Cases and Scenarios

1. Config discovery
- Uses `$XDG_CONFIG_HOME/pyaichat/config.toml` when set.
- Falls back to `~/.config/pyaichat/config.toml` when unset.
- Missing file does not error.
- Missing/invalid base config dir resolution fails startup with a clear error.

2. Precedence
- Env overrides file for `GEMINI_*`.
- File value used when env missing.
- Defaults used when neither set.

3. Theme preset selection
- `theme.name=default|light|high-contrast` resolves expected style tokens.
- Unknown preset fails startup.

4. Style override merge
- Preset base + per-token override applies only overridden fields.
- Partial override (`fg` only) keeps preset `bg/modifiers`.

5. Validation failures (fail-fast)
- Unknown token key.
- Unknown TOML keys at root/theme/style levels.
- Invalid color format (`#123`, `red`, missing `#`).
- Unknown modifier string.
- Wrong TOML type (`modifiers = "bold"`).

6. Color enablement compatibility
- `PYAICHAT_FORCE_COLOR=true` enables styling even non-TTY.
- `NO_COLOR=1` disables styling when not forced.
- Theme config present does not alter above precedence.

7. Behavior parity (simple)
- Existing REPL line-rendering tests continue passing.
- Add focused unit tests for token enum parsing, token-to-style mapping, and override application.

## Assumptions and Defaults
- No CLI flags for theme/config in this task.
- Single config file location strategy (XDG + fallback) only; no project-local file.
- Color parser supports only `#RRGGBB` in v1.
- Theme token names are internal-but-documented and treated as stable for users.
- Invalid config is treated as startup error (not warning/fallback).
- Env support is transitional; keep it simple now and track removal with inline TODO in `src/config.rs`.
