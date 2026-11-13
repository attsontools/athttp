use std::{sync::Arc, time::Duration};

use tempfile::tempdir;

use crate::{
    server::ServiceServer, ServiceLaunchSpec, ServiceLauncher, ServiceProcess, ServiceSupervisor,
    ServiceSupervisorError, StandaloneService,
};

struct UnexpectedLauncher;

impl ServiceLauncher for UnexpectedLauncher {
    fn launch(
        &self,
        _spec: &ServiceLaunchSpec,
    ) -> Result<Box<dyn ServiceProcess>, ServiceSupervisorError> {
        panic!("test server is already running")
    }
}

#[tokio::test]
async fn standalone_service_rejects_requests_before_connect() {
    let directory = tempdir().unwrap();
    let supervisor = ServiceSupervisor::with_launcher(
        directory.path().join("service.endpoint.json"),
        directory.path().join("data"),
        "standalone-test",
        env!("CARGO_PKG_VERSION"),
        Duration::from_secs(1),
        Arc::new(UnexpectedLauncher),
    );
    let service = StandaloneService::new(supervisor);

    let error = service
        .request("data.tabs.list", serde_json::json!({}))
        .await
        .unwrap_err();

    assert_eq!(error.code, "HTTP_SERVICE_NOT_CONNECTED");
}

#[tokio::test]
async fn standalone_service_routes_concurrent_requests_after_connect() {
    let directory = tempdir().unwrap();
    let data_directory = directory.path().join("data");
    let endpoint_path = directory.path().join("service.endpoint.json");
    let server = ServiceServer::bind(
        endpoint_path.clone(),
        data_directory.to_string_lossy(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .unwrap();
    let stop = server.stop_handle();
    let task = tokio::spawn(server.run());
    let supervisor = ServiceSupervisor::with_launcher(
        endpoint_path,
        data_directory,
        "standalone-test",
        env!("CARGO_PKG_VERSION"),
        Duration::from_secs(1),
        Arc::new(UnexpectedLauncher),
    );
    let service = StandaloneService::new(supervisor);
    let _events = service.connect().await.unwrap();

    let (tabs, envs) = tokio::join!(
        service.request("data.tabs.list", serde_json::json!({})),
        service.request("data.envs.list", serde_json::json!({}))
    );

    assert_eq!(tabs.unwrap(), serde_json::json!([]));
    assert_eq!(envs.unwrap(), serde_json::json!([]));
    stop.notify_one();
    task.await.unwrap().unwrap();
}
