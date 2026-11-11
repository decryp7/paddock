//! Changing something in the Windows service needs an elevated administrator (the service runs as LocalSystem; see
//! `pitcrew_service::caller`). The app is not elevated, so when the service answers `PermissionDenied` the app runs *itself*
//! once with Windows' own permission prompt (`runas`, one prompt per change) in a mode that only forwards that one request:
//!
//! ```text
//! pitcrew.exe --forward-request "<request file>" <sha-256 of the request file> "<response file>"
//! ```
//!
//! The request travels in a file (a registration token must not be on a command line); the command line carries the file's
//! digest, which the elevated process checks before it sends anything, so a program that swaps the file while the prompt is
//! open cannot make an administrator's approval carry its own request. The response comes back in the response file; a
//! forged response can only mislead what the window shows, never change the service. This module is the pure part
//! (files, digest, arguments, the elevated side over a [`ServiceClient`]); the prompt itself is in `service_install`.
//!
//! **What this does not do.** A Windows permission prompt on the same desktop as the program that asked for it is *not a security
//! boundary*: a program running as the same user can click it, send it keystrokes or ask for a prompt of its own, and the user
//! may approve what they did not start. The protection is the *check on the service's side* (only an elevated administrator
//! changes what runs) and what the user sees in the prompt; this module only makes sure that a prompt the user did approve
//! carries exactly the request the app showed, to the SYSTEM service and to nobody else (`pipe_server`).

use crate::service_client::{ClientError, ServiceClient};
use pitcrew_core::checksum::Sha256Digest;
use pitcrew_core::protocol::{ErrorCode, Request, Response, ServiceError};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The command-line flag of the forwarding mode.
pub const FLAG: &str = "--forward-request";

/// The largest request or response file read (a whole configuration is far smaller).
const MAX_FILE: u64 = 4 * 1024 * 1024;

/// One request waiting for the elevated process.
#[derive(Debug)]
pub struct Handoff {
    dir: PathBuf,
    request_file: PathBuf,
    response_file: PathBuf,
    digest: Sha256Digest,
}

fn io_message(what: &str, path: &Path, e: &std::io::Error) -> String {
    format!("{what} {}: {e}", path.display())
}

/// Writes `request` into a new folder below `parent` (created for this call, never reused) and returns what the elevated side
/// needs.
///
/// # Errors
/// The folder or the file could not be created.
pub fn prepare(parent: &Path, request: &Request) -> Result<Handoff, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = parent.join(format!("forward-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| io_message("Creating", &dir, &e))?;
    let bytes = serde_json::to_vec(request).map_err(|e| format!("the request cannot be written: {e}"))?;
    let request_file = dir.join("request.json");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&request_file)
        .map_err(|e| io_message("Creating", &request_file, &e))?;
    f.write_all(&bytes).map_err(|e| io_message("Writing", &request_file, &e))?;
    Ok(Handoff {
        response_file: dir.join("response.json"),
        digest: Sha256Digest::of_bytes(&bytes),
        request_file,
        dir,
    })
}

/// The parameters of the elevated run.
pub fn parameters(h: &Handoff) -> String {
    format!(
        "{FLAG} \"{}\" {} \"{}\"",
        h.request_file.display(),
        h.digest,
        h.response_file.display()
    )
}

/// What the elevated process was asked to forward.
#[derive(Debug, PartialEq, Eq)]
pub struct Forward {
    request_file: PathBuf,
    digest: Sha256Digest,
    response_file: PathBuf,
}

/// The forwarding mode's arguments (after the program name); `None` when the app was started normally.
pub fn parse_args(args: &[String]) -> Option<Result<Forward, String>> {
    if args.first().map(String::as_str) != Some(FLAG) {
        return None;
    }
    let [_, request, digest, response] = args else {
        return Some(Err(format!("{FLAG} needs a request file, its SHA-256 and a response file")));
    };
    Some(Sha256Digest::parse_hex(digest).map_err(|e| e.to_string()).map(|digest| Forward {
        request_file: request.into(),
        digest,
        response_file: response.into(),
    }))
}

/// Whether `m` describes a link: a symbolic link, a junction or any other reparse point.
fn is_link(m: &std::fs::Metadata) -> bool {
    if m.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Opens a hand-off file for reading **without following a link**: the folder it is in and the file itself must not be a
/// link or reparse point (checked on the opened handle where the OS allows it), so the elevated process never reads what a link
/// planted by another program leads to.
fn open_no_link(path: &Path) -> Result<std::fs::File, String> {
    if let Some(parent) = path.parent() {
        let m = std::fs::symlink_metadata(parent).map_err(|e| io_message("Examining", parent, &e))?;
        if is_link(&m) {
            return Err(format!("{} is a link; the elevated request refuses it", parent.display()));
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT: open the link itself, not what it points to.
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(path).map_err(|e| io_message("Opening", path, &e))?;
    let m = file.metadata().map_err(|e| io_message("Examining", path, &e))?;
    let named = std::fs::symlink_metadata(path).map_err(|e| io_message("Examining", path, &e))?;
    if is_link(&m) || is_link(&named) {
        return Err(format!("{} is a link; the elevated request refuses it", path.display()));
    }
    Ok(file)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let f = open_no_link(path)?;
    let mut bytes = Vec::new();
    f.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io_message("Reading", path, &e))?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(format!("{} is larger than {MAX_FILE} bytes", path.display()));
    }
    Ok(bytes)
}

/// The elevated side: checks the request file against its digest, sends the request through `client`, writes the response (an
/// error answer included; so is a failure to reach the service) into the response file. `Err` only when nothing could be
/// written back; the exit code then tells the waiting app.
///
/// # Errors
/// The request file does not match its digest or cannot be read, or the response cannot be written.
pub fn serve(f: &Forward, client: &dyn ServiceClient) -> Result<(), String> {
    let bytes = read_bounded(&f.request_file)?;
    let actual = Sha256Digest::of_bytes(&bytes);
    if actual != f.digest {
        return Err(format!(
            "the request file changed after it was approved (expected {}, found {actual}); nothing was sent",
            f.digest
        ));
    }
    let request: Request = serde_json::from_slice(&bytes).map_err(|e| format!("the request file is not a request: {e}"))?;
    if matches!(request, Request::Hello { .. } | Request::Subscribe) {
        return Err("only a single request can be forwarded".into());
    }
    let response = match client.request(request) {
        Ok(r) => r,
        Err(ClientError::Service(e)) => Response::Error(e),
        Err(e) => Response::Error(ServiceError::new(ErrorCode::Internal, e.to_string())),
    };
    let text = serde_json::to_vec(&response).map_err(|e| format!("the response cannot be written: {e}"))?;
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&f.response_file)
        .map_err(|e| io_message("Creating", &f.response_file, &e))?;
    out.write_all(&text).map_err(|e| io_message("Writing", &f.response_file, &e))
}

/// The app's side after the elevated process ended: the response it wrote, and the folder removed.
///
/// # Errors
/// There is no (valid) response: the prompt was approved but the elevated run failed; the message says what to look at.
pub fn collect(h: Handoff) -> Result<Response, ClientError> {
    let result = read_bounded(&h.response_file)
        .map_err(|e| ClientError::Protocol(format!("the elevated request gave no answer ({e})")))
        .and_then(|b| {
            serde_json::from_slice::<Response>(&b)
                .map_err(|e| ClientError::Protocol(format!("the elevated request gave an unreadable answer ({e})")))
        });
    // Best effort: a folder that cannot be removed holds one request and its answer, in the user's own data folder.
    if let Err(e) = std::fs::remove_dir_all(&h.dir) {
        eprintln!("pitcrew: could not remove {}: {e}", h.dir.display());
    }
    result
}

/// Removes the folders of earlier hand-offs below `parent` that are older than `max_age` (a crashed app, a prompt nobody answered).
/// Only `forward-*` folders are touched; each holds one request, which may carry a registration token, and its answer. Returns how
/// many were removed.
pub fn sweep_stale(parent: &Path, now: std::time::SystemTime, max_age: std::time::Duration) -> usize {
    let Ok(list) = std::fs::read_dir(parent) else { return 0 };
    let mut removed = 0;
    for entry in list.flatten() {
        let is_handoff = entry.file_name().to_string_lossy().starts_with("forward-");
        let Ok(meta) = entry.metadata() else { continue };
        let old = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > max_age);
        // A link is never followed: only a real folder is removed, by its own name.
        if is_handoff && meta.is_dir() && !meta.file_type().is_symlink() && old && std::fs::remove_dir_all(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Runs the forwarding mode when the app was started for it; returns the exit code, or `None` for a normal start.
pub fn run_if_asked(args: &[String], client: &dyn ServiceClient) -> Option<i32> {
    let forward = parse_args(args)?;
    let outcome = forward.and_then(|f| serve(&f, client));
    Some(match outcome {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("pitcrew: {e}");
            1
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitcrew_core::names::RunnerName;
    use std::sync::Mutex;

    struct Fake {
        seen: Mutex<Vec<Request>>,
        answer: Result<Response, ClientError>,
    }

    impl ServiceClient for Fake {
        fn request(&self, r: Request) -> Result<Response, ClientError> {
            self.seen.lock().unwrap().push(r);
            self.answer.clone()
        }
    }

    fn fake(answer: Result<Response, ClientError>) -> Fake {
        Fake {
            seen: Mutex::default(),
            answer,
        }
    }

    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("pitcrew-it-elevated-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn drain() -> Request {
        Request::Drain {
            runner: RunnerName::parse("a").unwrap(),
        }
    }

    fn forward_of(h: &Handoff) -> Forward {
        let args: Vec<String> = [
            FLAG,
            &h.request_file.display().to_string(),
            &h.digest.to_string(),
            &h.response_file.display().to_string(),
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        parse_args(&args).unwrap().unwrap()
    }

    #[test]
    fn the_request_goes_through_a_file_and_the_answer_comes_back() {
        let root = scratch("roundtrip");
        let h = prepare(&root, &drain()).unwrap();
        assert!(parameters(&h).starts_with(FLAG) && parameters(&h).contains(&h.digest.to_string()));
        let client = fake(Ok(Response::Ok));
        serve(&forward_of(&h), &client).unwrap();
        assert_eq!(*client.seen.lock().unwrap(), [drain()]);
        assert_eq!(collect(h).unwrap(), Response::Ok);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0, "the handoff folder is removed");
    }

    #[test]
    fn a_request_file_changed_after_the_prompt_is_never_sent() {
        let root = scratch("swapped");
        let h = prepare(&root, &drain()).unwrap();
        let other = serde_json::to_vec(&Request::RemoveRunner {
            runner: RunnerName::parse("b").unwrap(),
        })
        .unwrap();
        std::fs::write(&h.request_file, other).unwrap();
        let client = fake(Ok(Response::Ok));
        let e = serve(&forward_of(&h), &client).unwrap_err();
        assert!(e.contains("changed after it was approved"), "{e}");
        assert!(client.seen.lock().unwrap().is_empty(), "nothing reached the service");
    }

    #[test]
    fn a_refusal_or_an_unreachable_service_is_written_back_for_the_window_to_show() {
        let root = scratch("errors");
        for (answer, expect) in [
            (
                Ok(Response::Error(ServiceError::new(ErrorCode::BadRequest, "no"))),
                ErrorCode::BadRequest,
            ),
            (Err(ClientError::Unavailable("not running".into())), ErrorCode::Internal),
        ] {
            let h = prepare(&root, &drain()).unwrap();
            serve(&forward_of(&h), &fake(answer)).unwrap();
            assert!(matches!(collect(h).unwrap(), Response::Error(e) if e.code == expect));
        }
    }

    #[test]
    fn a_connection_or_subscription_cannot_be_forwarded() {
        let root = scratch("kinds");
        let h = prepare(&root, &Request::Subscribe).unwrap();
        let client = fake(Ok(Response::Ok));
        assert!(serve(&forward_of(&h), &client).is_err());
        assert!(client.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn the_arguments_are_parsed_once_and_a_normal_start_is_not_a_forward() {
        assert!(parse_args(&[]).is_none());
        assert!(parse_args(&["--hidden".to_string()]).is_none());
        assert!(parse_args(&[FLAG.to_string()]).unwrap().is_err());
        assert!(parse_args(&[FLAG.into(), "a".into(), "not-a-digest".into(), "b".into()])
            .unwrap()
            .is_err());
        assert_eq!(run_if_asked(&["--hidden".to_string()], &fake(Ok(Response::Ok))), None);
        assert_eq!(run_if_asked(&[FLAG.to_string()], &fake(Ok(Response::Ok))), Some(1));
    }

    #[test]
    fn stale_handoff_folders_are_swept_and_nothing_else() {
        let root = scratch("sweep");
        let old = prepare(&root, &drain()).unwrap();
        std::fs::create_dir(root.join("keep-me")).unwrap();
        let now = std::time::SystemTime::now();
        let hour = std::time::Duration::from_secs(3600);
        assert_eq!(
            sweep_stale(&root, now, hour),
            0,
            "a fresh hand-off may still be waiting for its prompt"
        );
        assert!(old.dir.exists());
        assert_eq!(sweep_stale(&root, now + 2 * hour, hour), 1);
        assert!(!old.dir.exists() && root.join("keep-me").exists(), "only forward-* folders go");
        assert_eq!(sweep_stale(&root.join("missing"), now, hour), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_hand_off_file_or_folder_that_is_a_link_is_refused() {
        let root = scratch("links");
        let h = prepare(&root, &drain()).unwrap();
        let client = fake(Ok(Response::Ok));
        // The request file replaced by a link to another file with the very same bytes (so the digest matches).
        let real = root.join("elsewhere.json");
        std::fs::copy(&h.request_file, &real).unwrap();
        std::fs::remove_file(&h.request_file).unwrap();
        std::os::unix::fs::symlink(&real, &h.request_file).unwrap();
        let e = serve(&forward_of(&h), &client).unwrap_err();
        assert!(e.contains("is a link"), "{e}");
        assert!(client.seen.lock().unwrap().is_empty(), "nothing was sent");
        // The hand-off folder replaced by a link to a folder that holds the same file.
        let h2 = prepare(&root, &drain()).unwrap();
        let moved = root.join("moved");
        std::fs::rename(&h2.dir, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &h2.dir).unwrap();
        let e = serve(&forward_of(&h2), &client).unwrap_err();
        assert!(e.contains("is a link"), "{e}");
        assert!(client.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn a_missing_answer_is_an_error_not_a_guess() {
        let root = scratch("missing");
        let h = prepare(&root, &drain()).unwrap();
        assert!(matches!(collect(h), Err(ClientError::Protocol(m)) if m.contains("no answer")));
    }
}
