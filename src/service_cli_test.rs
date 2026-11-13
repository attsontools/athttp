use std::{ffi::OsString, path::PathBuf};

use crate::parse_service_args;

#[test]
fn service_cli_requires_explicit_endpoint_and_data_directory() {
    let error = parse_service_args(Vec::<OsString>::new()).unwrap_err();

    assert_eq!(error.code, "HTTP_SERVICE_ARGUMENT_INVALID");
}

#[test]
fn service_cli_parses_explicit_paths() {
    let options = parse_service_args([
        OsString::from("--endpoint"),
        OsString::from("/tmp/athttp/service.json"),
        OsString::from("--data-directory"),
        OsString::from("/tmp/athttp/data"),
    ])
    .unwrap();

    assert_eq!(
        options.endpoint_path,
        PathBuf::from("/tmp/athttp/service.json")
    );
    assert_eq!(options.data_directory, PathBuf::from("/tmp/athttp/data"));
}
