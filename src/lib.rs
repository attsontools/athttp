mod client;
mod endpoint;
pub mod http;
mod protocol;
pub mod server;
mod service_cli;
mod standalone_service;
mod supervisor;

#[cfg(test)]
mod client_test;
#[cfg(test)]
mod endpoint_test;
#[cfg(test)]
mod protocol_test;
#[cfg(test)]
mod service_cli_test;
#[cfg(test)]
mod standalone_service_test;
#[cfg(test)]
mod supervisor_test;

pub use client::{ServiceClient, ServiceClientError, ServiceEvent};
pub use endpoint::{read_endpoint, write_endpoint, Endpoint};
pub use protocol::{
    negotiate, ErrorEnvelope, HandshakeRequest, ServiceCapabilities, ServiceError,
    ServiceHandshake, ServiceHandshakeResponse, ServiceReply, ServiceRequest, APP_ID, DATA_API,
    HOST_API, SERVICE_API,
};
pub use service_cli::{parse_service_args, ServiceRunOptions};
pub use standalone_service::StandaloneService;
pub use supervisor::{
    CommandServiceLauncher, ServiceLaunchSpec, ServiceLauncher, ServiceProcess, ServiceSupervisor,
    ServiceSupervisorError,
};
