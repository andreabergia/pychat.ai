use anyhow::{Result, anyhow};
use pyo3::prelude::*;
use pyo3::types::PyModuleMethods;
use pyo3::types::{PyAnyMethods, PyDict, PyDictMethods, PyList, PyModule, PyTuple, PyTupleMethods};
use std::ffi::CString;

use super::capabilities::{
    CapabilityError, CapabilityProvider, CapabilityResult, DIR_MAX_MEMBERS, DOC_MAX_LEN, DirInfo,
    DocInfo, EvalInfo, GlobalEntry, REPR_MAX_LEN, ReprInfo, TypeInfo, truncate_members,
    truncate_text,
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

    fn cap_dict_optional_string(
        result: &Bound<'_, pyo3::types::PyAny>,
        key: &str,
    ) -> CapabilityResult<Option<String>> {
        let dict = Self::cap_cast_dict(result)?;
        let value = dict
            .get_item(key)
            .map_err(Self::cap_internal)?
            .ok_or_else(|| Self::cap_invalid_shape(format!("missing {key} in helper result")))?;
        if value.is_none() {
            return Ok(None);
        }

        value
            .extract()
            .map(Some)
            .map_err(|_| Self::cap_invalid_shape(format!("{key} must be string or None")))
    }

    fn cap_dict_string_list(
        result: &Bound<'_, pyo3::types::PyAny>,
        key: &str,
    ) -> CapabilityResult<Vec<String>> {
        let dict = Self::cap_cast_dict(result)?;
        let value = dict
            .get_item(key)
            .map_err(Self::cap_internal)?
            .ok_or_else(|| Self::cap_invalid_shape(format!("missing {key} in helper result")))?;
        value
            .extract()
            .map_err(|_| Self::cap_invalid_shape(format!("{key} must be list[str]")))
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

    fn get_type(&self, expr: &str) -> CapabilityResult<TypeInfo> {
        Python::attach(|py| -> CapabilityResult<TypeInfo> {
            let main = self.main_module.bind(py);
            let result = Self::call_runtime_helper(main, "_pyaichat_get_type", (expr,))
                .map_err(Self::cap_internal)?;
            if !Self::cap_result_ok(&result)? {
                return Err(CapabilityError::PythonException(Self::cap_dict_exception(
                    &result,
                )?));
            }

            Ok(TypeInfo {
                name: Self::cap_dict_string(&result, "name")?,
                module: Self::cap_dict_string(&result, "module")?,
                qualified: Self::cap_dict_string(&result, "qualified")?,
            })
        })
    }

    fn get_repr(&self, expr: &str) -> CapabilityResult<ReprInfo> {
        Python::attach(|py| -> CapabilityResult<ReprInfo> {
            let main = self.main_module.bind(py);
            let result = Self::call_runtime_helper(main, "_pyaichat_get_repr", (expr,))
                .map_err(Self::cap_internal)?;
            if !Self::cap_result_ok(&result)? {
                return Err(CapabilityError::PythonException(Self::cap_dict_exception(
                    &result,
                )?));
            }

            let repr = Self::cap_dict_string(&result, "repr")?;
            let (repr, truncated, original_len) = truncate_text(repr, REPR_MAX_LEN);
            Ok(ReprInfo {
                repr,
                truncated,
                original_len,
            })
        })
    }

    fn get_dir(&self, expr: &str) -> CapabilityResult<DirInfo> {
        Python::attach(|py| -> CapabilityResult<DirInfo> {
            let main = self.main_module.bind(py);
            let result = Self::call_runtime_helper(main, "_pyaichat_get_dir", (expr,))
                .map_err(Self::cap_internal)?;
            if !Self::cap_result_ok(&result)? {
                return Err(CapabilityError::PythonException(Self::cap_dict_exception(
                    &result,
                )?));
            }

            let members = Self::cap_dict_string_list(&result, "members")?;
            let (members, truncated, original_len) = truncate_members(members, DIR_MAX_MEMBERS);
            Ok(DirInfo {
                members,
                truncated,
                original_len,
            })
        })
    }

    fn get_doc(&self, expr: &str) -> CapabilityResult<DocInfo> {
        Python::attach(|py| -> CapabilityResult<DocInfo> {
            let main = self.main_module.bind(py);
            let result = Self::call_runtime_helper(main, "_pyaichat_get_doc", (expr,))
                .map_err(Self::cap_internal)?;
            if !Self::cap_result_ok(&result)? {
                return Err(CapabilityError::PythonException(Self::cap_dict_exception(
                    &result,
                )?));
            }

            let doc = Self::cap_dict_optional_string(&result, "doc")?;
            let (doc, truncated, original_len) = match doc {
                Some(doc) => {
                    let (doc, truncated, original_len) = truncate_text(doc, DOC_MAX_LEN);
                    (Some(doc), truncated, original_len)
                }
                None => (None, false, 0),
            };

            Ok(DocInfo {
                doc,
                truncated,
                original_len,
            })
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

    fn get_last_exception(&self) -> CapabilityResult<Option<ExceptionInfo>> {
        PythonSession::get_last_exception(self).map_err(Self::cap_internal)
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use crate::python::{CapabilityError, CapabilityProvider};
    use pyo3::prelude::Python;
    use pyo3::types::PyModuleMethods;

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

    #[test]
    fn capability_get_type_returns_name_module_and_qualified() {
        let session = PythonSession::initialize().expect("python session");
        let type_info = CapabilityProvider::get_type(&session, "[1, 2, 3]").expect("get type");
        assert_eq!(type_info.name, "list");
        assert_eq!(type_info.module, "builtins");
        assert_eq!(type_info.qualified, "builtins.list");
    }

    #[test]
    fn capability_get_type_supports_user_defined_types() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code("class CustomThing:\n    pass\nobj = CustomThing()")
            .expect("seed custom type");

        let type_info = CapabilityProvider::get_type(&session, "obj").expect("get type");
        assert_eq!(type_info.name, "CustomThing");
        assert_eq!(type_info.module, "__main__");
        assert_eq!(type_info.qualified, "__main__.CustomThing");
    }

    #[test]
    fn capability_get_repr_truncates_large_output() {
        let session = PythonSession::initialize().expect("python session");
        let repr_info = CapabilityProvider::get_repr(&session, "'x' * 5000").expect("get repr");
        assert!(repr_info.truncated);
        assert_eq!(repr_info.original_len, 5002);
        assert_eq!(repr_info.repr.chars().count(), 4096);
    }

    #[test]
    fn capability_get_dir_is_sorted_and_truncated() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code(
                "class CustomDir:\n    def __dir__(self):\n        return [f'n{i:03}' for i in range(300, -1, -1)]\nobj = CustomDir()",
            )
            .expect("seed dir object");

        let dir_info = CapabilityProvider::get_dir(&session, "obj").expect("get dir");
        assert!(dir_info.truncated);
        assert_eq!(dir_info.original_len, 301);
        assert_eq!(dir_info.members.len(), 256);
        assert!(dir_info.members.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(dir_info.members.first().expect("first member"), "n000");
    }

    #[test]
    fn capability_get_doc_returns_none_when_missing() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code("class NoDoc:\n    __doc__ = None\nobj = NoDoc()")
            .expect("seed no doc object");

        let doc_info = CapabilityProvider::get_doc(&session, "obj").expect("get doc");
        assert!(!doc_info.truncated);
        assert_eq!(doc_info.original_len, 0);
        assert!(doc_info.doc.is_none());
    }

    #[test]
    fn capability_get_doc_truncates_long_doc() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code("class LongDoc:\n    __doc__ = 'd' * 5000\nobj = LongDoc()")
            .expect("seed long doc object");

        let doc_info = CapabilityProvider::get_doc(&session, "obj").expect("get doc");
        assert!(doc_info.truncated);
        assert_eq!(doc_info.original_len, 5000);
        assert_eq!(doc_info.doc.expect("doc present").chars().count(), 4096);
    }

    #[test]
    fn capability_get_doc_returns_some_without_truncation() {
        let session = PythonSession::initialize().expect("python session");
        session
            .exec_code("class HasDoc:\n    \"\"\"small docs\"\"\"")
            .expect("seed doc object");

        let doc_info = CapabilityProvider::get_doc(&session, "HasDoc").expect("get doc");
        assert!(!doc_info.truncated);
        assert_eq!(doc_info.original_len, 10);
        assert_eq!(doc_info.doc.as_deref(), Some("small docs"));
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
    fn capability_errors_surface_python_exception_payload() {
        let session = PythonSession::initialize().expect("python session");
        let err = CapabilityProvider::get_repr(&session, "missing_name")
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
    fn capability_get_last_exception_returns_same_payload_as_runtime_api() {
        let session = PythonSession::initialize().expect("python session");
        session.run_user_input("1 / 0").expect("run failure");

        let direct = session
            .get_last_exception()
            .expect("runtime exception")
            .expect("runtime exception exists");
        let via_capability = CapabilityProvider::get_last_exception(&session)
            .expect("capability exception")
            .expect("capability exception exists");

        assert_eq!(via_capability.exc_type, direct.exc_type);
        assert_eq!(via_capability.message, direct.message);
        assert_eq!(via_capability.traceback, direct.traceback);
    }

    #[test]
    fn capability_invalid_result_shape_surfaces_for_missing_ok() {
        let session = PythonSession::initialize().expect("python session");

        Python::attach(|py| {
            let code =
                CString::new("def _pyaichat_get_repr(expr):\n    return {'repr': 'no ok key'}\n")
                    .expect("python code cstring");
            let main = session.main_module.bind(py);
            let globals = main.dict();
            py.run(code.as_c_str(), Some(&globals), Some(&globals))
                .expect("override helper in session globals");
        });

        let err = CapabilityProvider::get_repr(&session, "1 + 1")
            .expect_err("malformed helper payload should fail");

        match err {
            CapabilityError::InvalidResultShape(msg) => {
                assert!(msg.contains("missing ok"));
            }
            other => panic!("expected InvalidResultShape, got {other:?}"),
        }
    }
}
