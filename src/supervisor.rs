use std::{
    io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};

use serde::Serialize;
use tokio::time::{sleep, Instant};

use crate::{read_endpoint, ServiceClient, ServiceClientError};

const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSupervisorError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl ServiceSupervisorError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
        }
    }
}

impl std::fmt::Display for ServiceSupervisorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ServiceSupervisorError {}

impl From<ServiceClientError> for ServiceSupervisorError {
    fn from(error: ServiceClientError) -> Self {
        Self {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServiceLaunchSpec {
    pub endpoint_path: PathBuf,
    pub data_directory: PathBuf,
    pub service_version: String,
}

pub trait ServiceProcess: Send {
    fn try_exit(&mut self) -> io::Result<Option<i32>>;
}

pub trait ServiceLauncher: Send + Sync {
    fn launch(
        &self,
        spec: &ServiceLaunchSpec,
    ) -> Result<Box<dyn ServiceProcess>, ServiceSupervisorError>;
}

pub struct CommandServiceLauncher {
    executable: PathBuf,
}

impl CommandServiceLauncher {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }
}

impl ServiceLauncher for CommandServiceLauncher {
    fn launch(
        &self,
        spec: &ServiceLaunchSpec,
    ) -> Result<Box<dyn ServiceProcess>, ServiceSupervisorError> {
        let child = Command::new(&self.executable)
            .arg("--endpoint")
            .arg(&spec.endpoint_path)
            .arg("--data-directory")
            .arg(&spec.data_directory)
            .stdin(Stdio::null())
            .spawn()
            .map_err(|error| {
                ServiceSupervisorError::new(
                    "HTTP_SERVICE_START_FAILED",
                    format!("cannot start HTTP service: {error}"),
                )
            })?;
        Ok(Box::new(ChildProcess(child)))
    }
}

struct ChildProcess(Child);

impl ServiceProcess for ChildProcess {
    fn try_exit(&mut self) -> io::Result<Option<i32>> {
        self.0
            .try_wait()
            .map(|status| status.map(|status| status.code().unwrap_or(-1)))
    }
}

pub struct ServiceSupervisor {
    endpoint_path: PathBuf,
    data_directory: PathBuf,
    client_id: String,
    service_version: String,
    startup_timeout: Duration,
    launcher: Arc<dyn ServiceLauncher>,
    process: Option<Box<dyn ServiceProcess>>,
}

impl ServiceSupervisor {
    pub fn new(
        endpoint_path: PathBuf,
        data_directory: PathBuf,
        executable: PathBuf,
        client_id: impl Into<String>,
        service_version: impl Into<String>,
        startup_timeout: Duration,
    ) -> Self {
        Self::with_launcher(
            endpoint_path,
            data_directory,
            client_id,
            service_version,
            startup_timeout,
            Arc::new(CommandServiceLauncher::new(executable)),
        )
    }

    pub fn with_launcher(
        endpoint_path: PathBuf,
        data_directory: PathBuf,
        client_id: impl Into<String>,
        service_version: impl Into<String>,
        startup_timeout: Duration,
        launcher: Arc<dyn ServiceLauncher>,
    ) -> Self {
        Self {
            endpoint_path,
            data_directory,
            client_id: client_id.into(),
            service_version: service_version.into(),
            startup_timeout,
            launcher,
            process: None,
        }
    }

    pub async fn connect(&mut self) -> Result<ServiceClient, ServiceSupervisorError> {
        if self.endpoint_path.exists() {
            match self.connect_client().await {
                Ok(client) => {
                    self.ensure_endpoint_version()?;
                    return Ok(client);
                }
                Err(error) if is_stale_endpoint_error(&error.code) => {
                    remove_stale_endpoint(&self.endpoint_path, &self.data_directory)?;
                }
                Err(error) => return Err(error),
            }
        }

        let spec = ServiceLaunchSpec {
            endpoint_path: self.endpoint_path.clone(),
            data_directory: self.data_directory.clone(),
            service_version: self.service_version.clone(),
        };
        self.process = Some(self.launcher.launch(&spec)?);
        self.wait_until_ready().await
    }

    fn ensure_endpoint_version(&self) -> Result<(), ServiceSupervisorError> {
        let endpoint = read_endpoint(&self.endpoint_path)
            .map_err(|error| ServiceSupervisorError::new(error.code, error.message))?;
        if endpoint.service_version != self.service_version {
            return Err(ServiceSupervisorError::new(
                "HTTP_SERVICE_VERSION_UNSUPPORTED",
                format!(
                    "HTTP service version {} does not match client version {}",
                    endpoint.service_version, self.service_version
                ),
            ));
        }
        Ok(())
    }

    async fn connect_client(&mut self) -> Result<ServiceClient, ServiceSupervisorError> {
        ServiceClient::connect(
            &self.endpoint_path,
            &self.data_directory,
            self.client_id.clone(),
        )
        .await
        .map_err(Into::into)
    }

    async fn wait_until_ready(&mut self) -> Result<ServiceClient, ServiceSupervisorError> {
        let deadline = Instant::now() + self.startup_timeout;
        loop {
            if self.endpoint_path.exists() {
                match self.connect_client().await {
                    Ok(client) => {
                        self.ensure_endpoint_version()?;
                        return Ok(client);
                    }
                    Err(error) if is_stale_endpoint_error(&error.code) => {}
                    Err(error) => return Err(error),
                }
            }
            if let Some(exit_code) = self.process_exit_code()? {
                if self.data_directory.join("service.lock").exists() {
                    self.process = None;
                } else {
                    return Err(ServiceSupervisorError::new(
                        "HTTP_SERVICE_START_FAILED",
                        format!("HTTP service exited before it became ready (status {exit_code})"),
                    ));
                }
            }
            if Instant::now() >= deadline {
                return Err(ServiceSupervisorError::new(
                    "HTTP_SERVICE_START_TIMEOUT",
                    "HTTP service did not become ready before the startup timeout",
                ));
            }
            sleep(READINESS_POLL_INTERVAL).await;
        }
    }

    fn process_exit_code(&mut self) -> Result<Option<i32>, ServiceSupervisorError> {
        self.process
            .as_mut()
            .map(|process| process.try_exit())
            .transpose()
            .map(|status| status.flatten())
            .map_err(|error| {
                ServiceSupervisorError::new(
                    "HTTP_SERVICE_START_FAILED",
                    format!("cannot inspect HTTP service process: {error}"),
                )
            })
    }
}

fn is_stale_endpoint_error(code: &str) -> bool {
    code == "HTTP_SERVICE_NOT_RUNNING"
}

fn remove_stale_endpoint(path: &Path, data_directory: &Path) -> Result<(), ServiceSupervisorError> {
    let endpoint = read_endpoint(path)
        .map_err(|error| ServiceSupervisorError::new(error.code, error.message))?;
    std::fs::remove_file(path).map_err(|error| {
        ServiceSupervisorError::new(
            "HTTP_SERVICE_ENDPOINT_WRITE_FAILED",
            format!("cannot remove stale HTTP service endpoint: {error}"),
        )
    })?;
    let lease_path = data_directory.join("service.lock");
    if let Ok(lease) = std::fs::read_to_string(&lease_path) {
        if lease.lines().next() == Some(endpoint.token.as_str()) {
            std::fs::remove_file(lease_path).map_err(|error| {
                ServiceSupervisorError::new(
                    "HTTP_SERVICE_LEASE_FAILED",
                    format!("cannot remove stale HTTP service lease: {error}"),
                )
            })?;
        }
    }
    Ok(())
}
