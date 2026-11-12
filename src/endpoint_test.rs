use super::endpoint::{read_endpoint, write_endpoint, Endpoint};

#[test]
fn endpoint_is_atomically_written_and_read_back() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("service.endpoint.json");
    let endpoint = Endpoint {
        port: 42_123,
        token: "token-value".into(),
        service_version: "0.1.0-alpha.0".into(),
    };

    write_endpoint(&path, &endpoint).unwrap();
    assert_eq!(read_endpoint(&path).unwrap(), endpoint);
    assert!(!path.with_extension("tmp").exists());
}

#[test]
fn malformed_endpoint_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("service.endpoint.json");
    std::fs::write(&path, b"not-json").unwrap();
    let error = read_endpoint(&path).unwrap_err();
    assert_eq!(error.code, "HTTP_SERVICE_ENDPOINT_INVALID");
}
