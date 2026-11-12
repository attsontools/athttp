use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "com.attson.http";
pub const HOST_API: &str = "2.0.0";
pub const DATA_API: &str = "1.0.0";
pub const SERVICE_API: &str = "1.0.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeRequest {
    pub protocol_version: u32,
    pub app_id: String,
    pub client_id: String,
    pub request_id: String,
    pub host_api: String,
    pub data_api: String,
    pub service_api: String,
    pub data_directory: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServiceCapabilities {
    pub http_request: bool,
    pub http_stream: bool,
    pub data_store: bool,
    pub openapi_import: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceHandshakeResponse {
    pub protocol_version: u32,
    pub request_id: String,
    pub app_id: String,
    pub host_api: String,
    pub data_api: String,
    pub service_api: String,
    pub schema_version: u32,
    pub migration_state: String,
    pub lease_state: String,
    pub capabilities: ServiceCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ServiceHandshake {
    Request(HandshakeRequest),
    Response(ServiceHandshakeResponse),
}

impl ServiceHandshake {
    pub fn request(request: HandshakeRequest) -> Self {
        Self::Request(request)
    }

    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Request(request) => Some(request.request_id.as_str()),
            Self::Response(response) => Some(response.request_id.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl ServiceError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorEnvelope {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl From<ServiceError> for ErrorEnvelope {
    fn from(error: ServiceError) -> Self {
        Self {
            code: error.code.into(),
            message: error.message,
            retryable: error.retryable,
        }
    }
}

pub fn negotiate(request: &HandshakeRequest, expected_token: &str) -> Result<ServiceHandshakeResponse, ServiceError> {
    if request.protocol_version != 1 || request.app_id != APP_ID || request.data_directory.trim().is_empty() {
        return Err(ServiceError::new(
            "HTTP_SERVICE_REQUEST_INVALID",
            "handshake request is invalid",
        ));
    }
    if request.token != expected_token {
        return Err(ServiceError::new(
            "HTTP_SERVICE_AUTH_FAILED",
            "service authentication failed",
        ));
    }
    if !compatible(&request.host_api, HOST_API)
        || !compatible(&request.data_api, DATA_API)
        || !compatible(&request.service_api, SERVICE_API)
    {
        return Err(ServiceError::new(
            "HTTP_SERVICE_VERSION_UNSUPPORTED",
            "client and service API versions are incompatible",
        ));
    }
    Ok(ServiceHandshakeResponse {
        protocol_version: 1,
        request_id: request.request_id.clone(),
        app_id: APP_ID.into(),
        host_api: HOST_API.into(),
        data_api: DATA_API.into(),
        service_api: SERVICE_API.into(),
        schema_version: 1,
        migration_state: "ready".into(),
        lease_state: "owner".into(),
        capabilities: ServiceCapabilities {
            http_request: true,
            http_stream: true,
            data_store: true,
            openapi_import: true,
        },
    })
}

fn compatible(client_range: &str, current: &str) -> bool {
    let current = match Version::parse(current) {
        Ok(version) => version,
        Err(_) => return false,
    };
    let normalized = match client_range.trim() {
        value if value.ends_with(".x") => format!(">={}.0.0,<{}.0.0", &value[..value.len() - 2], current.major + 1),
        value if value.chars().all(|character| character.is_ascii_digit() || character == '.') => {
            format!("={value}")
        }
        value => value.to_string(),
    };
    VersionReq::parse(&normalized)
        .map(|requirement| requirement.matches(&current))
        .unwrap_or(false)
}
