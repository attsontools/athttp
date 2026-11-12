use std::{path::PathBuf, time::Duration};

use athttp_service::{ServiceSupervisor, ServiceSupervisorError, StandaloneService};
use serde_json::Value;
use tauri::{Emitter, Manager, State};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

#[tauri::command]
async fn connect_http_service(
    app: tauri::AppHandle,
    service: State<'_, StandaloneService>,
) -> Result<(), ServiceSupervisorError> {
    let mut events = service.connect().await?;
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let _ = app.emit("http-service-event", event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}

#[tauri::command]
async fn http_service_request(
    service: State<'_, StandaloneService>,
    method: String,
    params: Value,
) -> Result<Value, ServiceSupervisorError> {
    service.request(method, params).await
}

#[tauri::command]
async fn write_text_file(path: String, content: String) -> Result<(), String> {
    tokio::fs::write(path, content)
        .await
        .map_err(|error| format!("保存响应失败：{error}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_directory = shared_data_directory(app)?;
            let endpoint_path = data_directory.join("service.endpoint.json");
            let executable = service_executable(app)?;
            let supervisor = ServiceSupervisor::new(
                endpoint_path,
                data_directory,
                executable,
                "athttp-standalone",
                env!("CARGO_PKG_VERSION"),
                STARTUP_TIMEOUT,
            );
            app.manage(StandaloneService::new(supervisor));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connect_http_service,
            http_service_request,
            write_text_file
        ])
        .run(tauri::generate_context!())
        .expect("failed to run HTTP 请求");
}

fn shared_data_directory(app: &tauri::App) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = std::env::var_os("ATHTTP_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    let standalone_directory = app.path().app_data_dir()?;
    let application_data_root = standalone_directory.parent().ok_or_else(|| {
        std::io::Error::other("standalone app data directory has no parent")
    })?;
    Ok(application_data_root.join("com.attson.attool").join("http"))
}

fn service_executable(app: &tauri::App) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = std::env::var_os("ATHTTP_SERVICE_EXECUTABLE") {
        return Ok(PathBuf::from(path));
    }
    let executable_name = if cfg!(windows) {
        "athttp-service.exe"
    } else {
        "athttp-service"
    };
    Ok(app.path().resource_dir()?.join(executable_name))
}
