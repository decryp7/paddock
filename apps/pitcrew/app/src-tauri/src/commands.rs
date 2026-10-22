//! The Tauri commands the UI calls (`app/src/app/core/tauri-backend.ts`). Each is a thin edge over a pure function
//! that takes its dependencies as arguments, so the logic is tested without Tauri.

use crate::dto::{AppUpdateStatusDto, CommandError, CommandErrorCode, RunnerSnapshotDto};
use crate::service_client::{ClientError, ServiceClient};
use crate::updater;
use pitcrew_core::protocol::{Request, Response};
use std::sync::Arc;
use tauri::{AppHandle, Runtime, State};

/// The service client, managed by Tauri.
pub type SharedClient = Arc<dyn ServiceClient>;

impl From<ClientError> for CommandError {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::Service(s) => s.into(),
            ClientError::NotImplemented(_) => CommandError {
                code: CommandErrorCode::NotImplemented,
                message: e.to_string(),
            },
            ClientError::Unavailable(_) => CommandError {
                code: CommandErrorCode::ServiceUnavailable,
                message: e.to_string(),
            },
            ClientError::Protocol(_) => CommandError {
                code: CommandErrorCode::Internal,
                message: e.to_string(),
            },
        }
    }
}

/// Every runner's status, from the service.
pub fn status(client: &dyn ServiceClient) -> Result<Vec<RunnerSnapshotDto>, CommandError> {
    match client.request(Request::Status)? {
        Response::Status { runners } => Ok(runners.into_iter().map(RunnerSnapshotDto::from).collect()),
        Response::Error(e) => Err(e.into()),
        other => Err(ClientError::Protocol(format!("{other:?}")).into()),
    }
}

/// `runner_status`: off the UI thread, since the service may take a moment to answer.
#[tauri::command]
pub async fn runner_status(client: State<'_, SharedClient>) -> Result<Vec<RunnerSnapshotDto>, CommandError> {
    let client = client.inner().clone();
    tauri::async_runtime::spawn_blocking(move || status(client.as_ref()))
        .await
        .map_err(|e| CommandError {
            code: CommandErrorCode::Internal,
            message: format!("The status request failed: {e}"),
        })?
}

/// `app_update_status`: whether this build can update itself.
#[tauri::command]
pub fn app_update_status<R: Runtime>(app: AppHandle<R>) -> AppUpdateStatusDto {
    updater::status(&app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitcrew_core::distribution::RunnerKind;
    use pitcrew_core::health::RunnerStatus;
    use pitcrew_core::names::RunnerName;
    use pitcrew_core::protocol::{RunnerSnapshot, ServiceError};
    use pitcrew_core::update::UpdatePhase;

    struct Fake(Result<Response, ClientError>);

    impl ServiceClient for Fake {
        fn request(&self, _: Request) -> Result<Response, ClientError> {
            self.0.clone()
        }
    }

    #[test]
    fn status_maps_the_service_answer() {
        let runner = RunnerSnapshot {
            name: RunnerName::parse("a").unwrap(),
            kind: RunnerKind::GiteaRunner,
            status: RunnerStatus::Idle,
            current_version: None,
            latest_version: None,
            last_update: None,
            update_phase: UpdatePhase::Idle,
        };
        let ok = status(&Fake(Ok(Response::Status { runners: vec![runner] }))).unwrap();
        assert_eq!(ok[0].name, "a");
    }

    #[test]
    fn status_errors_are_typed_for_the_ui() {
        let e = status(&Fake(Err(ClientError::NotImplemented("Connecting")))).unwrap_err();
        assert_eq!(e.code, CommandErrorCode::NotImplemented);
        let e = status(&Fake(Err(ClientError::Unavailable("no pipe".into())))).unwrap_err();
        assert_eq!(e.code, CommandErrorCode::ServiceUnavailable);
        let e = status(&Fake(Ok(Response::Error(ServiceError::not_implemented("Runner status"))))).unwrap_err();
        assert_eq!(e.code, CommandErrorCode::NotImplemented);
        let e = status(&Fake(Ok(Response::Ok))).unwrap_err();
        assert_eq!(e.code, CommandErrorCode::Internal);
        let e = status(&Fake(Err(ClientError::Service(ServiceError::not_implemented("x"))))).unwrap_err();
        assert_eq!(e.code, CommandErrorCode::NotImplemented);
    }
}
