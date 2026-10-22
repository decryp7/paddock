//! Answers the app's requests (ADR 0001). The transport (named pipe / Unix socket) is not built yet; this is the
//! part both transports will call, so its behaviour is already fixed and tested.

use pitcrew_core::protocol::{answer_hello, Request, Response, ServiceError};

/// The service's version, reported in the hello.
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Answers one request. Everything but the hello is honest about not being built yet.
pub fn handle(request: Request) -> Response {
    let not_yet = |what: &str| Response::Error(ServiceError::not_implemented(what));
    match request {
        Request::Hello { protocol } => answer_hello(protocol, SERVICE_VERSION),
        Request::Status => not_yet("Runner status"),
        Request::Start { .. } | Request::Stop { .. } | Request::Restart { .. } => not_yet("Starting and stopping runners"),
        Request::Logs { .. } | Request::SearchLogs { .. } => not_yet("Runner logs"),
        Request::CheckUpdate { .. } | Request::ApplyUpdate { .. } | Request::CancelUpdate { .. } => not_yet("Runner updates"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitcrew_core::names::RunnerName;
    use pitcrew_core::protocol::{ErrorCode, PROTOCOL_VERSION};

    #[test]
    fn hello_works_and_everything_else_says_not_implemented() {
        assert!(matches!(
            handle(Request::Hello {
                protocol: PROTOCOL_VERSION
            }),
            Response::Hello { .. }
        ));
        let runner = RunnerName::parse("a").unwrap();
        for r in [
            Request::Status,
            Request::Start { runner: runner.clone() },
            Request::CheckUpdate { runner },
        ] {
            assert!(matches!(
                handle(r),
                Response::Error(ServiceError {
                    code: ErrorCode::NotImplemented,
                    ..
                })
            ));
        }
    }
}
