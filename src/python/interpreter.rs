use anyhow::{Result, anyhow};
use pyo3::prelude::*;
use pyo3::types::{
    PyAnyMethods, PyDict, PyDictMethods, PyFloat, PyList, PyModule, PyString, PyTuple,
};
use serde_json::Value;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use super::capabilities::{
    CapabilityError, CapabilityProvider, CapabilityResult, EvalInfo, GlobalEntry, InspectInfo,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalResult {
    pub value_repr: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionInfo {
    pub exc_type: String,
    pub message: String,
    pub traceback: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserRunResult {
    Evaluated(EvalResult),
    Executed(ExecResult),
    Failed {
        stdout: String,
        stderr: String,
        exception: ExceptionInfo,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputCompleteness {
    Complete,
    Incomplete,
    Invalid,
}

pub struct PythonSession {
    globals: Py<PyDict>,
    last_exception: Mutex<Option<ExceptionInfo>>,
    source_counter: AtomicU64,
}

const INSPECT_EVAL_TIMEOUT_SECONDS: f64 = 1.0;
const MIN_TIMER_DELAY_SECONDS: f64 = 1e-6;

#[allow(dead_code)]
impl PythonSession {
    pub fn initialize() -> Result<Self> {
        Python::attach(|py| -> Result<Self> {
            let globals = PyDict::new(py);
            let builtins = PyModule::import(py, "builtins")?;
            globals.set_item("__builtins__", builtins)?;
            globals.set_item("__name__", "__main__")?;
            Self::health_check(py, &globals)?;

            let session = Self {
                globals: globals.unbind(),
                last_exception: Mutex::new(None),
                source_counter: AtomicU64::new(0),
            };

            if !session.is_healthy() {
                anyhow::bail!("python session failed health check");
            }

            Ok(session)
        })
    }

    #[allow(dead_code)]
    pub fn exec_code(&self, code: &str) -> Result<ExecResult> {
        Python::attach(|py| -> Result<ExecResult> {
            self.exec_code_inner(py, code)
                .map_err(|exception| anyhow!(exception.traceback))
        })
    }

    #[allow(dead_code)]
    pub fn eval_expr(&self, expr: &str) -> Result<EvalResult> {
        Python::attach(|py| -> Result<EvalResult> {
            self.eval_expr_inner(py, expr)
                .map_err(|exception| anyhow!(exception.traceback))
        })
    }

    pub fn run_user_input(&self, line: &str) -> Result<UserRunResult> {
        Python::attach(|py| -> Result<UserRunResult> {
            let eval_filename = self
                .register_source(py, line, "eval")
                .map_err(|err| pyo3::exceptions::PyRuntimeError::new_err(err.to_string()))?;
            match self.compile_source(py, line, &eval_filename, "eval") {
                Ok(compiled) => {
                    let compiled = compiled.unbind();
                    let output = self.capture_output(py, |py| {
                        let globals = self.globals.bind(py);
                        let value = self.eval_compiled(py, globals, compiled.bind(py))?;
                        let value_repr = self.safe_repr(py, &value).0;
                        Ok(Some(value_repr))
                    })?;
                    if let Some(exception) = output.exception {
                        Ok(UserRunResult::Failed {
                            stdout: output.stdout,
                            stderr: output.stderr,
                            exception,
                        })
                    } else {
                        Ok(UserRunResult::Evaluated(EvalResult {
                            value_repr: output.value_repr.unwrap_or_default(),
                            stdout: output.stdout,
                            stderr: output.stderr,
                        }))
                    }
                }
                Err(err) => {
                    if err.is_instance_of::<pyo3::exceptions::PySyntaxError>(py) {
                        let output = self.capture_output(py, |py| {
                            let filename =
                                self.register_source(py, line, "exec").map_err(|err| {
                                    pyo3::exceptions::PyRuntimeError::new_err(err.to_string())
                                })?;
                            let globals = self.globals.bind(py);
                            let compiled = self.compile_source(py, line, &filename, "exec")?;
                            self.exec_compiled(py, globals, &compiled)?;
                            Ok(None)
                        })?;
                        if let Some(exception) = output.exception {
                            Ok(UserRunResult::Failed {
                                stdout: output.stdout,
                                stderr: output.stderr,
                                exception,
                            })
                        } else {
                            Ok(UserRunResult::Executed(ExecResult {
                                stdout: output.stdout,
                                stderr: output.stderr,
                            }))
                        }
                    } else {
                        let exception = self.capture_exception(py, &err)?;
                        self.store_last_exception(Some(exception.clone()))?;
                        Ok(UserRunResult::Failed {
                            stdout: String::new(),
                            stderr: String::new(),
                            exception,
                        })
                    }
                }
            }
        })
    }

    pub fn check_input_completeness(&self, source: &str) -> Result<InputCompleteness> {
        Python::attach(|py| -> Result<InputCompleteness> {
            let codeop = PyModule::import(py, "codeop")?;
            let compile_command = codeop.getattr("compile_command")?;
            let result = compile_command.call1((source, "<stdin>", "exec"));
            match result {
                Ok(value) => {
                    if value.is_none() {
                        Ok(InputCompleteness::Incomplete)
                    } else {
                        Ok(InputCompleteness::Complete)
                    }
                }
                Err(err)
                    if err.is_instance_of::<pyo3::exceptions::PySyntaxError>(py)
                        || err.is_instance_of::<pyo3::exceptions::PyOverflowError>(py)
                        || err.is_instance_of::<pyo3::exceptions::PyValueError>(py)
                        || err.is_instance_of::<pyo3::exceptions::PyTypeError>(py) =>
                {
                    Ok(InputCompleteness::Invalid)
                }
                Err(err) => {
                    let exception = self.capture_exception(py, &err)?;
                    self.store_last_exception(Some(exception.clone()))?;
                    anyhow::bail!("{}", exception.traceback)
                }
            }
        })
    }

    #[allow(dead_code)]
    pub fn list_globals(&self) -> Result<Vec<GlobalEntry>> {
        Python::attach(|py| -> Result<Vec<GlobalEntry>> {
            let globals = self.globals.bind(py);
            let mut entries = Vec::new();
            for (name, value) in globals.iter() {
                let name: String = name.extract()?;
                if name == "__builtins__" {
                    continue;
                }
                if name.starts_with("_pyaichat_") {
                    continue;
                }
                if name.starts_with("__") && name.ends_with("__") {
                    continue;
                }
                let type_name: String = value.get_type().name()?.extract()?;
                entries.push(GlobalEntry { name, type_name });
            }
            entries.sort_by(|left, right| left.name.cmp(&right.name));
            Ok(entries)
        })
    }

    #[allow(dead_code)]
    pub fn get_last_exception(&self) -> Result<Option<ExceptionInfo>> {
        self.last_exception
            .lock()
            .map(|value| value.clone())
            .map_err(|err| anyhow!("failed to lock last_exception: {err}"))
    }

    pub fn is_healthy(&self) -> bool {
        Python::attach(|py| {
            let globals = self.globals.bind(py);
            Self::health_check(py, globals).is_ok()
        })
    }

    fn health_check(py: Python<'_>, globals: &Bound<'_, PyDict>) -> PyResult<()> {
        let _ = py.eval(c"1 + 1", Some(globals), Some(globals))?;
        Ok(())
    }

    fn compile_source<'py>(
        &self,
        py: Python<'py>,
        source: &str,
        filename: &str,
        mode: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        let builtins = PyModule::import(py, "builtins")?;
        builtins.getattr("compile")?.call1((source, filename, mode))
    }

    fn exec_compiled(
        &self,
        py: Python<'_>,
        globals: &Bound<'_, PyDict>,
        compiled: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let builtins = PyModule::import(py, "builtins")?;
        let _ = builtins
            .getattr("exec")?
            .call1((compiled, globals, globals))?;
        Ok(())
    }

    fn eval_compiled<'py>(
        &self,
        py: Python<'py>,
        globals: &Bound<'py, PyDict>,
        compiled: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let builtins = PyModule::import(py, "builtins")?;
        builtins
            .getattr("eval")?
            .call1((compiled, globals, globals))
    }

    fn eval_compiled_with_timeout<'py>(
        &self,
        py: Python<'py>,
        globals: &Bound<'py, PyDict>,
        compiled: &Bound<'py, PyAny>,
        timeout_seconds: f64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let timeout_context = match self.inspect_timeout_context(py)? {
            Some(ctx) => ctx,
            None => return self.eval_compiled(py, globals, compiled),
        };

        let timeout_handler = PyModule::from_code(
            py,
            c"def _pyaichat_timeout_handler(_signum, _frame):
    raise TimeoutError('inspect timed out after 1.0 seconds')",
            c"<pyaichat-timeout-handler>",
            c"_pyaichat_timeout_handler",
        )?
        .getattr("_pyaichat_timeout_handler")?;

        timeout_context
            .signal
            .getattr("signal")?
            .call1((&timeout_context.sigalrm, &timeout_handler))?;
        timeout_context.signal.getattr("setitimer")?.call1((
            &timeout_context.itimer_real,
            timeout_seconds,
            0.0_f64,
        ))?;
        let inspect_started_at = std::time::Instant::now();

        let eval_result = self.eval_compiled(py, globals, compiled);
        let inspect_elapsed = inspect_started_at.elapsed().as_secs_f64();
        let restored_delay = if timeout_context.previous_timer.0 <= 0.0 {
            0.0_f64
        } else {
            let remaining = timeout_context.previous_timer.0 - inspect_elapsed;
            if remaining <= 0.0 {
                MIN_TIMER_DELAY_SECONDS
            } else {
                remaining
            }
        };

        let restore_result = timeout_context.signal.getattr("setitimer")?.call1((
            &timeout_context.itimer_real,
            restored_delay,
            timeout_context.previous_timer.1,
        ));
        let restore_handler_result = timeout_context
            .signal
            .getattr("signal")?
            .call1((&timeout_context.sigalrm, &timeout_context.previous_handler));
        restore_result?;
        restore_handler_result?;

        eval_result
    }

    fn inspect_timeout_context<'py>(
        &self,
        py: Python<'py>,
    ) -> PyResult<Option<InspectTimeoutContext<'py>>> {
        let signal = match PyModule::import(py, "signal") {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        let threading = match PyModule::import(py, "threading") {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        if !signal.hasattr("SIGALRM")? || !signal.hasattr("ITIMER_REAL")? {
            return Ok(None);
        }

        let current_thread = threading.getattr("current_thread")?.call0()?;
        let main_thread = threading.getattr("main_thread")?.call0()?;
        if !current_thread.is(&main_thread) {
            return Ok(None);
        }

        let sigalrm = signal.getattr("SIGALRM")?;
        let itimer_real = signal.getattr("ITIMER_REAL")?;
        let previous_handler = signal.getattr("getsignal")?.call1((&sigalrm,))?;
        let previous_timer: (f64, f64) = signal
            .getattr("getitimer")?
            .call1((&itimer_real,))?
            .extract()?;
        Ok(Some(InspectTimeoutContext {
            signal,
            sigalrm,
            itimer_real,
            previous_handler: previous_handler.unbind(),
            previous_timer,
        }))
    }

    fn register_source(&self, py: Python<'_>, source: &str, mode: &str) -> Result<String> {
        let counter = self.source_counter.fetch_add(1, Ordering::Relaxed) + 1;
        let filename = format!("<pyaichat-{mode}-{counter}>");
        let text = if source.ends_with('\n') {
            source.to_string()
        } else {
            format!("{source}\n")
        };

        let linecache = PyModule::import(py, "linecache")?;
        let cache = linecache.getattr("cache")?;
        let lines = text
            .lines()
            .map(|line| format!("{line}\n"))
            .collect::<Vec<_>>();
        let entry = (text.len(), py.None(), lines, filename.clone());
        cache.set_item(filename.as_str(), entry)?;
        Ok(filename)
    }

    fn safe_repr(&self, _py: Python<'_>, value: &Bound<'_, PyAny>) -> (String, Option<String>) {
        match value.repr() {
            Ok(text) => match text.extract::<String>() {
                Ok(text) => (text, None),
                Err(err) => (
                    format!("<repr failed: TypeError: {err}>"),
                    Some(format!("TypeError: {err}")),
                ),
            },
            Err(err) => {
                let err_type = err
                    .get_type(_py)
                    .name()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|_| "Exception".to_string());
                let message = err
                    .value(_py)
                    .str()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "repr failed".to_string());
                (
                    format!("<repr failed: {err_type}: {message}>"),
                    Some(format!("{err_type}: {message}")),
                )
            }
        }
    }

    fn truncate_text(value: &str, max_chars: usize) -> (String, bool, usize) {
        let original_len = value.chars().count();
        if original_len <= max_chars {
            return (value.to_string(), false, original_len);
        }
        let text = value.chars().take(max_chars).collect::<String>();
        (text, true, original_len)
    }

    fn capture_exception(&self, py: Python<'_>, err: &PyErr) -> Result<ExceptionInfo> {
        let exc_type = err.get_type(py).name()?.to_string();
        let message = err
            .value(py)
            .str()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|_| String::new());

        let traceback_module = PyModule::import(py, "traceback")?;
        let formatted = traceback_module.getattr("format_exception")?.call1((
            err.get_type(py),
            err.value(py),
            err.traceback(py),
        ))?;
        let mut traceback = String::new();
        let lines = formatted
            .cast::<PyList>()
            .map_err(|e| anyhow!(e.to_string()))?;
        for line in lines.iter() {
            let line = line.extract::<String>()?;
            traceback.push_str(&line);
        }

        Ok(ExceptionInfo {
            exc_type,
            message,
            traceback,
        })
    }

    fn store_last_exception(&self, value: Option<ExceptionInfo>) -> Result<()> {
        let mut guard = self
            .last_exception
            .lock()
            .map_err(|err| anyhow!("failed to lock last_exception: {err}"))?;
        *guard = value;
        Ok(())
    }

    fn eval_expr_inner(&self, py: Python<'_>, expr: &str) -> Result<EvalResult, ExceptionInfo> {
        let globals = self.globals.bind(py);
        let output = self.capture_output(py, |py| {
            let filename = self
                .register_source(py, expr, "eval")
                .map_err(|err| pyo3::exceptions::PyRuntimeError::new_err(err.to_string()))?;
            let compiled = self.compile_source(py, expr, &filename, "eval")?;
            let value = self.eval_compiled(py, globals, &compiled)?;
            let value_repr = self.safe_repr(py, &value).0;
            Ok(Some(value_repr))
        });

        match output {
            Ok(output) => {
                if let Some(exception) = output.exception {
                    Err(exception)
                } else {
                    Ok(EvalResult {
                        value_repr: output.value_repr.unwrap_or_default(),
                        stdout: output.stdout,
                        stderr: output.stderr,
                    })
                }
            }
            Err(err) => {
                let exception = ExceptionInfo {
                    exc_type: "RuntimeError".to_string(),
                    message: err.to_string(),
                    traceback: err.to_string(),
                };
                let _ = self.store_last_exception(Some(exception.clone()));
                Err(exception)
            }
        }
    }

    fn exec_code_inner(&self, py: Python<'_>, code: &str) -> Result<ExecResult, ExceptionInfo> {
        let globals = self.globals.bind(py);
        let output = self.capture_output(py, |py| {
            let filename = self
                .register_source(py, code, "exec")
                .map_err(|err| pyo3::exceptions::PyRuntimeError::new_err(err.to_string()))?;
            let compiled = self.compile_source(py, code, &filename, "exec")?;
            self.exec_compiled(py, globals, &compiled)?;
            Ok(None)
        });

        match output {
            Ok(output) => {
                if let Some(exception) = output.exception {
                    Err(exception)
                } else {
                    Ok(ExecResult {
                        stdout: output.stdout,
                        stderr: output.stderr,
                    })
                }
            }
            Err(err) => {
                let exception = ExceptionInfo {
                    exc_type: "RuntimeError".to_string(),
                    message: err.to_string(),
                    traceback: err.to_string(),
                };
                let _ = self.store_last_exception(Some(exception.clone()));
                Err(exception)
            }
        }
    }

    fn inspect_expr(&self, py: Python<'_>, expr: &str) -> CapabilityResult<Value> {
        let globals = self.globals.bind(py);
        let value = match self.compile_source(py, expr, "<inspect>", "eval") {
            Ok(compiled) => match self.eval_compiled_with_timeout(
                py,
                globals,
                &compiled,
                INSPECT_EVAL_TIMEOUT_SECONDS,
            ) {
                Ok(value) => value,
                Err(err) => {
                    let exception = self
                        .capture_exception(py, &err)
                        .map_err(Self::cap_internal)?;
                    let _ = self.store_last_exception(Some(exception.clone()));
                    return Err(CapabilityError::PythonException(exception));
                }
            },
            Err(err) => {
                let exception = self
                    .capture_exception(py, &err)
                    .map_err(Self::cap_internal)?;
                let _ = self.store_last_exception(Some(exception.clone()));
                return Err(CapabilityError::PythonException(exception));
            }
        };

        self.build_inspect_payload(py, &value)
            .map_err(CapabilityError::PythonException)
    }

    fn build_inspect_payload(
        &self,
        py: Python<'_>,
        value: &Bound<'_, PyAny>,
    ) -> Result<Value, ExceptionInfo> {
        let kind = self.kind_of(py, value);
        let (repr_text, repr_error) = self.safe_repr(py, value);
        let (repr_text, repr_truncated, repr_original_len) =
            Self::truncate_text(&repr_text, super::capabilities::REPR_MAX_LEN);

        let doc_payload = self.doc_payload(py, value);
        let mut payload = serde_json::json!({
            "type": self.type_payload(py, value),
            "kind": kind,
            "repr": {
                "text": repr_text,
                "truncated": repr_truncated,
                "original_len": repr_original_len,
            },
            "doc": doc_payload,
            "members": self.members_payload(py, value),
            "limits": {
                "repr_max_chars": super::capabilities::REPR_MAX_LEN,
                "doc_max_chars": super::capabilities::DOC_MAX_LEN,
                "sample_max_items": super::capabilities::INSPECT_SAMPLE_MAX_ITEMS,
                "member_max_per_group": super::capabilities::INSPECT_MEMBER_MAX_PER_GROUP,
                "source_preview_max_chars": super::capabilities::INSPECT_SOURCE_PREVIEW_MAX_LEN,
            }
        });
        if let Some(error) = repr_error {
            payload["repr"]["repr_error"] = Value::String(error);
        }
        if let Some(size) = self.size_payload(py, value) {
            payload["size"] = size;
        }
        if let Some(sample) = self.sample_payload(py, value, &kind) {
            payload["sample"] = sample;
        }
        if value.is_callable() {
            payload["callable"] = self.callable_payload(py, value);
        }
        if self.is_exception_instance(py, value) {
            let exc_type = value
                .get_type()
                .name()
                .map(|n| n.to_string())
                .unwrap_or_else(|_| "Exception".to_string());
            let message = value
                .str()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            payload["exception"] = serde_json::json!({
                "exc_type": exc_type,
                "message": message,
            });
        }
        Ok(payload)
    }

    fn kind_of(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> String {
        if value.is_none() {
            return "none".to_string();
        }
        if value.is_instance_of::<pyo3::types::PyBool>() {
            return "bool".to_string();
        }
        if value.is_instance_of::<pyo3::types::PyInt>()
            || value.is_instance_of::<PyFloat>()
            || value.is_instance_of::<pyo3::types::PyComplex>()
        {
            return "number".to_string();
        }
        if value.is_instance_of::<PyString>() {
            return "string".to_string();
        }
        if value.is_instance_of::<pyo3::types::PyBytes>()
            || value.is_instance_of::<pyo3::types::PyByteArray>()
        {
            return "bytes".to_string();
        }
        if value.cast::<PyDict>().is_ok() {
            return "mapping".to_string();
        }
        if value.is_instance_of::<PyList>() || value.is_instance_of::<PyTuple>() {
            return "sequence".to_string();
        }
        let type_name = value
            .get_type()
            .name()
            .map(|v| v.to_string())
            .unwrap_or_default();
        if matches!(type_name.as_str(), "memoryview") {
            return "bytes".to_string();
        }
        if matches!(type_name.as_str(), "range") {
            return "sequence".to_string();
        }
        if matches!(type_name.as_str(), "set" | "frozenset") {
            return "set".to_string();
        }

        if let Ok(inspect) = PyModule::import(py, "inspect") {
            if inspect
                .getattr("isasyncgen")
                .and_then(|f| f.call1((value,)))
                .and_then(|v| v.extract::<bool>())
                .unwrap_or(false)
            {
                return "async_generator".to_string();
            }
            if inspect
                .getattr("iscoroutine")
                .and_then(|f| f.call1((value,)))
                .and_then(|v| v.extract::<bool>())
                .unwrap_or(false)
            {
                return "coroutine".to_string();
            }
            if inspect
                .getattr("isgenerator")
                .and_then(|f| f.call1((value,)))
                .and_then(|v| v.extract::<bool>())
                .unwrap_or(false)
            {
                return "generator".to_string();
            }
            if inspect
                .getattr("ismodule")
                .and_then(|f| f.call1((value,)))
                .and_then(|v| v.extract::<bool>())
                .unwrap_or(false)
            {
                return "module".to_string();
            }
            if inspect
                .getattr("isclass")
                .and_then(|f| f.call1((value,)))
                .and_then(|v| v.extract::<bool>())
                .unwrap_or(false)
            {
                return "class".to_string();
            }
        }

        if let (Ok(collections_abc), Ok(builtins)) = (
            PyModule::import(py, "collections.abc"),
            PyModule::import(py, "builtins"),
        ) && let Ok(iterator_type) = collections_abc.getattr("Iterator")
            && builtins
                .getattr("isinstance")
                .and_then(|f| f.call1((value, iterator_type)))
                .and_then(|v| v.extract::<bool>())
                .unwrap_or(false)
        {
            return "iterator".to_string();
        }

        if self.is_exception_instance(py, value) {
            return "exception".to_string();
        }
        if value.is_callable() {
            return "callable".to_string();
        }
        "object".to_string()
    }

    fn is_exception_instance(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> bool {
        let builtins = match PyModule::import(py, "builtins") {
            Ok(v) => v,
            Err(_) => return false,
        };
        let base_exception = match builtins.getattr("BaseException") {
            Ok(v) => v,
            Err(_) => return false,
        };
        value.is_instance(&base_exception).unwrap_or(false)
    }

    fn type_payload(&self, _py: Python<'_>, value: &Bound<'_, PyAny>) -> Value {
        let value_type = value.get_type();
        let name = value_type.name().map(|v| v.to_string()).unwrap_or_default();
        let module = value_type
            .getattr("__module__")
            .and_then(|v| v.extract::<String>())
            .unwrap_or_default();
        let qualified_name = value_type
            .getattr("__qualname__")
            .and_then(|v| v.extract::<String>())
            .unwrap_or_else(|_| name.clone());
        let qualified = if module.is_empty() {
            qualified_name
        } else {
            format!("{module}.{qualified_name}")
        };
        serde_json::json!({
            "name": name,
            "module": module,
            "qualified": qualified,
        })
    }

    fn doc_payload(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> Value {
        let doc = match value.getattr("__doc__") {
            Ok(v) => v,
            Err(err) => {
                let exc = self.capture_exception(py, &err).ok();
                return serde_json::json!({
                    "text": Value::Null,
                    "truncated": false,
                    "original_len": 0,
                    "doc_error": exc.map(|v| format!("{}: {}", v.exc_type, v.message)).unwrap_or_else(|| "doc error".to_string()),
                });
            }
        };

        if doc.is_none() {
            return serde_json::json!({"text": Value::Null, "truncated": false, "original_len": 0});
        }

        let as_string = doc
            .extract::<String>()
            .or_else(|_| doc.str().map(|v| v.to_string_lossy().into_owned()));
        match as_string {
            Ok(text) => {
                let (text, truncated, original_len) =
                    Self::truncate_text(&text, super::capabilities::DOC_MAX_LEN);
                serde_json::json!({
                    "text": text,
                    "truncated": truncated,
                    "original_len": original_len,
                })
            }
            Err(err) => serde_json::json!({
                "text": Value::Null,
                "truncated": false,
                "original_len": 0,
                "doc_error": err.to_string(),
            }),
        }
    }

    fn size_payload(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> Option<Value> {
        let mut object = serde_json::Map::new();
        if let Ok(length) = value.len() {
            object.insert("len".to_string(), serde_json::json!(length));
        }
        if let Ok(shape) = value.getattr("shape")
            && !shape.is_none()
        {
            let mut shape_values = Vec::new();
            if let Ok(iter) = shape.try_iter() {
                for item in iter.flatten() {
                    shape_values.push(Value::String(self.safe_repr(py, &item).0));
                }
            } else {
                shape_values.push(Value::String(self.safe_repr(py, &shape).0));
            }
            object.insert("shape".to_string(), Value::Array(shape_values));
        }
        if object.is_empty() {
            return None;
        }
        Some(Value::Object(object))
    }

    fn sample_payload(
        &self,
        _py: Python<'_>,
        value: &Bound<'_, PyAny>,
        kind: &str,
    ) -> Option<Value> {
        if matches!(
            kind,
            "generator" | "iterator" | "coroutine" | "async_generator"
        ) {
            return None;
        }

        let mut items = Vec::new();
        if let Ok(dict) = value.cast::<PyDict>() {
            for (key, item) in dict.iter() {
                if items.len() >= super::capabilities::INSPECT_SAMPLE_MAX_ITEMS {
                    break;
                }
                let key_repr = self.safe_repr(_py, &key).0;
                let item_repr = self.safe_repr(_py, &item).0;
                items.push(Value::String(format!("{key_repr}: {item_repr}")));
            }
        } else if kind == "sequence" || kind == "set" {
            let type_name = value
                .get_type()
                .name()
                .map(|v| v.to_string())
                .unwrap_or_default();
            if !matches!(
                type_name.as_str(),
                "list" | "tuple" | "range" | "set" | "frozenset"
            ) {
                return None;
            }
            let Ok(iter) = value.try_iter() else {
                return None;
            };
            for item in iter.flatten() {
                if items.len() >= super::capabilities::INSPECT_SAMPLE_MAX_ITEMS {
                    break;
                }
                items.push(Value::String(self.safe_repr(_py, &item).0));
            }
        } else {
            return None;
        }

        let total = value.len().unwrap_or(items.len());
        Some(serde_json::json!({
            "items": items,
            "shown": items.len(),
            "total": total,
            "truncated": total > items.len(),
        }))
    }

    fn members_payload(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> Value {
        let builtins = match PyModule::import(py, "builtins") {
            Ok(v) => v,
            Err(err) => {
                let details = self
                    .capture_exception(py, &err)
                    .map(|v| format!("{}: {}", v.exc_type, v.message))
                    .unwrap_or_else(|_| err.to_string());
                return serde_json::json!({
                    "data": [],
                    "callables": [],
                    "dunder_count": 0,
                    "shown_per_group": super::capabilities::INSPECT_MEMBER_MAX_PER_GROUP,
                    "truncated": false,
                    "members_error": details,
                });
            }
        };

        let dir = match builtins.getattr("dir").and_then(|f| f.call1((value,))) {
            Ok(v) => v,
            Err(err) => {
                let details = self
                    .capture_exception(py, &err)
                    .map(|v| format!("{}: {}", v.exc_type, v.message))
                    .unwrap_or_else(|_| err.to_string());
                return serde_json::json!({
                    "data": [],
                    "callables": [],
                    "dunder_count": 0,
                    "shown_per_group": super::capabilities::INSPECT_MEMBER_MAX_PER_GROUP,
                    "truncated": false,
                    "members_error": details,
                });
            }
        };

        let mut names = match dir.extract::<Vec<String>>() {
            Ok(v) => v,
            Err(err) => {
                return serde_json::json!({
                    "data": [],
                    "callables": [],
                    "dunder_count": 0,
                    "shown_per_group": super::capabilities::INSPECT_MEMBER_MAX_PER_GROUP,
                    "truncated": false,
                    "members_error": format!("TypeError: {err}"),
                });
            }
        };
        names.sort();

        let inspect = PyModule::import(py, "inspect").ok();
        let mut dunder_count = 0usize;
        let mut data = Vec::new();
        let mut callables = Vec::new();
        let mut non_dunder_total = 0usize;

        for name in names {
            if name.starts_with("__") && name.ends_with("__") {
                dunder_count += 1;
                continue;
            }
            non_dunder_total += 1;

            let attr = if let Some(inspect) = inspect.as_ref() {
                inspect
                    .getattr("getattr_static")
                    .and_then(|f| f.call1((value, name.as_str())))
                    .ok()
            } else {
                None
            };

            let is_callable = attr.as_ref().map(|v| v.is_callable()).unwrap_or(false);
            if is_callable {
                if callables.len() < super::capabilities::INSPECT_MEMBER_MAX_PER_GROUP {
                    callables.push(Value::String(name));
                }
            } else if data.len() < super::capabilities::INSPECT_MEMBER_MAX_PER_GROUP {
                data.push(Value::String(name));
            }
        }

        serde_json::json!({
            "data": data,
            "callables": callables,
            "dunder_count": dunder_count,
            "shown_per_group": super::capabilities::INSPECT_MEMBER_MAX_PER_GROUP,
            "truncated": non_dunder_total > (data.len() + callables.len()),
        })
    }

    fn callable_payload(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> Value {
        let module = value
            .getattr("__module__")
            .ok()
            .and_then(|v| v.extract::<String>().ok());
        let inspect = PyModule::import(py, "inspect").ok();
        let signature = inspect
            .as_ref()
            .and_then(|module| module.getattr("signature").ok())
            .and_then(|f| f.call1((value,)).ok())
            .and_then(|v| v.str().ok())
            .map(|v| v.to_string_lossy().into_owned());
        let doc_text = self
            .doc_payload(py, value)
            .get("text")
            .cloned()
            .unwrap_or(Value::Null);

        let source = inspect
            .as_ref()
            .and_then(|module| module.getattr("getsource").ok())
            .and_then(|f| f.call1((value,)).ok())
            .and_then(|v| v.extract::<String>().ok());
        let (source_preview, source_truncated, source_error) = match source {
            Some(text) => {
                let (text, truncated, _) =
                    Self::truncate_text(&text, super::capabilities::INSPECT_SOURCE_PREVIEW_MAX_LEN);
                (Value::String(text), Value::Bool(truncated), Value::Null)
            }
            None => (
                Value::Null,
                Value::Bool(false),
                Value::String("source unavailable".to_string()),
            ),
        };

        let mut payload = serde_json::Map::new();
        payload.insert("module".to_string(), serde_json::json!(module));
        payload.insert("signature".to_string(), serde_json::json!(signature));
        payload.insert("doc".to_string(), doc_text);
        payload.insert("source_preview".to_string(), source_preview);
        payload.insert("source_truncated".to_string(), source_truncated);
        if !source_error.is_null() {
            payload.insert("source_error".to_string(), source_error);
        }
        Value::Object(payload)
    }

    fn capture_output<F>(&self, py: Python<'_>, operation: F) -> Result<CapturedOutput>
    where
        F: FnOnce(Python<'_>) -> PyResult<Option<String>>,
    {
        let sys = PyModule::import(py, "sys")?;
        let io = PyModule::import(py, "io")?;
        let stdout_buffer = io.getattr("StringIO")?.call0()?;
        let stderr_buffer = io.getattr("StringIO")?.call0()?;
        let previous_stdout = sys.getattr("stdout")?.unbind();
        let previous_stderr = sys.getattr("stderr")?.unbind();
        sys.setattr("stdout", &stdout_buffer)?;
        sys.setattr("stderr", &stderr_buffer)?;
        let mut redirect_guard = StdioRedirectGuard::new(sys, previous_stdout, previous_stderr);

        let operation_result = operation(py);
        let stdout = stdout_buffer
            .getattr("getvalue")?
            .call0()?
            .extract::<String>()?;
        let stderr = stderr_buffer
            .getattr("getvalue")?
            .call0()?
            .extract::<String>()?;
        redirect_guard.restore()?;

        match operation_result {
            Ok(value_repr) => Ok(CapturedOutput {
                stdout,
                stderr,
                value_repr,
                exception: None,
            }),
            Err(err) => {
                let exception = self.capture_exception(py, &err)?;
                self.store_last_exception(Some(exception.clone()))?;
                Ok(CapturedOutput {
                    stdout,
                    stderr,
                    value_repr: None,
                    exception: Some(exception),
                })
            }
        }
    }
}

impl PythonSession {
    fn cap_internal(err: impl std::fmt::Display) -> CapabilityError {
        CapabilityError::Internal(err.to_string())
    }
}

#[allow(dead_code)]
impl CapabilityProvider for PythonSession {
    fn list_globals(&self) -> CapabilityResult<Vec<GlobalEntry>> {
        PythonSession::list_globals(self).map_err(Self::cap_internal)
    }

    fn inspect(&self, expr: &str) -> CapabilityResult<InspectInfo> {
        Python::attach(|py| {
            self.inspect_expr(py, expr)
                .map(|value| InspectInfo { value })
        })
    }

    fn eval_expr(&self, expr: &str) -> CapabilityResult<EvalInfo> {
        Python::attach(|py| match self.eval_expr_inner(py, expr) {
            Ok(result) => Ok(EvalInfo {
                value_repr: result.value_repr,
                stdout: result.stdout,
                stderr: result.stderr,
            }),
            Err(exception) => Err(CapabilityError::PythonException(exception)),
        })
    }
}

struct CapturedOutput {
    stdout: String,
    stderr: String,
    value_repr: Option<String>,
    exception: Option<ExceptionInfo>,
}

struct StdioRedirectGuard<'py> {
    sys: Bound<'py, PyModule>,
    previous_stdout: Py<PyAny>,
    previous_stderr: Py<PyAny>,
    restored: bool,
}

impl<'py> StdioRedirectGuard<'py> {
    fn new(
        sys: Bound<'py, PyModule>,
        previous_stdout: Py<PyAny>,
        previous_stderr: Py<PyAny>,
    ) -> Self {
        Self {
            sys,
            previous_stdout,
            previous_stderr,
            restored: false,
        }
    }

    fn restore(&mut self) -> Result<()> {
        let py = self.sys.py();
        let stdout_restore = self.sys.setattr("stdout", self.previous_stdout.bind(py));
        let stderr_restore = self.sys.setattr("stderr", self.previous_stderr.bind(py));
        match (stdout_restore, stderr_restore) {
            (Ok(_), Ok(_)) => {
                self.restored = true;
                Ok(())
            }
            (Err(stdout_err), Err(stderr_err)) => anyhow::bail!(
                "failed to restore sys.stdout ({stdout_err}) and sys.stderr ({stderr_err})"
            ),
            (Err(stdout_err), Ok(_)) => anyhow::bail!("failed to restore sys.stdout: {stdout_err}"),
            (Ok(_), Err(stderr_err)) => anyhow::bail!("failed to restore sys.stderr: {stderr_err}"),
        }
    }
}

impl Drop for StdioRedirectGuard<'_> {
    fn drop(&mut self) {
        if self.restored {
            return;
        }
        let py = self.sys.py();
        let _ = self.sys.setattr("stdout", self.previous_stdout.bind(py));
        let _ = self.sys.setattr("stderr", self.previous_stderr.bind(py));
    }
}

struct InspectTimeoutContext<'py> {
    signal: Bound<'py, PyModule>,
    sigalrm: Bound<'py, PyAny>,
    itimer_real: Bound<'py, PyAny>,
    previous_handler: Py<PyAny>,
    previous_timer: (f64, f64),
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::{LazyLock, Mutex};
    use std::time::{Duration, Instant};

    use pyo3::types::{PyAnyMethods, PyModule};
    use pyo3::{PyResult, Python};

    use crate::python::{CapabilityError, CapabilityProvider};

    use super::{InputCompleteness, PythonSession, UserRunResult};

    static SIGNAL_TEST_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn exec_persists_state_across_calls() {
        let session = PythonSession::initialize().expect("python session");
        session.exec_code("x = 41").expect("exec set x");
        let eval = session.eval_expr("x + 1").expect("eval x");
        assert_eq!(eval.value_repr, "42");
    }

    #[test]
    fn eval_sees_prior_exec_defined_globals() {
        let session = PythonSession::initialize().expect("python session");
        session.exec_code("value = {'a': 1}").expect("exec value");
        let eval = session.eval_expr("value['a']").expect("eval value");
        assert_eq!(eval.value_repr, "1");
    }

    #[test]
    fn run_user_input_hybrid_dispatches_eval_and_exec() {
        let session = PythonSession::initialize().expect("python session");
        let evaluated = session
            .run_user_input("1 + 2")
            .expect("evaluate expression");
        assert!(matches!(
            evaluated,
            UserRunResult::Evaluated(ref r) if r.value_repr == "3"
        ));

        let executed = session
            .run_user_input("answer = 123")
            .expect("execute statement");
        assert!(matches!(executed, UserRunResult::Executed(_)));

        let roundtrip = session.eval_expr("answer").expect("eval answer");
        assert_eq!(roundtrip.value_repr, "123");
    }

    #[test]
    fn input_completeness_classifies_complete_incomplete_and_invalid() {
        let session = PythonSession::initialize().expect("python session");
        assert_eq!(
            session
                .check_input_completeness("x = 1")
                .expect("complete status"),
            InputCompleteness::Complete
        );
        assert_eq!(
            session
                .check_input_completeness("if True:")
                .expect("incomplete status"),
            InputCompleteness::Incomplete
        );
        assert_eq!(
            session
                .check_input_completeness("if True")
                .expect("invalid status"),
            InputCompleteness::Invalid
        );
    }

    #[test]
    fn capture_output_restores_std_streams_after_panic() {
        let session = PythonSession::initialize().expect("python session");
        Python::attach(|py| {
            let sys = PyModule::import(py, "sys").expect("sys module");
            let before_stdout = sys.getattr("stdout").expect("stdout before").unbind();
            let before_stderr = sys.getattr("stderr").expect("stderr before").unbind();

            let _ = catch_unwind(AssertUnwindSafe(|| {
                let _ = session.capture_output(py, |_py| -> PyResult<Option<String>> {
                    panic!("forced panic while stdio is redirected");
                });
            }));

            let after_stdout = sys.getattr("stdout").expect("stdout after");
            let after_stderr = sys.getattr("stderr").expect("stderr after");
            assert!(after_stdout.is(before_stdout.bind(py)));
            assert!(after_stderr.is(before_stderr.bind(py)));
        });
    }

    #[test]
    fn captures_stdout_and_stderr() {
        let session = PythonSession::initialize().expect("python session");
        let result = session
            .exec_code("import sys\nprint('hello')\nprint('oops', file=sys.stderr)")
            .expect("exec with output");
        assert_eq!(result.stdout, "hello\n");
        assert_eq!(result.stderr, "oops\n");
    }

    #[test]
    fn list_globals_returns_name_and_type_excluding_internals() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code("x = 10\ndef fn():\n    return x")
            .expect("seed globals");
        let globals = session.list_globals().expect("list globals");

        assert!(
            globals
                .iter()
                .any(|entry| entry.name == "x" && entry.type_name == "int")
        );
        assert!(
            globals
                .iter()
                .any(|entry| entry.name == "fn" && entry.type_name == "function")
        );
        assert!(!globals.iter().any(|entry| entry.name == "__builtins__"));
        assert!(!globals.iter().any(|entry| entry.name == "__name__"));
        assert!(
            !globals
                .iter()
                .any(|entry| entry.name.starts_with("_pyaichat_"))
        );
    }

    #[test]
    fn exception_payload_contains_type_message_and_traceback() {
        let session = PythonSession::initialize().expect("python session");
        let result = session.run_user_input("1 / 0").expect("run failure");
        assert!(matches!(result, UserRunResult::Failed { .. }));

        let exception = session
            .get_last_exception()
            .expect("get last exception")
            .expect("exception present");
        assert_eq!(exception.exc_type, "ZeroDivisionError");
        assert!(exception.message.contains("division by zero"));
        assert!(exception.traceback.contains("Traceback"));
        assert!(exception.traceback.contains("ZeroDivisionError"));
    }

    #[test]
    fn last_exception_persists_after_success_and_is_replaced_on_new_failure() {
        let session = PythonSession::initialize().expect("python session");
        session.run_user_input("1/0").expect("first failure");
        let first = session
            .get_last_exception()
            .expect("get first exception")
            .expect("first exception exists");
        assert_eq!(first.exc_type, "ZeroDivisionError");

        session.run_user_input("x = 5").expect("success command");
        let persisted = session
            .get_last_exception()
            .expect("get persisted exception");
        if let Some(persisted) = persisted {
            assert_eq!(persisted.exc_type, "ZeroDivisionError");
        }

        session
            .run_user_input("unknown_name")
            .expect("second failure call returns structured failed result");
        let replaced = session
            .get_last_exception()
            .expect("get replaced exception")
            .expect("replaced exception exists");
        assert_eq!(replaced.exc_type, "NameError");
    }

    #[test]
    fn capability_eval_expr_returns_value_and_output_streams() {
        let session = PythonSession::initialize().expect("python session");
        let eval = CapabilityProvider::eval_expr(
            &session,
            "(print('out'), __import__('sys').stderr.write('err'), 7)[2]",
        )
        .expect("capability eval");
        assert_eq!(eval.value_repr, "7");
        assert_eq!(eval.stdout, "out\n");
        assert_eq!(eval.stderr, "err");
    }

    #[test]
    fn capability_inspect_returns_type_and_kind() {
        let session = PythonSession::initialize().expect("python session");
        let inspect = CapabilityProvider::inspect(&session, "42").expect("inspect");
        assert_eq!(inspect.value["kind"], "number");
        assert_eq!(inspect.value["type"]["name"], "int");
    }

    #[test]
    fn capability_inspect_list_has_size_and_sample_metadata() {
        let session = PythonSession::initialize().expect("python session");
        let inspect = CapabilityProvider::inspect(&session, "list(range(30))").expect("inspect");
        assert_eq!(inspect.value["kind"], "sequence");
        assert_eq!(inspect.value["size"]["len"], 30);
        assert_eq!(inspect.value["sample"]["shown"], 16);
        assert_eq!(inspect.value["sample"]["total"], 30);
        assert_eq!(inspect.value["sample"]["truncated"], true);
    }

    #[test]
    fn capability_inspect_none_reports_none_kind() {
        let session = PythonSession::initialize().expect("python session");
        let inspect = CapabilityProvider::inspect(&session, "None").expect("inspect");
        assert_eq!(inspect.value["kind"], "none");
    }

    #[test]
    fn capability_inspect_callable_reports_signature_and_source() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code("def fn(x):\n    return x + 1")
            .expect("seed function");
        let inspect = CapabilityProvider::inspect(&session, "fn").expect("inspect");
        assert_eq!(inspect.value["kind"], "callable");
        assert_eq!(inspect.value["callable"]["module"], "__main__");
        assert_eq!(inspect.value["callable"]["signature"], "(x)");
        assert!(
            inspect.value["callable"]["source_preview"]
                .as_str()
                .is_some_and(|s| s.contains("def fn(x):"))
        );
    }

    #[test]
    fn capability_inspect_repl_defined_callable_includes_source_preview() {
        let session = PythonSession::initialize().expect("python session");
        session
            .run_user_input("def next(x):\n    x + 1")
            .expect("define function via run_user_input");

        let inspect = CapabilityProvider::inspect(&session, "next").expect("inspect");
        assert_eq!(inspect.value["kind"], "callable");
        assert_eq!(inspect.value["callable"]["module"], "__main__");
        assert_eq!(inspect.value["callable"]["signature"], "(x)");
        assert!(
            inspect.value["callable"]["source_preview"]
                .as_str()
                .is_some_and(|s| s.contains("def next(x):"))
        );
    }

    #[test]
    fn capability_inspect_exception_instance_includes_exception_section() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code(
                "try:\n    raise ValueError('boom')\nexcept ValueError as exc:\n    saved_exc = exc",
            )
            .expect("seed exception");
        let inspect = CapabilityProvider::inspect(&session, "saved_exc").expect("inspect");
        assert_eq!(inspect.value["kind"], "exception");
        assert_eq!(inspect.value["exception"]["exc_type"], "ValueError");
        assert_eq!(inspect.value["exception"]["message"], "boom");
    }

    #[test]
    fn capability_inspect_handles_circular_containers() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code("x = []\nx.append(x)")
            .expect("seed circular");
        let inspect = CapabilityProvider::inspect(&session, "x").expect("inspect");
        assert_eq!(inspect.value["kind"], "sequence");
        assert_eq!(inspect.value["size"]["len"], 1);
        assert_eq!(inspect.value["sample"]["shown"], 1);
    }

    #[test]
    fn capability_inspect_handles_broken_repr() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code(
                "class BrokenRepr:\n    def __repr__(self):\n        raise RuntimeError('repr boom')\nobj = BrokenRepr()",
            )
            .expect("seed broken repr");
        let inspect = CapabilityProvider::inspect(&session, "obj").expect("inspect");
        assert_eq!(inspect.value["kind"], "object");
        assert!(
            inspect.value["repr"]["repr_error"]
                .as_str()
                .is_some_and(|s| s.contains("repr boom"))
        );
    }

    #[test]
    fn capability_inspect_members_avoids_property_side_effects() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code(
                "class SideEffect:\n    hits = 0\n    @property\n    def boom(self):\n        SideEffect.hits += 1\n        return 1\nobj = SideEffect()",
            )
            .expect("seed side-effect property");

        let inspect = CapabilityProvider::inspect(&session, "obj").expect("inspect");
        assert!(
            inspect.value["members"]["data"]
                .as_array()
                .is_some_and(|members| members.iter().any(|member| member == "boom"))
        );

        let hits = session
            .eval_expr("SideEffect.hits")
            .expect("read hit counter");
        assert_eq!(hits.value_repr, "0");
    }

    #[test]
    fn capability_inspect_large_range_sampling_stays_bounded() {
        let session = PythonSession::initialize().expect("python session");
        let inspect = CapabilityProvider::inspect(&session, "range(10**12)").expect("inspect");
        assert_eq!(inspect.value["kind"], "sequence");
        assert_eq!(inspect.value["sample"]["shown"], 16);
        assert_eq!(inspect.value["sample"]["total"], 1_000_000_000_000_u64);
        assert_eq!(inspect.value["sample"]["truncated"], true);
    }

    #[test]
    fn capability_inspect_iterator_does_not_advance_state() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code("it = iter([1, 2, 3])")
            .expect("seed iterator");

        let inspect = CapabilityProvider::inspect(&session, "it").expect("inspect");
        assert_eq!(inspect.value["kind"], "iterator");
        assert!(inspect.value.get("sample").is_none());

        let next_value = session.eval_expr("next(it)").expect("next iterator");
        assert_eq!(next_value.value_repr, "1");
    }

    #[test]
    fn capability_inspect_generator_reports_generator_kind() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code("g = (n for n in [1, 2, 3])")
            .expect("seed generator");

        let inspect = CapabilityProvider::inspect(&session, "g").expect("inspect");
        assert_eq!(inspect.value["kind"], "generator");
    }

    #[test]
    fn capability_inspect_does_not_iterate_custom_iterables_for_sampling() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code(
                "class CustomIterable:\n    iter_calls = 0\n    def __iter__(self):\n        CustomIterable.iter_calls += 1\n        return iter([1, 2, 3])\nobj = CustomIterable()",
            )
            .expect("seed custom iterable");

        let inspect = CapabilityProvider::inspect(&session, "obj").expect("inspect");
        assert_eq!(inspect.value["kind"], "object");
        assert!(inspect.value.get("sample").is_none());

        let iter_calls = session
            .eval_expr("CustomIterable.iter_calls")
            .expect("read iter call count");
        assert_eq!(iter_calls.value_repr, "0");
    }

    #[test]
    fn capability_inspect_handles_broken_dir_without_failing() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code(
                "class BrokenDir:\n    def __dir__(self):\n        raise RuntimeError('dir boom')\nobj = BrokenDir()",
            )
            .expect("seed broken dir");

        let inspect = CapabilityProvider::inspect(&session, "obj").expect("inspect");
        assert_eq!(inspect.value["kind"], "object");
        assert_eq!(inspect.value["members"]["data"], serde_json::json!([]));
        assert_eq!(inspect.value["members"]["callables"], serde_json::json!([]));
        assert!(
            inspect.value["members"]["members_error"]
                .as_str()
                .is_some_and(|s| s.contains("dir boom"))
        );
    }

    #[test]
    fn capability_inspect_timeout_restores_existing_alarm_handler_and_timer() {
        let _signal_guard = SIGNAL_TEST_MUTEX.lock().expect("lock signal test mutex");
        let session = PythonSession::initialize().expect("python session");
        let timeout_supported = session
            .eval_expr("hasattr(__import__('signal'), 'SIGALRM') and hasattr(__import__('signal'), 'ITIMER_REAL')")
            .expect("check signal")
            .value_repr;
        let runs_on_main_thread = session
            .eval_expr(
                "__import__('threading').current_thread() is __import__('threading').main_thread()",
            )
            .expect("check thread")
            .value_repr;
        if timeout_supported != "True" || runs_on_main_thread != "True" {
            return;
        }

        session
            .exec_code(
                r#"import signal
def _test_alarm_handler(_signum, _frame):
    return None
_prev_alarm_handler = signal.getsignal(signal.SIGALRM)
signal.signal(signal.SIGALRM, _test_alarm_handler)
signal.setitimer(signal.ITIMER_REAL, 0.4)"#,
            )
            .expect("seed alarm state");

        CapabilityProvider::inspect(&session, "__import__('time').sleep(0.25)")
            .expect("inspect with delay");
        let check = session
            .eval_expr(
                "(__import__('signal').getsignal(__import__('signal').SIGALRM) is _test_alarm_handler) and (0.01 < __import__('signal').getitimer(__import__('signal').ITIMER_REAL)[0] < 0.3)",
            )
            .expect("check restored alarm state");
        assert_eq!(check.value_repr, "True");

        session
            .exec_code(
                r#"import signal
signal.setitimer(signal.ITIMER_REAL, 0)
signal.signal(signal.SIGALRM, _prev_alarm_handler)"#,
            )
            .expect("restore alarm state");
    }

    #[test]
    fn capability_inspect_times_out_slow_expressions() {
        let _signal_guard = SIGNAL_TEST_MUTEX.lock().expect("lock signal test mutex");
        let session = PythonSession::initialize().expect("python session");
        let timeout_supported = session
            .eval_expr("hasattr(__import__('signal'), 'SIGALRM') and hasattr(__import__('signal'), 'ITIMER_REAL')")
            .expect("check signal")
            .value_repr;
        let runs_on_main_thread = session
            .eval_expr(
                "__import__('threading').current_thread() is __import__('threading').main_thread()",
            )
            .expect("check thread")
            .value_repr;
        if timeout_supported != "True" || runs_on_main_thread != "True" {
            return;
        }

        let started = Instant::now();
        let err = CapabilityProvider::inspect(&session, "__import__('time').sleep(2)")
            .expect_err("inspect should timeout");
        assert!(started.elapsed() < Duration::from_millis(1900));
        match err {
            CapabilityError::PythonException(exc) => {
                assert_eq!(exc.exc_type, "TimeoutError");
                assert!(exc.message.contains("inspect timed out"));
            }
            other => panic!("expected PythonException, got {other:?}"),
        }
    }

    #[test]
    fn capability_inspect_errors_surface_python_exception_payload() {
        let session = PythonSession::initialize().expect("python session");
        let err = CapabilityProvider::inspect(&session, "missing_name")
            .expect_err("name error should map to capability error");
        match err {
            CapabilityError::PythonException(exc) => {
                assert_eq!(exc.exc_type, "NameError");
                assert!(exc.message.contains("missing_name"));
            }
            other => panic!("expected PythonException, got {other:?}"),
        }
    }

    #[test]
    fn capability_list_globals_matches_filtered_runtime_names() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code("alpha = 1\n_beta = 2")
            .expect("seed globals");

        let globals = CapabilityProvider::list_globals(&session).expect("capability globals");
        assert!(globals.iter().any(|entry| entry.name == "alpha"));
        assert!(globals.iter().any(|entry| entry.name == "_beta"));
        assert!(
            !globals
                .iter()
                .any(|entry| entry.name.starts_with("_pyaichat_"))
        );
    }
}
