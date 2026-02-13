use anyhow::Result;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use std::ffi::CString;

pub struct PythonSession {
    main_module: Py<PyModule>,
}

impl PythonSession {
    pub fn initialize() -> Result<Self> {
        Python::attach(|py| -> Result<Self> {
            let main_module = PyModule::import(py, "__main__")?;
            let _ = py.eval(c"1 + 1", None, None)?;
            Ok(Self {
                main_module: main_module.unbind(),
            })
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
}
