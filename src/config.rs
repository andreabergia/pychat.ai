use std::env;

pub const DEFAULT_GEMINI_MODEL: &str = "gemini-2.0-flash";
pub const DEFAULT_GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub gemini_api_key: Option<String>,
    pub gemini_model: String,
    pub gemini_base_url: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();

        Self {
            gemini_api_key: env::var("GEMINI_API_KEY")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            gemini_model: env::var("GEMINI_MODEL")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_GEMINI_MODEL.to_string()),
            gemini_base_url: env::var("GEMINI_BASE_URL")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_GEMINI_BASE_URL.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, DEFAULT_GEMINI_MODEL};
    use serial_test::serial;
    use std::env;
    use std::fs;

    fn reset_vars() {
        unsafe {
            env::remove_var("GEMINI_API_KEY");
            env::remove_var("GEMINI_MODEL");
            env::remove_var("GEMINI_BASE_URL");
        }
    }

    #[test]
    #[serial]
    fn from_env_uses_default_model_when_unset() {
        reset_vars();
        let cfg = AppConfig::from_env();
        assert_eq!(cfg.gemini_model, DEFAULT_GEMINI_MODEL);
        assert_eq!(cfg.gemini_api_key, None);
    }

    #[test]
    #[serial]
    fn from_env_does_not_override_existing_os_env_with_dotenv() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let env_path = tmp.path().join(".env");
        fs::write(
            &env_path,
            "GEMINI_API_KEY=file_key\nGEMINI_MODEL=file_model\n",
        )
        .expect("write env file");

        reset_vars();
        unsafe {
            env::set_var("GEMINI_API_KEY", "os_key");
            env::set_var("GEMINI_MODEL", "os_model");
        }

        let cwd = env::current_dir().expect("current dir");
        env::set_current_dir(tmp.path()).expect("set current dir");

        let cfg = AppConfig::from_env();

        env::set_current_dir(cwd).expect("restore current dir");

        assert_eq!(cfg.gemini_api_key.as_deref(), Some("os_key"));
        assert_eq!(cfg.gemini_model, "os_model");

        reset_vars();
    }
}
