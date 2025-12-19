use open_xiaoai::services::connect::message::MessageManager;
use open_xiaoai::services::connect::rpc::RPC;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use serde_json::json;
use server::AppServer;

pub mod macros;
pub mod python;
pub mod server;

#[pyfunction]
fn on_output_data(py: Python, data: Py<PyBytes>) -> PyResult<Bound<PyAny>> {
    let bytes = data.as_bytes(py).to_vec();
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let _ = MessageManager::instance()
            .send_stream("play", bytes, None)
            .await;
        Ok(())
    })
}

#[pyfunction]
fn start_server<'a>(py: Python<'a>, token: Option<&str>) -> PyResult<Bound<'a, PyAny>> {
    // 如果提供了token，设置环境变量
    if let Some(t) = token {
        std::env::set_var("OPEN_XIAOAI_TOKEN", t);
    }
    
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        AppServer::run().await;
        Ok(())
    })
}

#[pyfunction]
fn run_shell(py: Python, script: String, timeout_millis: f64) -> PyResult<Bound<PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let res = RPC::instance()
            .call_remote(
                "run_shell",
                Some(json!(script)),
                Some(timeout_millis as u64),
            )
            .await;
        let result = match res {
            Err(e) => format!("run_shell error: {}", e),
            Ok(res) => serde_json::to_string(&res.data.unwrap()).unwrap(),
        };
        Ok(result)
    })
}

#[pymodule]
fn open_xiaoai_server(_py: Python, m: Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(start_server, &m)?)?;
    m.add_function(wrap_pyfunction!(on_output_data, &m)?)?;
    m.add_function(wrap_pyfunction!(run_shell, &m)?)?;
    crate::python::init_module(&m)?;
    Ok(())
}
