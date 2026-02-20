use anyhow::{Result, anyhow};
use pyo3::prelude::*;
use pyo3::types::PyModuleMethods;
use pyo3::types::{PyAnyMethods, PyDict, PyDictMethods, PyList, PyModule, PyTuple, PyTupleMethods};
use serde_json::Value;
use std::ffi::CString;

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
    main_module: Py<PyModule>,
}

#[allow(dead_code)]
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

    pub fn check_input_completeness(&self, source: &str) -> Result<InputCompleteness> {
        Python::attach(|py| -> Result<InputCompleteness> {
            let main = self.main_module.bind(py);
            let result =
                Self::call_runtime_helper(main, "_pyaichat_check_input_complete", (source,))?;
            if !Self::result_ok(&result)? {
                let exception = Self::dict_exception(&result)?;
                anyhow::bail!("{}", exception.traceback)
            }

            let status = Self::dict_string(&result, "status")?;
            match status.as_str() {
                "complete" => Ok(InputCompleteness::Complete),
                "incomplete" => Ok(InputCompleteness::Incomplete),
                "invalid" => Ok(InputCompleteness::Invalid),
                _ => anyhow::bail!("unknown completeness status: {status}"),
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
        let helper_code = CString::new(include_str!("runtime_helpers.py"))?;
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
            .ok_or_else(|| anyhow!("missing ok in helper result"))?
            .extract()?)
    }

    fn dict_string(result: &Bound<'_, pyo3::types::PyAny>, key: &str) -> Result<String> {
        let dict = Self::cast_dict(result)?;
        Ok(dict
            .get_item(key)?
            .ok_or_else(|| anyhow!("missing {key} in helper result"))?
            .extract()?)
    }

    fn dict_exception(result: &Bound<'_, pyo3::types::PyAny>) -> Result<ExceptionInfo> {
        let dict = Self::cast_dict(result)?;
        let exception = dict
            .get_item("exception")?
            .ok_or_else(|| anyhow!("missing exception in helper result"))?;
        Self::any_to_exception(exception)
    }

    fn any_to_exception(exception: Bound<'_, pyo3::types::PyAny>) -> Result<ExceptionInfo> {
        let dict = Self::cast_dict(&exception)?;
        Ok(ExceptionInfo {
            exc_type: dict
                .get_item("exc_type")?
                .ok_or_else(|| anyhow!("missing exc_type"))?
                .extract()?,
            message: dict
                .get_item("message")?
                .ok_or_else(|| anyhow!("missing message"))?
                .extract()?,
            traceback: dict
                .get_item("traceback")?
                .ok_or_else(|| anyhow!("missing traceback"))?
                .extract()?,
        })
    }

    fn cast_dict<'a>(value: &'a Bound<'a, pyo3::types::PyAny>) -> Result<&'a Bound<'a, PyDict>> {
        value
            .cast::<PyDict>()
            .map_err(|err| anyhow!(err.to_string()))
    }

    #[allow(dead_code)]
    fn cast_list<'a>(value: &'a Bound<'a, pyo3::types::PyAny>) -> Result<&'a Bound<'a, PyList>> {
        value
            .cast::<PyList>()
            .map_err(|err| anyhow!(err.to_string()))
    }

    #[allow(dead_code)]
    fn cast_tuple<'a>(value: &'a Bound<'a, pyo3::types::PyAny>) -> Result<&'a Bound<'a, PyTuple>> {
        value
            .cast::<PyTuple>()
            .map_err(|err| anyhow!(err.to_string()))
    }
}

impl PythonSession {
    fn cap_internal(err: impl std::fmt::Display) -> CapabilityError {
        CapabilityError::Internal(err.to_string())
    }

    fn cap_invalid_shape(msg: impl Into<String>) -> CapabilityError {
        CapabilityError::InvalidResultShape(msg.into())
    }

    fn cap_result_ok(result: &Bound<'_, pyo3::types::PyAny>) -> CapabilityResult<bool> {
        let dict = Self::cap_cast_dict(result)?;
        let value = dict
            .get_item("ok")
            .map_err(Self::cap_internal)?
            .ok_or_else(|| Self::cap_invalid_shape("missing ok in helper result"))?;
        value.extract().map_err(Self::cap_internal)
    }

    fn cap_dict_string(
        result: &Bound<'_, pyo3::types::PyAny>,
        key: &str,
    ) -> CapabilityResult<String> {
        let dict = Self::cap_cast_dict(result)?;
        let value = dict
            .get_item(key)
            .map_err(Self::cap_internal)?
            .ok_or_else(|| Self::cap_invalid_shape(format!("missing {key} in helper result")))?;
        value.extract().map_err(Self::cap_internal)
    }

    fn cap_dict_json_value(
        result: &Bound<'_, pyo3::types::PyAny>,
        key: &str,
    ) -> CapabilityResult<Value> {
        let encoded = Self::cap_dict_string(result, key)?;
        serde_json::from_str(&encoded).map_err(|err| {
            Self::cap_invalid_shape(format!("invalid JSON in {key} helper result: {err}"))
        })
    }

    fn cap_dict_exception(
        result: &Bound<'_, pyo3::types::PyAny>,
    ) -> CapabilityResult<ExceptionInfo> {
        let dict = Self::cap_cast_dict(result)?;
        let exception = dict
            .get_item("exception")
            .map_err(Self::cap_internal)?
            .ok_or_else(|| Self::cap_invalid_shape("missing exception in helper result"))?;
        Self::any_to_exception(exception)
            .map_err(|err| Self::cap_invalid_shape(format!("invalid exception payload: {err}")))
    }

    fn cap_cast_dict<'a>(
        value: &'a Bound<'a, pyo3::types::PyAny>,
    ) -> CapabilityResult<&'a Bound<'a, PyDict>> {
        value
            .cast::<PyDict>()
            .map_err(|err| Self::cap_invalid_shape(err.to_string()))
    }
}

#[allow(dead_code)]
impl CapabilityProvider for PythonSession {
    fn list_globals(&self) -> CapabilityResult<Vec<GlobalEntry>> {
        PythonSession::list_globals(self).map_err(Self::cap_internal)
    }

    fn inspect(&self, expr: &str) -> CapabilityResult<InspectInfo> {
        Python::attach(|py| -> CapabilityResult<InspectInfo> {
            let main = self.main_module.bind(py);
            let result = Self::call_runtime_helper(main, "_pyaichat_inspect", (expr,))
                .map_err(Self::cap_internal)?;
            if !Self::cap_result_ok(&result)? {
                return Err(CapabilityError::PythonException(Self::cap_dict_exception(
                    &result,
                )?));
            }

            let value = Self::cap_dict_json_value(&result, "inspect_json")?;
            let type_is_object = value.get("type").is_some_and(Value::is_object);
            let kind_is_string = value.get("kind").is_some_and(Value::is_string);
            if !type_is_object || !kind_is_string {
                return Err(Self::cap_invalid_shape(
                    "inspect result must contain object type and string kind",
                ));
            }

            Ok(InspectInfo { value })
        })
    }

    fn eval_expr(&self, expr: &str) -> CapabilityResult<EvalInfo> {
        Python::attach(|py| -> CapabilityResult<EvalInfo> {
            let main = self.main_module.bind(py);
            let result = Self::call_runtime_helper(main, "_pyaichat_eval_expr", (expr,))
                .map_err(Self::cap_internal)?;
            if !Self::cap_result_ok(&result)? {
                return Err(CapabilityError::PythonException(Self::cap_dict_exception(
                    &result,
                )?));
            }

            Ok(EvalInfo {
                value_repr: Self::cap_dict_string(&result, "value_repr")?,
                stdout: Self::cap_dict_string(&result, "stdout")?,
                stderr: Self::cap_dict_string(&result, "stderr")?,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use crate::python::{CapabilityError, CapabilityProvider};
    use pyo3::prelude::Python;
    use pyo3::types::PyModuleMethods;

    use super::{InputCompleteness, PythonSession, UserRunResult};

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
    fn capability_inspect_timeout_surfaces_as_python_exception() {
        let session = PythonSession::initialize().expect("python session");
        let timeout_supported = session
            .eval_expr("hasattr(__import__('signal'), 'SIGALRM')")
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
                "import __main__, time\n__main__._INSPECT_TIMEOUT_SECONDS = 0.05\nclass SlowRepr:\n    def __repr__(self):\n        time.sleep(0.2)\n        return 'slow'\nslow = SlowRepr()",
            )
            .expect("seed slow repr");
        let err =
            CapabilityProvider::inspect(&session, "slow").expect_err("inspect timeout should fail");
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

    #[test]
    fn capability_invalid_result_shape_surfaces_for_missing_ok() {
        let session = PythonSession::initialize().expect("python session");

        Python::attach(|py| {
            let code =
                CString::new("def _pyaichat_inspect(expr):\n    return {'inspect_json': '{}'}\n")
                    .expect("python code cstring");
            let main = session.main_module.bind(py);
            let globals = main.dict();
            py.run(code.as_c_str(), Some(&globals), Some(&globals))
                .expect("override helper in session globals");
        });

        let err = CapabilityProvider::inspect(&session, "1 + 1")
            .expect_err("malformed helper payload should fail");

        match err {
            CapabilityError::InvalidResultShape(msg) => {
                assert!(msg.contains("missing ok"));
            }
            other => panic!("expected InvalidResultShape, got {other:?}"),
        }
    }

    #[test]
    fn capability_inspect_invalid_result_shape_for_missing_kind() {
        let session = PythonSession::initialize().expect("python session");

        Python::attach(|py| {
            let code = CString::new(
                "def _pyaichat_inspect(expr):\n    return {'ok': True, 'inspect_json': '{\"type\": {\"name\": \"int\"}}'}\n",
            )
            .expect("python code cstring");
            let main = session.main_module.bind(py);
            let globals = main.dict();
            py.run(code.as_c_str(), Some(&globals), Some(&globals))
                .expect("override helper in session globals");
        });

        let err = CapabilityProvider::inspect(&session, "1 + 1")
            .expect_err("malformed inspect payload should fail");
        match err {
            CapabilityError::InvalidResultShape(msg) => {
                assert!(msg.contains("inspect result must contain"));
            }
            other => panic!("expected InvalidResultShape, got {other:?}"),
        }
    }
}
