//! Typed errors of the VNC client.

use sighter_core::ports::TargetError;

/// Why a VNC conversation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RfbError {
    /// The socket failed or closed.
    #[error("connection problem: {0}")]
    Io(String),
    /// A read or write did not finish in time.
    #[error("the VNC server did not answer in time")]
    Timeout,
    /// The peer does not speak RFB.
    #[error("not a VNC server (got {0:?})")]
    NotVnc(String),
    /// The peer sent something the protocol does not allow, or over our limits.
    #[error("invalid VNC data: {0}")]
    Protocol(String),
    /// The server refused the connection and said why.
    #[error("the VNC server refused the connection: {0}")]
    Refused(String),
    /// The server wants a password and none was given.
    #[error("the VNC server needs a password")]
    PasswordRequired,
    /// The password was wrong.
    #[error("VNC authentication failed (wrong password?) {0}")]
    AuthFailed(String),
    /// The server offers no security type we support.
    #[error("the VNC server offers only security types {0:?}; supported are None (1) and VNC password (2)")]
    UnsupportedSecurity(Vec<u8>),
}

impl From<std::io::Error> for RfbError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => RfbError::Timeout,
            std::io::ErrorKind::UnexpectedEof => RfbError::Io("the connection was closed".into()),
            _ => RfbError::Io(e.to_string()),
        }
    }
}

impl RfbError {
    /// The port error: a timeout reports the limit that was hit, everything else is a lost or broken target.
    pub fn into_target(self, limit: std::time::Duration) -> TargetError {
        match self {
            RfbError::Timeout => TargetError::Timeout(limit),
            RfbError::Protocol(m) => TargetError::Protocol(m),
            other => TargetError::Lost(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};
    use std::time::Duration;

    #[test]
    fn io_errors_are_classified() {
        assert_eq!(RfbError::from(Error::from(ErrorKind::TimedOut)), RfbError::Timeout);
        assert_eq!(RfbError::from(Error::from(ErrorKind::WouldBlock)), RfbError::Timeout);
        assert!(matches!(RfbError::from(Error::from(ErrorKind::UnexpectedEof)), RfbError::Io(m) if m.contains("closed")));
        assert!(matches!(RfbError::from(Error::other("boom")), RfbError::Io(m) if m.contains("boom")));
    }

    #[test]
    fn port_mapping_keeps_infrastructure_apart() {
        assert_eq!(
            RfbError::Timeout.into_target(Duration::from_millis(500)),
            TargetError::Timeout(Duration::from_millis(500))
        );
        assert_eq!(
            RfbError::Protocol("x".into()).into_target(Duration::ZERO),
            TargetError::Protocol("x".into())
        );
        assert!(matches!(RfbError::PasswordRequired.into_target(Duration::ZERO), TargetError::Lost(m) if m.contains("password")));
        for e in [
            RfbError::NotVnc("x".into()),
            RfbError::Refused("busy".into()),
            RfbError::AuthFailed(String::new()),
            RfbError::UnsupportedSecurity(vec![5]),
        ] {
            assert!(!e.to_string().is_empty());
            assert!(matches!(e.into_target(Duration::ZERO), TargetError::Lost(_)));
        }
    }
}
