use anyhow::{Result, anyhow, bail};
use reqwest::header::HeaderMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;

const TRACE_DIR_NAME: &str = "pychat.ai/traces";

#[derive(Clone)]
pub struct SessionTrace {
    inner: Arc<TraceInner>,
}

struct TraceInner {
    writer: Mutex<BufWriter<File>>,
    file_path: PathBuf,
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
        let file = create_trace_file(&file_path)
            .map_err(|err| anyhow!("Failed to create trace file {}: {err}", file_path.display()))?;

        Ok(Self {
            inner: Arc::new(TraceInner {
                writer: Mutex::new(BufWriter::new(file)),
                file_path,
                write_failed: AtomicBool::new(false),
            }),
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn create_in_temp_dir(session_id: &str, trace_dir: &Path) -> Result<Self> {
        Self::create_in_dir(session_id, trace_dir)
    }

    pub fn file_path(&self) -> &Path {
        &self.inner.file_path
    }

    pub fn log_input_python(&self, text: &str) {
        self.log_lines("py.in", text);
    }

    pub fn log_input_assistant(&self, text: &str) {
        self.log_lines("ai.in", text);
    }

    pub fn log_output(&self, kind: &str, text: &str) {
        self.log_lines(kind, text);
    }

    pub fn log_http_request(&self, method: &str, url: &str, headers: &HeaderMap, body: &str) {
        self.log_single("ai.http.in", &format!("{method} {url}"));
        for (name, value) in headers {
            let value = value
                .to_str()
                .map(std::string::ToString::to_string)
                .unwrap_or_else(|_| "<non-utf8>".to_string());
            self.log_single("ai.http.in", &format!("{}: {}", name.as_str(), value));
        }
        self.log_lines("ai.http.in", body);
    }

    pub fn log_http_response(&self, status: u16, headers: &HeaderMap, body: &str) {
        self.log_single("ai.http.out", &format!("HTTP {status}"));
        for (name, value) in headers {
            let value = value
                .to_str()
                .map(std::string::ToString::to_string)
                .unwrap_or_else(|_| "<non-utf8>".to_string());
            self.log_single("ai.http.out", &format!("{}: {}", name.as_str(), value));
        }
        self.log_lines("ai.http.out", body);
    }

    pub fn log_http_error(&self, message: &str) {
        self.log_single("ai.http.err", message);
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

    fn log_single(&self, kind: &str, text: &str) {
        let timestamp = current_timestamp();
        self.write_raw(&format!("[{timestamp}] [{:<11}] {text}\n", kind));
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
            eprintln!("PyChat.ai trace warning: {message}");
        }
    }
}

#[cfg(unix)]
fn create_trace_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_trace_file(path: &Path) -> std::io::Result<File> {
    File::create(path)
}

fn current_timestamp() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.millisecond()
    )
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
    use super::{SessionTrace, resolve_trace_dir};
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn resolve_trace_dir_uses_xdg_state_when_set() {
        let dir = resolve_trace_dir(Some("/tmp/state"), Some(Path::new("/home/fallback")))
            .expect("trace path");
        assert_eq!(dir, Path::new("/tmp/state/pychat.ai/traces"));
    }

    #[test]
    fn resolve_trace_dir_uses_home_fallback() {
        let dir = resolve_trace_dir(None, Some(Path::new("/home/alice"))).expect("trace path");
        assert_eq!(dir, Path::new("/home/alice/.local/state/pychat.ai/traces"));
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

    #[test]
    fn trace_line_uses_iso_timestamp_and_padded_kind() {
        let dir = tempdir().expect("tempdir");
        let trace = SessionTrace::create_in_temp_dir("abc", dir.path()).expect("trace");
        let path = trace.file_path().to_path_buf();
        trace.log_output("py.out", "value");

        let content = fs::read_to_string(path).expect("read trace");
        let first_line = content.lines().next().expect("line");
        assert!(first_line.starts_with("[20"));
        assert!(first_line.contains("T"));
        assert!(first_line.contains("Z] [py.out     ] value"));
    }

    #[cfg(unix)]
    #[test]
    fn trace_file_permissions_are_owner_only() {
        let dir = tempdir().expect("tempdir");
        let trace = SessionTrace::create_in_temp_dir("abc", dir.path()).expect("trace");
        let metadata = fs::metadata(trace.file_path()).expect("metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
