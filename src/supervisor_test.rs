use std::{
    io,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use tempfile::tempdir;
use tokio::{sync::Notify, task::JoinHandle};

use crate::{
    server::ServiceServer, write_endpoint, Endpoint, ServiceLaunchSpec, ServiceLauncher,
    ServiceProcess, ServiceSupervisor, ServiceSupervisorError,
};

struct PanicLauncher;

impl ServiceLauncher for PanicLauncher {
    fn launch(
        &self,
        _spec: &ServiceLaunchSpec,
    ) -> Result<Box<dyn ServiceProcess>, ServiceSupervisorError> {
        panic!("healthy service should be reused")
    }
}

struct TestLauncher {
    starts: Arc<AtomicUsize>,
}

impl ServiceLauncher for TestLauncher {
    fn launch(
        &self,
        spec: &ServiceLaunchSpec,
    ) -> Result<Box<dyn ServiceProcess>, ServiceSupervisorError> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        let endpoint_path = spec.endpoint_path.clone();
        let data_directory = spec.data_directory.clone();
        let service_version = spec.service_version.clone();
        let stop = Arc::new(Notify::new());
        let task_stop = Arc::clone(&stop);
        let task = tokio::spawn(async move {
            let server = ServiceServer::bind(
                endpoint_path,
                data_directory.to_string_lossy(),
                service_version,
            )
            .await
            .unwrap();
            let server_stop = server.stop_handle();
            tokio::spawn(async move {
                task_stop.notified().await;
                server_stop.notify_one();
            });
            server.run().await.unwrap();
        });
        Ok(Box::new(TestProcess { stop, task }))
    }
}

struct TestProcess {
    stop: Arc<Notify>,
    task: JoinHandle<()>,
}

struct ExitedProcess;

impl ServiceProcess for ExitedProcess {
    fn try_exit(&mut self) -> io::Result<Option<i32>> {
        Ok(Some(1))
    }
}

struct LosingRaceLauncher {
    winner_stop: Arc<Mutex<Option<Arc<Notify>>>>,
}

struct MissingWinnerLauncher;

impl ServiceLauncher for MissingWinnerLauncher {
    fn launch(
        &self,
        spec: &ServiceLaunchSpec,
    ) -> Result<Box<dyn ServiceProcess>, ServiceSupervisorError> {
        std::fs::create_dir_all(&spec.data_directory).unwrap();
        std::fs::write(spec.data_directory.join("service.lock"), "missing").unwrap();
        Ok(Box::new(ExitedProcess))
    }
}

impl ServiceLauncher for LosingRaceLauncher {
    fn launch(
        &self,
        spec: &ServiceLaunchSpec,
    ) -> Result<Box<dyn ServiceProcess>, ServiceSupervisorError> {
        std::fs::create_dir_all(&spec.data_directory).unwrap();
        std::fs::write(spec.data_directory.join("service.lock"), "winner").unwrap();
        let spec = spec.clone();
        let winner_stop = Arc::clone(&self.winner_stop);
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            std::fs::remove_file(spec.data_directory.join("service.lock")).unwrap();
            let server = ServiceServer::bind(
                spec.endpoint_path,
                spec.data_directory.to_string_lossy(),
                spec.service_version,
            )
            .await
            .unwrap();
            *winner_stop.lock().unwrap() = Some(server.stop_handle());
            server.run().await.unwrap();
        });
        Ok(Box::new(ExitedProcess))
    }
}

impl ServiceProcess for TestProcess {
    fn try_exit(&mut self) -> io::Result<Option<i32>> {
        Ok(self.task.is_finished().then_some(0))
    }
}

impl Drop for TestProcess {
    fn drop(&mut self) {
        self.stop.notify_one();
    }
}

#[tokio::test]
async fn reuses_a_healthy_existing_service() {
    let directory = tempdir().unwrap();
    let data_directory = directory.path().join("data");
    let endpoint_path = directory.path().join("run/service.endpoint.json");
    let server = ServiceServer::bind(
        endpoint_path.clone(),
        data_directory.to_string_lossy(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .unwrap();
    let stop = server.stop_handle();
    let task = tokio::spawn(server.run());
    let mut supervisor = ServiceSupervisor::with_launcher(
        endpoint_path,
        data_directory,
        "standalone-test",
        env!("CARGO_PKG_VERSION"),
        Duration::from_secs(1),
        Arc::new(PanicLauncher),
    );

    let client = supervisor.connect().await.unwrap();
    assert_eq!(
        client
            .request("data.tabs.list", serde_json::json!({}))
            .await
            .unwrap(),
        serde_json::json!([])
    );

    stop.notify_one();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn starts_service_and_waits_until_it_is_ready() {
    let directory = tempdir().unwrap();
    let data_directory = directory.path().join("data");
    let endpoint_path = directory.path().join("run/service.endpoint.json");
    let starts = Arc::new(AtomicUsize::new(0));
    let mut supervisor = ServiceSupervisor::with_launcher(
        endpoint_path.clone(),
        data_directory,
        "standalone-test",
        env!("CARGO_PKG_VERSION"),
        Duration::from_secs(2),
        Arc::new(TestLauncher {
            starts: Arc::clone(&starts),
        }),
    );

    let client = supervisor.connect().await.unwrap();

    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert!(Path::new(&endpoint_path).exists());
    assert_eq!(
        client
            .request("data.tabs.list", serde_json::json!({}))
            .await
            .unwrap(),
        serde_json::json!([])
    );
}

#[tokio::test]
async fn does_not_replace_a_malformed_endpoint_without_process_identity() {
    let directory = tempdir().unwrap();
    let data_directory = directory.path().join("data");
    let endpoint_path = directory.path().join("run/service.endpoint.json");
    std::fs::create_dir_all(endpoint_path.parent().unwrap()).unwrap();
    std::fs::write(&endpoint_path, b"not-json").unwrap();
    let starts = Arc::new(AtomicUsize::new(0));
    let mut supervisor = ServiceSupervisor::with_launcher(
        endpoint_path,
        data_directory,
        "standalone-test",
        env!("CARGO_PKG_VERSION"),
        Duration::from_secs(2),
        Arc::new(TestLauncher {
            starts: Arc::clone(&starts),
        }),
    );

    let error = match supervisor.connect().await {
        Ok(_) => panic!("malformed endpoint must not be replaced"),
        Err(error) => error,
    };

    assert_eq!(error.code, "HTTP_SERVICE_ENDPOINT_INVALID");
    assert_eq!(starts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn only_one_server_can_own_a_data_directory_lease() {
    let directory = tempdir().unwrap();
    let data_directory = directory.path().join("data");
    let first = ServiceServer::bind(
        directory.path().join("first.endpoint.json"),
        data_directory.to_string_lossy(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .unwrap();

    let error = match ServiceServer::bind(
        directory.path().join("second.endpoint.json"),
        data_directory.to_string_lossy(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    {
        Ok(_) => panic!("second server must not own the same data directory"),
        Err(error) => error,
    };

    assert_eq!(error.code, "HTTP_SERVICE_ALREADY_RUNNING");
    let stop = first.stop_handle();
    let task = tokio::spawn(first.run());
    stop.notify_one();
    task.await.unwrap().unwrap();

    ServiceServer::bind(
        directory.path().join("third.endpoint.json"),
        data_directory.to_string_lossy(),
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn startup_race_loser_waits_for_and_reuses_the_winning_service() {
    let directory = tempdir().unwrap();
    let data_directory = directory.path().join("data");
    let endpoint_path = directory.path().join("service.endpoint.json");
    let winner_stop = Arc::new(Mutex::new(None));
    let mut supervisor = ServiceSupervisor::with_launcher(
        endpoint_path,
        data_directory,
        "standalone-test",
        env!("CARGO_PKG_VERSION"),
        Duration::from_secs(1),
        Arc::new(LosingRaceLauncher {
            winner_stop: Arc::clone(&winner_stop),
        }),
    );

    let client = supervisor.connect().await.unwrap();

    assert_eq!(
        client
            .request("data.tabs.list", serde_json::json!({}))
            .await
            .unwrap(),
        serde_json::json!([])
    );
    winner_stop.lock().unwrap().take().unwrap().notify_one();
}

#[tokio::test]
async fn replaces_a_dead_endpoint_from_an_older_service_version() {
    let directory = tempdir().unwrap();
    let data_directory = directory.path().join("data");
    let endpoint_path = directory.path().join("service.endpoint.json");
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    write_endpoint(
        &endpoint_path,
        &Endpoint {
            port,
            token: "stale-token".into(),
            service_version: "0.0.1".into(),
        },
    )
    .unwrap();
    std::fs::create_dir_all(&data_directory).unwrap();
    std::fs::write(data_directory.join("service.lock"), "stale-token\n1").unwrap();
    let starts = Arc::new(AtomicUsize::new(0));
    let mut supervisor = ServiceSupervisor::with_launcher(
        endpoint_path,
        data_directory,
        "standalone-test",
        env!("CARGO_PKG_VERSION"),
        Duration::from_secs(1),
        Arc::new(TestLauncher {
            starts: Arc::clone(&starts),
        }),
    );

    supervisor.connect().await.unwrap();

    assert_eq!(starts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn rejects_a_healthy_service_from_a_different_app_version() {
    let directory = tempdir().unwrap();
    let data_directory = directory.path().join("data");
    let endpoint_path = directory.path().join("service.endpoint.json");
    let server = ServiceServer::bind(
        endpoint_path.clone(),
        data_directory.to_string_lossy(),
        "0.0.1",
    )
    .await
    .unwrap();
    let stop = server.stop_handle();
    let task = tokio::spawn(server.run());
    let mut supervisor = ServiceSupervisor::with_launcher(
        endpoint_path,
        data_directory,
        "standalone-test",
        env!("CARGO_PKG_VERSION"),
        Duration::from_secs(1),
        Arc::new(PanicLauncher),
    );

    let error = match supervisor.connect().await {
        Ok(_) => panic!("different app version must not be reused"),
        Err(error) => error,
    };

    assert_eq!(error.code, "HTTP_SERVICE_VERSION_UNSUPPORTED");
    stop.notify_one();
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn startup_race_without_a_winner_ends_at_the_readiness_budget() {
    let directory = tempdir().unwrap();
    let mut supervisor = ServiceSupervisor::with_launcher(
        directory.path().join("service.endpoint.json"),
        directory.path().join("data"),
        "standalone-test",
        env!("CARGO_PKG_VERSION"),
        Duration::from_millis(50),
        Arc::new(MissingWinnerLauncher),
    );

    let error = match supervisor.connect().await {
        Ok(_) => panic!("missing winner must not produce a client"),
        Err(error) => error,
    };

    assert_eq!(error.code, "HTTP_SERVICE_START_TIMEOUT");
}

#[tokio::test]
async fn migration_failure_releases_the_service_lease() {
    let directory = tempdir().unwrap();
    let data_directory = directory.path().join("data");
    std::fs::create_dir_all(&data_directory).unwrap();
    std::fs::write(data_directory.join("http.sqlite3"), b"not-sqlite").unwrap();

    let result = ServiceServer::bind(
        directory.path().join("service.endpoint.json"),
        data_directory.to_string_lossy(),
        env!("CARGO_PKG_VERSION"),
    )
    .await;

    assert!(matches!(
        result,
        Err(ref error) if error.code == "HTTP_DATA_MIGRATION_FAILED"
    ));
    assert!(!data_directory.join("service.lock").exists());
}
