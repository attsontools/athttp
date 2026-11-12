mod endpoint;
pub mod http;
mod protocol;
pub mod server;

#[cfg(test)]
mod endpoint_test;
#[cfg(test)]
mod protocol_test;

pub use endpoint::{read_endpoint, write_endpoint, Endpoint};
pub use protocol::{
    negotiate, ErrorEnvelope, HandshakeRequest, ServiceCapabilities, ServiceError,
    ServiceHandshake, ServiceHandshakeResponse, ServiceReply, ServiceRequest,
};
