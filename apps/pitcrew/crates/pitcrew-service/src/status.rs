//! `pitcrew-service status`: asks the running service for its status over the app's transport and prints it — for
//! scripts (`scripts/macos-verify.sh`) and support, without the app.

use crate::error::Error;
use pitcrew_core::protocol::{read_frame, write_frame, Request, Response, RunnerSnapshot, PROTOCOL_VERSION};
use std::io::{Read, Write};
use std::path::Path;

trait Stream: Read + Write {}
impl<T: Read + Write> Stream for T {}

fn connect(_socket_dir: &Path) -> Result<Box<dyn Stream>, Error> {
    let unavailable = |e: std::io::Error| Error::Os {
        what: "Connecting to the Pitcrew service",
        reason: format!("{e} (is it running?)"),
    };
    #[cfg(windows)]
    {
        let name = crate::ipc::pipe_name(_socket_dir);
        let f = pitcrew_core::service_paths::open_pipe_options().open(&name).map_err(unavailable)?;
        // Nothing is written before the pipe is shown to belong to the service (SYSTEM or Administrators), or, for the per-user
        // engine's own pipe, to this user.
        let own = (name != crate::ipc::PIPE_NAME)
            .then(crate::platform::win_session::current_user_sid)
            .transpose();
        let owner = crate::platform::win_session::pipe_owner_sid(&f).map_err(|e| Error::Os {
            what: "Checking the Pitcrew pipe",
            reason: format!("{e}; nothing was sent"),
        })?;
        let own = own.map_err(|e| Error::Os {
            what: "Checking the Pitcrew pipe",
            reason: format!("{e}; nothing was sent"),
        })?;
        if !pitcrew_core::service_paths::pipe_owner_ok(&owner, own.as_deref()) {
            return Err(Error::Os {
                what: "Checking the Pitcrew pipe",
                reason: format!("the pipe is owned by {owner}, not by the Pitcrew service; nothing was sent"),
            });
        }
        Ok(Box::new(f))
    }
    #[cfg(unix)]
    {
        let s = std::os::unix::net::UnixStream::connect(_socket_dir.join(crate::ipc::SOCKET_FILE)).map_err(unavailable)?;
        s.set_read_timeout(Some(std::time::Duration::from_secs(30))).map_err(unavailable)?;
        Ok(Box::new(s))
    }
    #[cfg(not(any(windows, unix)))]
    {
        Err(unavailable(std::io::Error::other("no transport")))
    }
}

/// One line per runner.
pub fn line(r: &RunnerSnapshot) -> String {
    let job = r
        .current_job
        .as_ref()
        .map(|j| format!(" job={}", j.info.repository.as_deref().unwrap_or("?")))
        .unwrap_or_default();
    format!(
        "{:<20} {:<16} version={} labels={}{}{}",
        r.name.as_str(),
        format!("{:?}", r.status),
        r.current_version.as_ref().map_or("?".into(), ToString::to_string),
        r.labels.join(","),
        job,
        r.problem.as_ref().map(|p| format!(" problem=\"{p}\"")).unwrap_or_default()
    )
}

/// Prints the service's status; fails when it is not running.
pub fn print(socket_dir: &Path) -> Result<(), Error> {
    let mut s = connect(socket_dir)?;
    let frame = |e: pitcrew_core::protocol::FrameError| Error::Os {
        what: "Talking to the Pitcrew service",
        reason: e.to_string(),
    };
    write_frame(
        &mut s,
        &Request::Hello {
            protocol: PROTOCOL_VERSION,
        },
    )
    .map_err(frame)?;
    match read_frame::<Response>(&mut s).map_err(frame)? {
        Response::Hello { service_version, .. } => println!("Pitcrew service {service_version} is running"),
        Response::Error(e) => {
            return Err(Error::Os {
                what: "Talking to the Pitcrew service",
                reason: e.message,
            })
        }
        _ => {}
    }
    write_frame(&mut s, &Request::Status).map_err(frame)?;
    if let Response::Status { runners, service } = read_frame::<Response>(&mut s).map_err(frame)? {
        println!("data folder: {}", service.data_dir);
        for p in &service.config_problems {
            println!("configuration problem: {p}");
        }
        if runners.is_empty() {
            println!("no runners yet: register one in the Pitcrew app");
        }
        for r in &runners {
            println!("{}", line(r));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn a_stopped_service_is_an_error() {
        let d = std::env::temp_dir().join(format!("pitcrew-it-status-{}", std::process::id()));
        if cfg!(unix) {
            assert!(print(&d).is_err());
        }
    }
}
