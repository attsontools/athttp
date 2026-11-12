use std::path::Path;

use serde_json::json;
use tempfile::tempdir;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    time::{timeout, Duration},
};

use crate::{write_endpoint, Endpoint, ServiceClient};

#[test]
fn stream_message_wire_fields_are_camel_case() {
    let message = crate::http::StreamMessage::WsBinary {
        at_ms: 11,
        direction: crate::http::Direction::In,
        bytes_len: 3,
        preview_b64: "YWJj".into(),
    };

    assert_eq!(
        serde_json::to_value(message).unwrap(),
        json!({
            "kind": "wsBinary",
            "atMs": 11,
            "direction": "in",
            "bytesLen": 3,
            "previewB64": "YWJj"
        })
    );
}

#[tokio::test]
async fn client_handshakes_and_routes_responses_and_events() {
    let directory = tempdir().unwrap();
    let data_directory = directory.path().join("data");
    std::fs::create_dir_all(&data_directory).unwrap();
    let endpoint_path = directory.path().join("service.endpoint.json");
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    write_endpoint(
        &endpoint_path,
        &Endpoint {
            port: listener.local_addr().unwrap().port(),
            token: "secret-token".into(),
            service_version: "0.1.0-alpha.0".into(),
        },
    )
    .unwrap();

    let expected_data_directory = data_directory.to_string_lossy().to_string();
    let server = tokio::spawn(fake_service(listener, expected_data_directory.clone()));
    let client = ServiceClient::connect(
        &endpoint_path,
        Path::new(&expected_data_directory),
        "test-client",
    )
    .await
    .unwrap();
    let mut events = client.subscribe();

    let result = client.request("data.tabs.list", json!({})).await.unwrap();
    assert_eq!(result, json!([{"id": "tab-1"}]));

    let event = events.recv().await.unwrap();
    assert_eq!(event.session_id, "session-1");
    assert_eq!(event.generation, 3);
    assert!(matches!(
        event.message,
        crate::http::StreamMessage::Open { .. }
    ));
    server.await.unwrap();
}

async fn fake_service(listener: TcpListener, data_directory: String) {
    let (stream, _) = listener.accept().await.unwrap();
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let handshake: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(handshake["token"], "secret-token");
    assert_eq!(handshake["dataDirectory"], data_directory);
    let request_id = handshake["requestId"].as_str().unwrap();
    write
        .write_all(
            format!(
                "{}\n",
                json!({
                    "kind": "handshake",
                    "protocolVersion": 1,
                    "requestId": request_id,
                    "appId": "com.attson.http",
                    "hostApi": "2.0.0",
                    "dataApi": "1.0.0",
                    "serviceApi": "1.0.0",
                    "schemaVersion": 1,
                    "migrationState": "ready",
                    "leaseState": "owner",
                    "capabilities": {
                        "httpRequest": true,
                        "httpStream": true,
                        "dataStore": true,
                        "openapiImport": true
                    }
                })
            )
            .as_bytes(),
        )
        .await
        .unwrap();

    line.clear();
    reader.read_line(&mut line).await.unwrap();
    let request: serde_json::Value = serde_json::from_str(&line).unwrap();
    let request_id = request["requestId"].as_str().unwrap();
    write
        .write_all(
            format!(
                "{}\n{}\n",
                json!({
                    "kind": "event",
                    "sessionId": "session-1",
                    "generation": 3,
                    "message": { "kind": "open", "atMs": 1, "status": 200, "headers": [] }
                }),
                json!({
                    "kind": "response",
                    "requestId": request_id,
                    "result": [{"id": "tab-1"}]
                })
            )
            .as_bytes(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn request_after_service_disconnect_fails_without_waiting_for_timeout() {
    let directory = tempdir().unwrap();
    let data_directory = directory.path().join("data");
    std::fs::create_dir_all(&data_directory).unwrap();
    let endpoint_path = directory.path().join("service.endpoint.json");
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    write_endpoint(
        &endpoint_path,
        &Endpoint {
            port: listener.local_addr().unwrap().port(),
            token: "secret-token".into(),
            service_version: env!("CARGO_PKG_VERSION").into(),
        },
    )
    .unwrap();
    let expected_data_directory = data_directory.to_string_lossy().to_string();
    let server = tokio::spawn(close_after_handshake(
        listener,
        expected_data_directory.clone(),
    ));
    let client = ServiceClient::connect(
        &endpoint_path,
        Path::new(&expected_data_directory),
        "test-client",
    )
    .await
    .unwrap();
    server.await.unwrap();
    tokio::task::yield_now().await;

    let error = timeout(
        Duration::from_millis(250),
        client.request("data.tabs.list", json!({})),
    )
    .await
    .expect("disconnect should be reported immediately")
    .unwrap_err();

    assert_eq!(error.code, "HTTP_SERVICE_DISCONNECTED");
}

async fn close_after_handshake(listener: TcpListener, data_directory: String) {
    let (stream, _) = listener.accept().await.unwrap();
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let handshake: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(handshake["dataDirectory"], data_directory);
    let request_id = handshake["requestId"].as_str().unwrap();
    write
        .write_all(
            format!(
                "{}\n",
                json!({
                    "kind": "handshake",
                    "protocolVersion": 1,
                    "requestId": request_id,
                    "appId": "com.attson.http",
                    "hostApi": "2.0.0",
                    "dataApi": "1.0.0",
                    "serviceApi": "1.0.0",
                    "schemaVersion": 1,
                    "migrationState": "ready",
                    "leaseState": "owner",
                    "capabilities": {
                        "httpRequest": true,
                        "httpStream": true,
                        "dataStore": true,
                        "openapiImport": true
                    }
                })
            )
            .as_bytes(),
        )
        .await
        .unwrap();
}
