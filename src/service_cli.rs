use std::{ffi::OsString, path::PathBuf};

use crate::ServiceError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRunOptions {
    pub endpoint_path: PathBuf,
    pub data_directory: PathBuf,
}

pub fn parse_service_args(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<ServiceRunOptions, ServiceError> {
    let mut arguments = arguments.into_iter();
    let mut endpoint_path = None;
    let mut data_directory = None;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--endpoint") if endpoint_path.is_none() => {
                endpoint_path = Some(required_path(arguments.next(), "--endpoint")?);
            }
            Some("--data-directory") if data_directory.is_none() => {
                data_directory = Some(required_path(arguments.next(), "--data-directory")?);
            }
            _ => return Err(invalid_arguments()),
        }
    }
    match (endpoint_path, data_directory) {
        (Some(endpoint_path), Some(data_directory)) => Ok(ServiceRunOptions {
            endpoint_path,
            data_directory,
        }),
        _ => Err(invalid_arguments()),
    }
}

fn required_path(value: Option<OsString>, option: &str) -> Result<PathBuf, ServiceError> {
    let path = value.map(PathBuf::from).ok_or_else(invalid_arguments)?;
    if path.as_os_str().is_empty() {
        return Err(ServiceError::new(
            "HTTP_SERVICE_ARGUMENT_INVALID",
            format!("{option} requires a non-empty path"),
        ));
    }
    Ok(path)
}

fn invalid_arguments() -> ServiceError {
    ServiceError::new(
        "HTTP_SERVICE_ARGUMENT_INVALID",
        "usage: athttp-service --endpoint <path> --data-directory <path>",
    )
}
