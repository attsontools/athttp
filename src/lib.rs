mod client;
mod endpoint;
pub mod http;
mod protocol;
pub mod server;

#[cfg(test)]
mod client_test;
#[cfg(test)]
mod endpoint_test;
#[cfg(test)]
mod protocol_test;

pub use client::{ServiceClient, ServiceClientError, ServiceEvent};
pub use endpoint::{read_endpoint, write_endpoint, Endpoint};
pub use protocol::{
    negotiate, ErrorEnvelope, HandshakeRequest, ServiceCapabilities, ServiceError,
    ServiceHandshake, ServiceHandshakeResponse, ServiceReply, ServiceRequest, APP_ID, DATA_API,
    HOST_API, SERVICE_API,
};
