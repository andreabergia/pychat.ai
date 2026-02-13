use anyhow::Result;
use pyo3::prelude::*;
use pyo3::types::PyModuleMethods;
use pyo3::types::{PyAnyMethods, PyDict, PyDictMethods, PyList, PyModule, PyTuple, PyTupleMethods};
use std::ffi::CString;

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
#[allow(dead_code)]
pub struct GlobalEntry {
    pub name: String,
    pub type_name: String,
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

pub struct PythonSession {
    main_module: Py<PyModule>,
}

impl PythonSession {
    pub fn initialize() -> Result<Self> {
        Python::attach(|py| -> Result<Self> {
            let main_module = PyModule::import(py, "__main__")?;
            Self::install_runtime_helpers(py, &main_module)?;
            Self::health_check(py, &main_module)?;

            let session = Self {
                main_module: main_module.unbind(),
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
            let main = self.main_module.bind(py);
            let result = Self::call_runtime_helper(main, "_pyaichat_exec_code", (code,))?;
            if Self::result_ok(&result)? {
                Ok(ExecResult {
                    stdout: Self::dict_string(&result, "stdout")?,
                    stderr: Self::dict_string(&result, "stderr")?,
                })
            } else {
                let exception = Self::dict_exception(&result)?;
                anyhow::bail!("{}", exception.traceback)
            }
        })
    }

    #[allow(dead_code)]
    pub fn eval_expr(&self, expr: &str) -> Result<EvalResult> {
        Python::attach(|py| -> Result<EvalResult> {
            let main = self.main_module.bind(py);
            let result = Self::call_runtime_helper(main, "_pyaichat_eval_expr", (expr,))?;
            if Self::result_ok(&result)? {
                Ok(EvalResult {
                    value_repr: Self::dict_string(&result, "value_repr")?,
                    stdout: Self::dict_string(&result, "stdout")?,
                    stderr: Self::dict_string(&result, "stderr")?,
                })
            } else {
                let exception = Self::dict_exception(&result)?;
                anyhow::bail!("{}", exception.traceback)
            }
        })
    }

    pub fn run_user_input(&self, line: &str) -> Result<UserRunResult> {
        Python::attach(|py| -> Result<UserRunResult> {
            let main = self.main_module.bind(py);
            let result = Self::call_runtime_helper(main, "_pyaichat_run_user_input", (line,))?;
            let kind = Self::dict_string(&result, "kind")?;
            let ok = Self::result_ok(&result)?;

            match (kind.as_str(), ok) {
                ("evaluated", true) => Ok(UserRunResult::Evaluated(EvalResult {
                    value_repr: Self::dict_string(&result, "value_repr")?,
                    stdout: Self::dict_string(&result, "stdout")?,
                    stderr: Self::dict_string(&result, "stderr")?,
                })),
                ("executed", true) => Ok(UserRunResult::Executed(ExecResult {
                    stdout: Self::dict_string(&result, "stdout")?,
                    stderr: Self::dict_string(&result, "stderr")?,
                })),
                (_, false) => Ok(UserRunResult::Failed {
                    stdout: Self::dict_string(&result, "stdout")?,
                    stderr: Self::dict_string(&result, "stderr")?,
                    exception: Self::dict_exception(&result)?,
                }),
                _ => anyhow::bail!("unknown user run result kind: {kind}"),
            }
        })
    }

    #[allow(dead_code)]
    pub fn list_globals(&self) -> Result<Vec<GlobalEntry>> {
        Python::attach(|py| -> Result<Vec<GlobalEntry>> {
            let main = self.main_module.bind(py);
            let py_entries = Self::call_runtime_helper(main, "_pyaichat_list_globals", ())?;
            let py_entries = Self::cast_list(&py_entries)?;
            let mut entries = Vec::new();
            for item in py_entries.iter() {
                let tuple = Self::cast_tuple(&item)?;
                entries.push(GlobalEntry {
                    name: tuple.get_item(0)?.extract()?,
                    type_name: tuple.get_item(1)?.extract()?,
                });
            }
            Ok(entries)
        })
    }

    #[allow(dead_code)]
    pub fn get_last_exception(&self) -> Result<Option<ExceptionInfo>> {
        Python::attach(|py| -> Result<Option<ExceptionInfo>> {
            let main = self.main_module.bind(py);
            let result = Self::call_runtime_helper(main, "_pyaichat_get_last_exception", ())?;
            if result.is_none() {
                Ok(None)
            } else {
                Ok(Some(Self::any_to_exception(result)?))
            }
        })
    }

    pub fn is_healthy(&self) -> bool {
        Python::attach(|py| {
            let main = self.main_module.bind(py);
            Self::health_check(py, main).is_ok()
        })
    }

    fn health_check(py: Python<'_>, main_module: &Bound<'_, PyModule>) -> PyResult<()> {
        let globals = main_module.dict();
        let _ = py.eval(c"1 + 1", Some(&globals), Some(&globals))?;
        Ok(())
    }

    fn install_runtime_helpers(py: Python<'_>, main_module: &Bound<'_, PyModule>) -> Result<()> {
        let globals = main_module.dict();
        let helper_code = CString::new(
            r#"
import contextlib
import io
import traceback

_PYAICHAT_LAST_EXCEPTION = None

def _pyaichat_capture_exception(exc):
    global _PYAICHAT_LAST_EXCEPTION
    _PYAICHAT_LAST_EXCEPTION = {
        "exc_type": type(exc).__name__,
        "message": str(exc),
        "traceback": traceback.format_exc(),
    }
    return _PYAICHAT_LAST_EXCEPTION

def _pyaichat_exec_code(code):
    out = io.StringIO()
    err = io.StringIO()
    try:
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            exec(code, globals(), globals())
        return {"ok": True, "stdout": out.getvalue(), "stderr": err.getvalue()}
    except BaseException as exc:
        return {
            "ok": False,
            "stdout": out.getvalue(),
            "stderr": err.getvalue(),
            "exception": _pyaichat_capture_exception(exc),
        }

def _pyaichat_eval_expr(expr):
    out = io.StringIO()
    err = io.StringIO()
    try:
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            value = eval(expr, globals(), globals())
        return {
            "ok": True,
            "value_repr": repr(value),
            "stdout": out.getvalue(),
            "stderr": err.getvalue(),
        }
    except BaseException as exc:
        return {
            "ok": False,
            "stdout": out.getvalue(),
            "stderr": err.getvalue(),
            "exception": _pyaichat_capture_exception(exc),
        }

def _pyaichat_run_user_input(line):
    try:
        compile(line, "<stdin>", "eval")
    except SyntaxError:
        result = _pyaichat_exec_code(line)
        result["kind"] = "executed"
        return result
    result = _pyaichat_eval_expr(line)
    result["kind"] = "evaluated"
    return result

def _pyaichat_list_globals():
    entries = []
    for name, value in globals().items():
        if name == "__builtins__":
            continue
        if name.startswith("__") and name.endswith("__"):
            continue
        entries.append((name, type(value).__name__))
    entries.sort(key=lambda item: item[0])
    return entries

def _pyaichat_get_last_exception():
    return _PYAICHAT_LAST_EXCEPTION
"#,
        )?;
        py.run(helper_code.as_c_str(), Some(&globals), Some(&globals))?;
        Ok(())
    }

    fn call_runtime_helper<'py, A>(
        main_module: &Bound<'py, PyModule>,
        helper_name: &str,
        args: A,
    ) -> Result<Bound<'py, pyo3::types::PyAny>>
    where
        A: pyo3::call::PyCallArgs<'py>,
    {
        let helper = main_module.getattr(helper_name)?;
        let result = helper.call1(args)?;
        Ok(result)
    }

    fn result_ok(result: &Bound<'_, pyo3::types::PyAny>) -> Result<bool> {
        let dict = Self::cast_dict(result)?;
        Ok(dict
            .get_item("ok")?
            .ok_or_else(|| anyhow::anyhow!("missing ok in helper result"))?
            .extract()?)
    }

    fn dict_string(result: &Bound<'_, pyo3::types::PyAny>, key: &str) -> Result<String> {
        let dict = Self::cast_dict(result)?;
        Ok(dict
            .get_item(key)?
            .ok_or_else(|| anyhow::anyhow!("missing {key} in helper result"))?
            .extract()?)
    }

    fn dict_exception(result: &Bound<'_, pyo3::types::PyAny>) -> Result<ExceptionInfo> {
        let dict = Self::cast_dict(result)?;
        let exception = dict
            .get_item("exception")?
            .ok_or_else(|| anyhow::anyhow!("missing exception in helper result"))?;
        Self::any_to_exception(exception)
    }

    fn any_to_exception(exception: Bound<'_, pyo3::types::PyAny>) -> Result<ExceptionInfo> {
        let dict = Self::cast_dict(&exception)?;
        Ok(ExceptionInfo {
            exc_type: dict
                .get_item("exc_type")?
                .ok_or_else(|| anyhow::anyhow!("missing exc_type"))?
                .extract()?,
            message: dict
                .get_item("message")?
                .ok_or_else(|| anyhow::anyhow!("missing message"))?
                .extract()?,
            traceback: dict
                .get_item("traceback")?
                .ok_or_else(|| anyhow::anyhow!("missing traceback"))?
                .extract()?,
        })
    }

    fn cast_dict<'a>(value: &'a Bound<'a, pyo3::types::PyAny>) -> Result<&'a Bound<'a, PyDict>> {
        value
            .cast::<PyDict>()
            .map_err(|err| anyhow::anyhow!(err.to_string()))
    }

    #[allow(dead_code)]
    fn cast_list<'a>(value: &'a Bound<'a, pyo3::types::PyAny>) -> Result<&'a Bound<'a, PyList>> {
        value
            .cast::<PyList>()
            .map_err(|err| anyhow::anyhow!(err.to_string()))
    }

    #[allow(dead_code)]
    fn cast_tuple<'a>(value: &'a Bound<'a, pyo3::types::PyAny>) -> Result<&'a Bound<'a, PyTuple>> {
        value
            .cast::<PyTuple>()
            .map_err(|err| anyhow::anyhow!(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::{PythonSession, UserRunResult};

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
            .expect("get persisted exception")
            .expect("persisted exception exists");
        assert_eq!(persisted.exc_type, "ZeroDivisionError");

        session
            .run_user_input("unknown_name")
            .expect("second failure call returns structured failed result");
        let replaced = session
            .get_last_exception()
            .expect("get replaced exception")
            .expect("replaced exception exists");
        assert_eq!(replaced.exc_type, "NameError");
    }
}
