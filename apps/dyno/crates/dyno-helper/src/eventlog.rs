//! The Windows Event Log adapter (thin): reports a text under the source "Dyno helper" through the system's own
//! `eventcreate.exe` (the crate forbids `unsafe`, and `eventcreate` registers the source with its generic message table, the
//! same one the install registers). The text and the choice of what to report are [`crate::refusal`]'s and are tested there;
//! this file only starts the program and never panics or fails the service.

use crate::refusal::eventcreate_arguments;
use log::Level;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// A hung `eventcreate` is left behind after this.
const WAIT: Duration = Duration::from_secs(3);

/// Reports `text` as `level` under the helper's source. A failure is ignored: the Event Log is a courtesy to the person who
/// reads why the service stopped, never a reason for it to stop.
pub fn report(level: Level, text: &str) {
    let Some(program) = crate::refusal::eventcreate_program(&paddock_service::privileged::OsKnownFolders) else {
        return;
    };
    let Ok(mut child) = dyno_platform::process::command(program)
        .args(eventcreate_arguments(level, text))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let began = Instant::now();
    while began.elapsed() < WAIT {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
