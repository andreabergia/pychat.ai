use anyhow::{Result, anyhow, bail};
use reqwest::header::HeaderMap;
use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const TRACE_DIR_NAME: &str = "pyaichat/traces";

#[derive(Clone)]
pub struct SessionTrace {
    inner: Arc<TraceInner>,
}

struct TraceInner {
    writer: Mutex<BufWriter<File>>,
    file_name: String,
    write_failed: AtomicBool,
}

impl SessionTrace {
    pub fn create(session_id: &str) -> Result<Self> {
        let trace_dir = resolve_trace_dir_from_env()?;
        Self::create_in_dir(session_id, &trace_dir)
    }

    fn create_in_dir(session_id: &str, trace_dir: &Path) -> Result<Self> {
        fs::create_dir_all(trace_dir).map_err(|err| {
            anyhow!(
                "Failed to create trace directory {}: {err}",
                trace_dir.display()
            )
        })?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        let file_name = format!("session-{session_id}-{timestamp}.log");
        let file_path = trace_dir.join(&file_name);
        let file = File::create(&file_path)
            .map_err(|err| anyhow!("Failed to create trace file {}: {err}", file_path.display()))?;

        Ok(Self {
            inner: Arc::new(TraceInner {
                writer: Mutex::new(BufWriter::new(file)),
                file_name,
                write_failed: AtomicBool::new(false),
            }),
        })
    }

    #[cfg(test)]
    pub fn create_in_temp_dir(session_id: &str, trace_dir: &Path) -> Result<Self> {
        Self::create_in_dir(session_id, trace_dir)
    }

    pub fn file_name(&self) -> &str {
        &self.inner.file_name
    }

    pub fn log_input_python(&self, text: &str) {
        self.log_lines("input.python", text);
    }

    pub fn log_input_assistant(&self, text: &str) {
        self.log_lines("input.assistant", text);
    }

    pub fn log_output(&self, kind: &str, text: &str) {
        self.log_lines(kind, text);
    }

    pub fn log_http_request(&self, method: &str, url: &str, headers: &HeaderMap, body: &str) {
        self.log_single("http.request", &format!("{method} {url}"));
        for (name, value) in headers {
            let value = value
                .to_str()
                .map(std::string::ToString::to_string)
                .unwrap_or_else(|_| "<non-utf8>".to_string());
            self.log_single(
                "http.request.header",
                &format!("{}: {}", name.as_str(), value),
            );
        }
        self.log_block("http.request.body", body);
    }

    pub fn log_http_response(&self, status: u16, headers: &HeaderMap, body: &str) {
        self.log_single("http.response", &format!("HTTP {status}"));
        for (name, value) in headers {
            let value = value
                .to_str()
                .map(std::string::ToString::to_string)
                .unwrap_or_else(|_| "<non-utf8>".to_string());
            self.log_single(
                "http.response.header",
                &format!("{}: {}", name.as_str(), value),
            );
        }
        self.log_block("http.response.body", body);
    }

    pub fn log_http_error(&self, message: &str) {
        self.log_single("http.error", message);
    }

    fn log_lines(&self, kind: &str, text: &str) {
        if text.is_empty() {
            self.log_single(kind, "<empty>");
            return;
        }

        for line in text.lines() {
            self.log_single(kind, line);
        }
    }

    fn log_block(&self, kind: &str, text: &str) {
        let timestamp = current_timestamp();
        self.write_raw(&format!("[{timestamp}] [{kind}.begin]\n"));
        if text.is_empty() {
            self.write_raw("<empty>\n");
        } else {
            self.write_raw(text);
            if !text.ends_with('\n') {
                self.write_raw("\n");
            }
        }
        let timestamp = current_timestamp();
        self.write_raw(&format!("[{timestamp}] [{kind}.end]\n"));
    }

    fn log_single(&self, kind: &str, text: &str) {
        let timestamp = current_timestamp();
        self.write_raw(&format!("[{timestamp}] [{kind}] {text}\n"));
    }

    fn write_raw(&self, text: &str) {
        let Ok(mut writer) = self.inner.writer.lock() else {
            self.report_write_failure("failed to acquire trace writer lock");
            return;
        };

        if writer.write_all(text.as_bytes()).is_err() || writer.flush().is_err() {
            self.report_write_failure("failed to write to trace file");
        }
    }

    fn report_write_failure(&self, message: &str) {
        if !self.inner.write_failed.swap(true, Ordering::Relaxed) {
            eprintln!("PyAiChat trace warning: {message}");
        }
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

pub fn resolve_trace_dir_from_env() -> Result<PathBuf> {
    let xdg_state = env::var("XDG_STATE_HOME").ok();
    let home = dirs::home_dir();
    resolve_trace_dir(xdg_state.as_deref(), home.as_deref())
}

fn resolve_trace_dir(xdg_state_home: Option<&str>, home_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(xdg) = xdg_state_home {
        let trimmed = xdg.trim();
        if trimmed.is_empty() {
            bail!("Failed to resolve trace path: XDG_STATE_HOME is set but empty");
        }
        return Ok(PathBuf::from(trimmed).join(TRACE_DIR_NAME));
    }

    let home = home_dir
        .ok_or_else(|| anyhow!("Failed to resolve trace path: HOME directory is unavailable"))?;
    Ok(home.join(".local/state").join(TRACE_DIR_NAME))
}

#[cfg(test)]
mod tests {
    use super::resolve_trace_dir;
    use std::path::Path;

    #[test]
    fn resolve_trace_dir_uses_xdg_state_when_set() {
        let dir = resolve_trace_dir(Some("/tmp/state"), Some(Path::new("/home/fallback")))
            .expect("trace path");
        assert_eq!(dir, Path::new("/tmp/state/pyaichat/traces"));
    }

    #[test]
    fn resolve_trace_dir_uses_home_fallback() {
        let dir = resolve_trace_dir(None, Some(Path::new("/home/alice"))).expect("trace path");
        assert_eq!(dir, Path::new("/home/alice/.local/state/pyaichat/traces"));
    }

    #[test]
    fn resolve_trace_dir_rejects_empty_xdg_state() {
        let err = resolve_trace_dir(Some("   "), Some(Path::new("/home/alice")))
            .expect_err("empty xdg state should fail");
        assert!(
            err.to_string()
                .contains("Failed to resolve trace path: XDG_STATE_HOME is set but empty")
        );
    }

    #[test]
    fn resolve_trace_dir_fails_without_home_and_xdg_state() {
        let err = resolve_trace_dir(None, None).expect_err("missing home should fail");
        assert!(
            err.to_string()
                .contains("Failed to resolve trace path: HOME directory is unavailable")
        );
    }
}
