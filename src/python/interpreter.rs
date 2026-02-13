use anyhow::Result;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use pyo3::types::PyModuleMethods;
use std::ffi::CString;

pub struct PythonSession {
    main_module: Py<PyModule>,
}

impl PythonSession {
    pub fn initialize() -> Result<Self> {
        Python::attach(|py| -> Result<Self> {
            let main_module = PyModule::import(py, "__main__")?;
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

    pub fn run_line(&self, line: &str) -> Result<()> {
        Python::attach(|py| -> Result<()> {
            let main = self.main_module.bind(py);
            let globals = main.dict();
            let code = CString::new(line)?;
            py.run(code.as_c_str(), Some(&globals), Some(&globals))?;
            Ok(())
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
}
