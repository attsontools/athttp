use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub port: u16,
    pub token: String,
    pub service_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for EndpointError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for EndpointError {}

pub fn write_endpoint(path: &Path, endpoint: &Endpoint) -> Result<(), EndpointError> {
    if endpoint.port == 0
        || endpoint.token.trim().is_empty()
        || endpoint.service_version.trim().is_empty()
    {
        return Err(EndpointError {
            code: "HTTP_SERVICE_ENDPOINT_INVALID",
            message: "endpoint fields are invalid".into(),
        });
    }
    let parent = path.parent().ok_or_else(|| EndpointError {
        code: "HTTP_SERVICE_ENDPOINT_WRITE_FAILED",
        message: "endpoint path has no parent".into(),
    })?;
    fs::create_dir_all(parent).map_err(|error| EndpointError {
        code: "HTTP_SERVICE_ENDPOINT_WRITE_FAILED",
        message: format!("cannot create endpoint directory: {error}"),
    })?;
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec(endpoint).map_err(|error| EndpointError {
        code: "HTTP_SERVICE_ENDPOINT_WRITE_FAILED",
        message: format!("cannot encode endpoint: {error}"),
    })?;
    fs::write(&temporary, bytes).map_err(|error| EndpointError {
        code: "HTTP_SERVICE_ENDPOINT_WRITE_FAILED",
        message: format!("cannot write endpoint: {error}"),
    })?;
    set_private_permissions(&temporary);
    fs::rename(&temporary, path).map_err(|error| EndpointError {
        code: "HTTP_SERVICE_ENDPOINT_WRITE_FAILED",
        message: format!("cannot activate endpoint: {error}"),
    })?;
    Ok(())
}

pub fn read_endpoint(path: &Path) -> Result<Endpoint, EndpointError> {
    let bytes = fs::read(path).map_err(|error| EndpointError {
        code: "HTTP_SERVICE_ENDPOINT_READ_FAILED",
        message: format!("cannot read endpoint: {error}"),
    })?;
    let endpoint: Endpoint = serde_json::from_slice(&bytes).map_err(|error| EndpointError {
        code: "HTTP_SERVICE_ENDPOINT_INVALID",
        message: format!("cannot decode endpoint: {error}"),
    })?;
    if endpoint.port == 0
        || endpoint.token.trim().is_empty()
        || endpoint.service_version.trim().is_empty()
    {
        return Err(EndpointError {
            code: "HTTP_SERVICE_ENDPOINT_INVALID",
            message: "endpoint fields are invalid".into(),
        });
    }
    Ok(endpoint)
}

fn set_private_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o600);
            let _ = fs::set_permissions(path, permissions);
        }
    }
}
