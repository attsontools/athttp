use super::protocol::{
    negotiate, ErrorEnvelope, HandshakeRequest, ServiceCapabilities, ServiceError, ServiceHandshake,
};

fn request(host_api: &str, data_api: &str, service_api: &str) -> HandshakeRequest {
    HandshakeRequest {
        protocol_version: 1,
        app_id: "com.attson.http".into(),
        client_id: "test-client".into(),
        request_id: "req-1".into(),
        host_api: host_api.into(),
        data_api: data_api.into(),
        service_api: service_api.into(),
        data_directory: "/tmp/http".into(),
        token: "token".into(),
    }
}

#[test]
fn accepts_compatible_ranges_and_returns_capabilities() {
    let result = negotiate(&request("2.0.0", "1.0.0", "1.0.0"), "token", "/tmp/http").unwrap();
    assert_eq!(result.service_api, "1.0.0");
    assert_eq!(result.data_api, "1.0.0");
    assert_eq!(result.host_api, "2.0.0");
    assert_eq!(
        result.capabilities,
        ServiceCapabilities {
            http_request: true,
            http_stream: true,
            data_store: true,
            openapi_import: true,
        }
    );
}

#[test]
fn rejects_wrong_token_without_leaking_expected_value() {
    let error = negotiate(
        &request("2.0.0", "1.0.0", "1.0.0"),
        "other-token",
        "/tmp/http",
    )
    .unwrap_err();
    assert_eq!(error.code, "HTTP_SERVICE_AUTH_FAILED");
    assert!(!error.message.contains("token"));
}

#[test]
fn rejects_incompatible_api_with_stable_error() {
    let error = negotiate(&request("1.0.0", "1.0.0", "1.0.0"), "token", "/tmp/http").unwrap_err();
    assert_eq!(error.code, "HTTP_SERVICE_VERSION_UNSUPPORTED");
}

#[test]
fn rejects_a_different_major_wildcard() {
    let error = negotiate(&request("1.x", "1.0.0", "1.0.0"), "token", "/tmp/http").unwrap_err();
    assert_eq!(error.code, "HTTP_SERVICE_VERSION_UNSUPPORTED");
}

#[test]
fn rejects_a_client_claiming_a_different_data_directory() {
    let error = negotiate(
        &request("2.0.0", "1.0.0", "1.0.0"),
        "token",
        "/tmp/another-http",
    )
    .unwrap_err();
    assert_eq!(error.code, "HTTP_SERVICE_REQUEST_INVALID");
}

#[test]
fn wire_envelopes_round_trip_without_losing_request_id() {
    let envelope = ServiceHandshake::request(request("2.0.0", "1.0.0", "1.0.0"));
    let encoded = serde_json::to_vec(&envelope).unwrap();
    let decoded: ServiceHandshake = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded.request_id(), Some("req-1"));

    let error = ErrorEnvelope::from(ServiceError::new(
        "HTTP_SERVICE_UNAVAILABLE",
        "service is unavailable",
    ));
    assert_eq!(error.code, "HTTP_SERVICE_UNAVAILABLE");
}
