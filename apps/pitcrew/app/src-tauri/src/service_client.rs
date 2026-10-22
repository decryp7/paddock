//! The app's side of the app ↔ service protocol (ADR 0001). Commands depend on the [`ServiceClient`] trait, never on
//! a transport, so the named pipe / Unix socket client can be swapped and the commands tested with a fake.

use pitcrew_core::protocol::{Request, Response, ServiceError};

/// Why a request could not be answered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    /// The service is not running or not installed.
    #[error("The Pitcrew service is not running: {0}")]
    Unavailable(String),
    /// The client is not built yet.
    #[error("{0} is not implemented yet")]
    NotImplemented(&'static str),
    /// The service answered something that is not part of the protocol.
    #[error("The Pitcrew service gave an unexpected answer: {0}")]
    Protocol(String),
    /// The service refused or failed the request.
    #[error(transparent)]
    Service(#[from] ServiceError),
}

/// Sends one request to the service and returns its response. Implementations must be safe to call from any thread
/// and must time out instead of blocking forever.
pub trait ServiceClient: Send + Sync {
    /// One request, one response.
    fn request(&self, request: Request) -> Result<Response, ClientError>;
}

/// The client of the local service (named pipe on Windows, Unix socket on macOS). Not built yet.
#[derive(Debug, Default)]
pub struct LocalServiceClient;

impl ServiceClient for LocalServiceClient {
    fn request(&self, _request: Request) -> Result<Response, ClientError> {
        Err(ClientError::NotImplemented("Connecting to the Pitcrew service"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_local_client_is_honest_about_not_being_built() {
        assert_eq!(
            LocalServiceClient.request(Request::Status),
            Err(ClientError::NotImplemented("Connecting to the Pitcrew service"))
        );
        assert!(ClientError::Unavailable("pipe not found".into())
            .to_string()
            .starts_with("The Pitcrew service is not running"));
    }
}
