use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub const DEFAULT_GEMINI_MODEL: &str = "gemini-3-flash-preview";
pub const DEFAULT_GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com";

const CONFIG_DIR_NAME: &str = "pychat.ai";
const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub config_path: PathBuf,
    pub config_is_explicit: bool,
    pub gemini_api_key: Option<String>,
    pub gemini_model: String,
    pub gemini_base_url: String,
    pub startup_file: Option<PathBuf>,
    pub theme: ThemeConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeConfig {
    pub preset: ThemePreset,
    pub styles: HashMap<ThemeToken, StyleOverride>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            preset: ThemePreset::Default,
            styles: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemePreset {
    Default,
    Light,
    HighContrast,
}

impl FromStr for ThemePreset {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "default" => Ok(Self::Default),
            "light" => Ok(Self::Light),
            "high-contrast" => Ok(Self::HighContrast),
            _ => Err(format!("unknown preset '{value}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeToken {
    PythonPrompt,
    AssistantPrompt,
    CommandPrompt,
    UserInputPython,
    UserInputAssistant,
    PythonValue,
    PythonStdout,
    PythonStderr,
    PythonTraceback,
    AssistantText,
    AssistantWaiting,
    AssistantProgressRequest,
    AssistantProgressResult,
    SystemInfo,
    SystemError,
    Status,
    Motd,
    MotdKey,
    MotdBrand,
    FooterPrimary,
    FooterSecondary,
    FooterAccent,
    InputBlock,
}

impl FromStr for ThemeToken {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "python_prompt" => Ok(Self::PythonPrompt),
            "assistant_prompt" => Ok(Self::AssistantPrompt),
            "command_prompt" => Ok(Self::CommandPrompt),
            "user_input_python" => Ok(Self::UserInputPython),
            "user_input_assistant" => Ok(Self::UserInputAssistant),
            "python_value" => Ok(Self::PythonValue),
            "python_stdout" => Ok(Self::PythonStdout),
            "python_stderr" => Ok(Self::PythonStderr),
            "python_traceback" => Ok(Self::PythonTraceback),
            "assistant_text" => Ok(Self::AssistantText),
            "assistant_waiting" => Ok(Self::AssistantWaiting),
            "assistant_progress_request" => Ok(Self::AssistantProgressRequest),
            "assistant_progress_result" => Ok(Self::AssistantProgressResult),
            "system_info" => Ok(Self::SystemInfo),
            "system_error" => Ok(Self::SystemError),
            "status" => Ok(Self::Status),
            "motd" => Ok(Self::Motd),
            "motd_key" => Ok(Self::MotdKey),
            "motd_brand" => Ok(Self::MotdBrand),
            "footer_primary" => Ok(Self::FooterPrimary),
            "footer_secondary" => Ok(Self::FooterSecondary),
            "footer_accent" => Ok(Self::FooterAccent),
            "input_block" => Ok(Self::InputBlock),
            _ => Err(format!("unknown token '{value}'")),
        }
    }
}

impl ThemeToken {
    pub const fn all() -> [Self; 23] {
        [
            Self::PythonPrompt,
            Self::AssistantPrompt,
            Self::CommandPrompt,
            Self::UserInputPython,
            Self::UserInputAssistant,
            Self::PythonValue,
            Self::PythonStdout,
            Self::PythonStderr,
            Self::PythonTraceback,
            Self::AssistantText,
            Self::AssistantWaiting,
            Self::AssistantProgressRequest,
            Self::AssistantProgressResult,
            Self::SystemInfo,
            Self::SystemError,
            Self::Status,
            Self::Motd,
            Self::MotdKey,
            Self::MotdBrand,
            Self::FooterPrimary,
            Self::FooterSecondary,
            Self::FooterAccent,
            Self::InputBlock,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleOverride {
    pub fg: Option<HexColor>,
    pub bg: Option<HexColor>,
    pub modifiers: Option<Vec<ThemeModifier>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HexColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl FromStr for HexColor {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let bytes = value.as_bytes();
        if bytes.len() != 7 || bytes[0] != b'#' {
            return Err("invalid hex color, expected #RRGGBB".to_string());
        }

        let r = u8::from_str_radix(&value[1..3], 16)
            .map_err(|_| "invalid hex color, expected #RRGGBB".to_string())?;
        let g = u8::from_str_radix(&value[3..5], 16)
            .map_err(|_| "invalid hex color, expected #RRGGBB".to_string())?;
        let b = u8::from_str_radix(&value[5..7], 16)
            .map_err(|_| "invalid hex color, expected #RRGGBB".to_string())?;

        Ok(Self { r, g, b })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeModifier {
    Bold,
    Dim,
    Italic,
    Underlined,
    SlowBlink,
    RapidBlink,
    Reversed,
    Hidden,
    CrossedOut,
}

impl FromStr for ThemeModifier {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "bold" => Ok(Self::Bold),
            "dim" => Ok(Self::Dim),
            "italic" => Ok(Self::Italic),
            "underlined" => Ok(Self::Underlined),
            "slow_blink" => Ok(Self::SlowBlink),
            "rapid_blink" => Ok(Self::RapidBlink),
            "reversed" => Ok(Self::Reversed),
            "hidden" => Ok(Self::Hidden),
            "crossed_out" => Ok(Self::CrossedOut),
            _ => Err(format!("unknown modifier '{value}'")),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFileConfig {
    gemini_api_key: Option<String>,
    gemini_model: Option<String>,
    gemini_base_url: Option<String>,
    startup_file: Option<String>,
    theme: Option<RawThemeConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawThemeConfig {
    name: Option<String>,
    styles: Option<HashMap<String, RawStyleOverride>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStyleOverride {
    fg: Option<String>,
    bg: Option<String>,
    modifiers: Option<Vec<String>>,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        Self::load_with_path(None)
    }

    pub fn load_with_path(config_path_override: Option<&Path>) -> Result<Self> {
        let (config_path, require_config_file) = resolve_config_path(config_path_override)?;
        let file_config = load_file_config(&config_path, require_config_file)?;
        dotenvy::dotenv().ok();

        let file_api_key = file_config
            .as_ref()
            .and_then(|cfg| cfg.gemini_api_key.as_ref())
            .and_then(|value| non_empty(value).map(ToOwned::to_owned));
        let file_model = file_config
            .as_ref()
            .and_then(|cfg| cfg.gemini_model.as_ref())
            .and_then(|value| non_empty(value).map(ToOwned::to_owned));
        let file_base_url = file_config
            .as_ref()
            .and_then(|cfg| cfg.gemini_base_url.as_ref())
            .and_then(|value| non_empty(value).map(ToOwned::to_owned));
        let startup_file = resolve_startup_file(
            file_config
                .as_ref()
                .and_then(|cfg| cfg.startup_file.as_deref())
                .and_then(non_empty),
            &config_path,
        )?;

        let theme = validate_theme(
            file_config.as_ref().and_then(|cfg| cfg.theme.as_ref()),
            &config_path,
        )?;

        Ok(Self {
            config_path: config_path.clone(),
            config_is_explicit: require_config_file,
            gemini_api_key: env_non_empty("GEMINI_API_KEY").or(file_api_key),
            gemini_model: file_model.unwrap_or_else(|| DEFAULT_GEMINI_MODEL.to_string()),
            gemini_base_url: file_base_url.unwrap_or_else(|| DEFAULT_GEMINI_BASE_URL.to_string()),
            startup_file,
            theme,
        })
    }
}

fn resolve_startup_file(startup_file: Option<&str>, config_path: &Path) -> Result<Option<PathBuf>> {
    let Some(startup_file) = startup_file else {
        return Ok(None);
    };

    let path = PathBuf::from(startup_file);
    if path.is_absolute() {
        return Ok(Some(path));
    }

    let config_dir = config_path.parent().ok_or_else(|| {
        anyhow!(
            "Failed to resolve startup_file relative path for config {}: missing parent directory",
            config_path.display()
        )
    })?;

    Ok(Some(config_dir.join(path)))
}

fn resolve_config_path(config_path_override: Option<&Path>) -> Result<(PathBuf, bool)> {
    if let Some(path) = config_path_override {
        return Ok((path.to_path_buf(), true));
    }

    Ok((discover_config_path()?, false))
}

fn discover_config_path() -> Result<PathBuf> {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        let trimmed = xdg.trim();
        if trimmed.is_empty() {
            bail!("Failed to resolve config path: XDG_CONFIG_HOME is set but empty");
        }

        return Ok(PathBuf::from(trimmed)
            .join(CONFIG_DIR_NAME)
            .join(CONFIG_FILE_NAME));
    }

    let home = dirs::home_dir()
        .ok_or_else(|| anyhow!("Failed to resolve config path: HOME directory is unavailable"))?;

    Ok(home
        .join(".config")
        .join(CONFIG_DIR_NAME)
        .join(CONFIG_FILE_NAME))
}

fn load_file_config(
    config_path: &Path,
    require_config_file: bool,
) -> Result<Option<RawFileConfig>> {
    if !config_path.is_file() {
        if require_config_file {
            bail!(
                "Failed to load config {}: file does not exist or is not a regular file",
                config_path.display()
            );
        }
        return Ok(None);
    }

    let config_text = fs::read_to_string(config_path).map_err(|err| {
        anyhow!(
            "Failed to load config {}: unable to read file: {err}",
            config_path.display()
        )
    })?;

    toml::from_str(&config_text)
        .map(Some)
        .map_err(|err| anyhow!("Failed to load config {}: {err}", config_path.display()))
}

fn validate_theme(raw_theme: Option<&RawThemeConfig>, config_path: &Path) -> Result<ThemeConfig> {
    let Some(theme) = raw_theme else {
        return Ok(ThemeConfig::default());
    };

    let mut config = ThemeConfig::default();

    if let Some(name) = &theme.name {
        config.preset = ThemePreset::from_str(name)
            .map_err(|reason| config_error(config_path, "theme.name", &reason))?;
    }

    if let Some(styles) = &theme.styles {
        for (token_name, raw_style) in styles {
            let token = ThemeToken::from_str(token_name).map_err(|reason| {
                config_error(config_path, &format!("theme.styles.{token_name}"), &reason)
            })?;

            let fg = parse_color(raw_style.fg.as_deref(), config_path, token_name, "fg")?;
            let bg = parse_color(raw_style.bg.as_deref(), config_path, token_name, "bg")?;
            let modifiers =
                parse_modifiers(raw_style.modifiers.as_deref(), config_path, token_name)?;

            config
                .styles
                .insert(token, StyleOverride { fg, bg, modifiers });
        }
    }

    Ok(config)
}

fn parse_color(
    value: Option<&str>,
    config_path: &Path,
    token_name: &str,
    field_name: &str,
) -> Result<Option<HexColor>> {
    let Some(value) = value else {
        return Ok(None);
    };

    HexColor::from_str(value).map(Some).map_err(|reason| {
        config_error(
            config_path,
            &format!("theme.styles.{token_name}.{field_name}"),
            &reason,
        )
    })
}

fn parse_modifiers(
    values: Option<&[String]>,
    config_path: &Path,
    token_name: &str,
) -> Result<Option<Vec<ThemeModifier>>> {
    let Some(values) = values else {
        return Ok(None);
    };

    let mut parsed = Vec::with_capacity(values.len());
    for value in values {
        let modifier = ThemeModifier::from_str(value).map_err(|reason| {
            config_error(
                config_path,
                &format!("theme.styles.{token_name}.modifiers"),
                &reason,
            )
        })?;
        parsed.push(modifier);
    }

    Ok(Some(parsed))
}

fn env_non_empty(key: &str) -> Option<String> {
    env::var(key).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn config_error(config_path: &Path, key_path: &str, reason: &str) -> anyhow::Error {
    anyhow!(
        "Failed to load config {}: {key_path}: {reason}",
        config_path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::{
        AppConfig, DEFAULT_GEMINI_BASE_URL, DEFAULT_GEMINI_MODEL, HexColor, ThemeConfig,
        ThemePreset, ThemeToken,
    };
    use serial_test::serial;
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn reset_vars() {
        unsafe {
            env::remove_var("GEMINI_API_KEY");
            env::remove_var("GEMINI_MODEL");
            env::remove_var("GEMINI_BASE_URL");
            env::remove_var("XDG_CONFIG_HOME");
        }
    }

    fn with_cwd<T>(path: &Path, f: impl FnOnce() -> T) -> T {
        let cwd = env::current_dir().expect("current dir");
        env::set_current_dir(path).expect("set current dir");
        let result = f();
        env::set_current_dir(cwd).expect("restore current dir");
        result
    }

    #[test]
    #[serial]
    fn load_uses_default_model_when_unset() {
        let tmp = tempfile::tempdir().expect("tempdir");
        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let cfg = with_cwd(tmp.path(), || AppConfig::load().expect("load config"));
        assert_eq!(cfg.gemini_model, DEFAULT_GEMINI_MODEL);
        assert_eq!(cfg.theme, ThemeConfig::default());
    }

    #[test]
    #[serial]
    fn load_env_api_key_overrides_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("pychat.ai");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(
            config_dir.join("config.toml"),
            r#"
gemini_api_key = "file_key"
gemini_model = "file_model"
gemini_base_url = "https://example.com"
"#,
        )
        .expect("write config");

        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
            env::set_var("GEMINI_API_KEY", "os_key");
            env::set_var("GEMINI_MODEL", "os_model");
            env::set_var("GEMINI_BASE_URL", "https://os.example.com");
        }

        let cfg = with_cwd(tmp.path(), || AppConfig::load().expect("load config"));
        assert_eq!(cfg.gemini_api_key.as_deref(), Some("os_key"));
        assert_eq!(cfg.gemini_model, "file_model");
        assert_eq!(cfg.gemini_base_url, "https://example.com");
    }

    #[test]
    #[serial]
    fn load_reads_api_key_from_dotenv_but_ignores_other_dotenv_vars() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let env_path = tmp.path().join(".env");
        fs::write(
            &env_path,
            "GEMINI_API_KEY=file_key\nGEMINI_MODEL=file_model\nGEMINI_BASE_URL=https://dotenv.example.com\n",
        )
        .expect("write env file");

        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let cfg = with_cwd(tmp.path(), || AppConfig::load().expect("load config"));

        assert_eq!(cfg.gemini_api_key.as_deref(), Some("file_key"));
        assert_eq!(cfg.gemini_model, DEFAULT_GEMINI_MODEL);
        assert_eq!(cfg.gemini_base_url, DEFAULT_GEMINI_BASE_URL);
    }

    #[test]
    #[serial]
    fn load_uses_xdg_config_path_when_set() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("pychat.ai");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(
            config_dir.join("config.toml"),
            r#"gemini_model = "from_file""#,
        )
        .expect("write config");

        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let cfg = with_cwd(tmp.path(), || AppConfig::load().expect("load config"));
        assert_eq!(cfg.config_path, config_dir.join("config.toml"));
        assert!(!cfg.config_is_explicit);
        assert_eq!(cfg.gemini_model, "from_file");
    }

    #[test]
    #[serial]
    fn load_with_path_uses_explicit_config_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("pychat.ai");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(
            config_dir.join("config.toml"),
            r#"gemini_model = "xdg_discovered_model""#,
        )
        .expect("write xdg config");

        let explicit_config = tmp.path().join("custom.toml");
        fs::write(&explicit_config, r#"gemini_model = "explicit_model""#).expect("write config");

        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let cfg = with_cwd(tmp.path(), || {
            AppConfig::load_with_path(Some(explicit_config.as_path())).expect("load config")
        });
        assert_eq!(cfg.config_path, explicit_config);
        assert!(cfg.config_is_explicit);
        assert_eq!(cfg.gemini_model, "explicit_model");
    }

    #[test]
    #[serial]
    fn load_resolves_relative_startup_file_from_config_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("pychat.ai");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(
            config_dir.join("config.toml"),
            r#"startup_file = "scripts/bootstrap.py""#,
        )
        .expect("write config");

        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let cfg = with_cwd(tmp.path(), || AppConfig::load().expect("load config"));
        assert_eq!(
            cfg.startup_file,
            Some(config_dir.join(PathBuf::from("scripts/bootstrap.py")))
        );
    }

    #[test]
    #[serial]
    fn load_keeps_absolute_startup_file_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("pychat.ai");
        fs::create_dir_all(&config_dir).expect("create config dir");
        let startup_path = tmp.path().join("bootstrap.py");
        fs::write(
            config_dir.join("config.toml"),
            format!(r#"startup_file = "{}""#, startup_path.display()),
        )
        .expect("write config");

        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let cfg = with_cwd(tmp.path(), || AppConfig::load().expect("load config"));
        assert_eq!(cfg.startup_file, Some(startup_path));
    }

    #[test]
    #[serial]
    fn load_treats_empty_startup_file_as_unset() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("pychat.ai");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(config_dir.join("config.toml"), r#"startup_file = "   ""#).expect("write config");

        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let cfg = with_cwd(tmp.path(), || AppConfig::load().expect("load config"));
        assert_eq!(cfg.startup_file, None);
    }

    #[test]
    #[serial]
    fn load_with_path_fails_when_explicit_file_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let missing_path = tmp.path().join("missing.toml");

        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let err = with_cwd(tmp.path(), || {
            AppConfig::load_with_path(Some(missing_path.as_path())).expect_err("load should fail")
        });
        assert!(
            err.to_string()
                .contains("file does not exist or is not a regular file")
        );
    }

    #[test]
    #[serial]
    fn load_fails_when_xdg_config_home_is_empty() {
        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", "   ");
        }

        let err = AppConfig::load().expect_err("load should fail");
        assert!(
            err.to_string()
                .contains("Failed to resolve config path: XDG_CONFIG_HOME is set but empty")
        );
    }

    #[test]
    #[serial]
    fn load_fails_on_unknown_root_key() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("pychat.ai");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(config_dir.join("config.toml"), "unknown_key = 1").expect("write config");

        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let err = with_cwd(tmp.path(), || {
            AppConfig::load().expect_err("load should fail")
        });
        assert!(err.to_string().contains("Failed to load config"));
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    #[serial]
    fn load_fails_on_unknown_style_token() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("pychat.ai");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(
            config_dir.join("config.toml"),
            r##"
[theme.styles.unknown_token]
fg = "#ffffff"
"##,
        )
        .expect("write config");

        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let err = with_cwd(tmp.path(), || {
            AppConfig::load().expect_err("load should fail")
        });
        assert!(
            err.to_string()
                .contains("theme.styles.unknown_token: unknown token 'unknown_token'")
        );
    }

    #[test]
    #[serial]
    fn load_fails_on_invalid_hex_color() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("pychat.ai");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[theme.styles.python_prompt]
fg = "red"
"#,
        )
        .expect("write config");

        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let err = with_cwd(tmp.path(), || {
            AppConfig::load().expect_err("load should fail")
        });
        assert!(
            err.to_string()
                .contains("theme.styles.python_prompt.fg: invalid hex color")
        );
    }

    #[test]
    #[serial]
    fn load_fails_on_unknown_modifier() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("pychat.ai");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(
            config_dir.join("config.toml"),
            r#"
[theme.styles.python_prompt]
modifiers = ["sparkly"]
"#,
        )
        .expect("write config");

        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let err = with_cwd(tmp.path(), || {
            AppConfig::load().expect_err("load should fail")
        });
        assert!(
            err.to_string()
                .contains("theme.styles.python_prompt.modifiers: unknown modifier 'sparkly'")
        );
    }

    #[test]
    #[serial]
    fn load_parses_theme_config_with_strong_types() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config_dir = tmp.path().join("pychat.ai");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(
            config_dir.join("config.toml"),
            r##"
[theme]
name = "light"

[theme.styles.python_prompt]
fg = "#A0B1C2"
"##,
        )
        .expect("write config");

        reset_vars();
        unsafe {
            env::set_var("XDG_CONFIG_HOME", tmp.path());
        }

        let cfg = with_cwd(tmp.path(), || AppConfig::load().expect("load config"));
        assert_eq!(cfg.theme.preset, ThemePreset::Light);
        let style = cfg
            .theme
            .styles
            .get(&ThemeToken::PythonPrompt)
            .expect("python_prompt style");
        assert_eq!(
            style.fg,
            Some(HexColor {
                r: 0xA0,
                g: 0xB1,
                b: 0xC2
            })
        );
    }
}
