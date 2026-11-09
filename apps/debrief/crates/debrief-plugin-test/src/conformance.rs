//! The conformance checks: what Debrief relies on from every plugin, run against the real binary under the sandbox it
//! gets in Debrief.
//!
//! | Check | What passes |
//! |---|---|
//! | `manifest` | the manifest is valid for this Debrief (protocol, ids, contributions), every permission has a reason, and a third-party plugin asks only for `network` and `runPrograms` |
//! | `permissions` | the plugin runs with exactly what it declares: every check below runs in that sandbox, so an undeclared right (a file outside its folders, the network, another program) fails there |
//! | `typed-errors` | an unknown importer, tool or transform and a request that does not decode are answered with a typed error, and the plugin keeps serving |
//! | `bounded-output` | every importer streams items of at most `MAX_EVENTS_PER_ITEM` events, and its records have bounded fields |
//! | `cancellation` | an import that is cancelled ends within the cancel grace period (`debrief_ipc::peer::CANCEL_GRACE`) and the plugin keeps serving |
//! | `oversized-frames` | on a connection with a small frame limit (64 KiB), an answer that does not fit is a typed error, not a hang or a crash |
//!
//! Panics: a panic in a plugin's code is answered as a typed `PANIC` error and the process keeps serving. That is
//! the SDK's behaviour for every plugin; the harness's own tests check it with a fixture that panics on request.

use crate::{phase, Launch, PluginUnderTest};
use debrief_ipc::peer::{code, CANCEL_GRACE};
use debrief_ipc::{IpcError, StreamEvent};
use debrief_plugin_api::protocol::{FileInfo, HostFileRef, PluginRequest, MAX_EVENTS_PER_ITEM};
use debrief_plugin_package::manifest::{valid_id, PACKAGE_PERMISSIONS};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The outcome of one check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// The check's name (`manifest`, `typed-errors`, ...).
    pub name: &'static str,
    /// Whether it passed.
    pub passed: bool,
    /// What was found (why it failed, or what was checked).
    pub detail: String,
}

/// The outcome of all checks.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Every check, in order.
    pub checks: Vec<Check>,
}

impl Report {
    /// Whether every check passed.
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }

    /// The failed checks.
    pub fn failures(&self) -> Vec<&Check> {
        self.checks.iter().filter(|c| !c.passed).collect()
    }

    /// Panics with every failure (for tests).
    #[track_caller]
    pub fn assert_passed(&self) {
        assert!(self.passed(), "conformance checks failed:\n{self}");
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for c in &self.checks {
            writeln!(f, "{} {}: {}", if c.passed { "ok  " } else { "FAIL" }, c.name, c.detail)?;
        }
        Ok(())
    }
}

/// Runs one check, writing its start and end to stderr and adding its duration to the detail (ER-27).
fn timed(name: &'static str, f: impl FnOnce() -> Result<String, String>) -> Check {
    phase(format_args!("check {name}: start"));
    let began = Instant::now();
    let r = f();
    let ms = began.elapsed().as_millis();
    let c = check(name, r.map(|d| format!("{d} ({ms} ms)")).map_err(|d| format!("{d} ({ms} ms)")));
    phase(format_args!(
        "check {name}: {} after {ms} ms",
        if c.passed { "passed" } else { "FAILED" }
    ));
    c
}

fn check(name: &'static str, r: Result<String, String>) -> Check {
    match r {
        Ok(detail) => Check {
            name,
            passed: true,
            detail,
        },
        Err(detail) => Check {
            name,
            passed: false,
            detail,
        },
    }
}

/// Synthetic text for importers when the plugin gives no samples: plain lines with a timestamp each.
fn synthetic(lines: usize) -> Vec<u8> {
    let mut s = String::new();
    for i in 0..lines {
        s.push_str(&format!(
            "2026-10-21T10:{:02}:{:02}Z INFO line {i} key=value\n",
            (i / 60) % 60,
            i % 60
        ));
    }
    s.into_bytes()
}

/// The inputs the import checks run over: the samples, or a synthetic log when there are none.
fn inputs(samples: &[PathBuf]) -> Vec<(String, Vec<u8>)> {
    if samples.is_empty() {
        vec![("synthetic.log".into(), synthetic(2000))]
    } else {
        samples
            .iter()
            .filter_map(|s| {
                std::fs::read(s)
                    .ok()
                    .map(|b| (s.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), b))
            })
            .collect()
    }
}

/// Runs every check against `p` (see the module docs); `samples` are files its importers should read.
pub fn run(p: &PluginUnderTest, samples: &[PathBuf]) -> Report {
    let mut r = Report::default();
    r.checks.push(timed("manifest", || manifest(p)));
    r.checks.push(timed("permissions", || {
        Ok(format!(
            "ran with {:?} ({})",
            p.granted(),
            if p.launch_options().third_party {
                "contained as a third-party plugin"
            } else {
                "as Debrief's own plugin"
            }
        ))
    }));
    r.checks.push(timed("typed-errors", || typed_errors(p)));
    let inputs = inputs(samples);
    r.checks.push(timed("bounded-output", || bounded(p, &inputs)));
    r.checks.push(timed("cancellation", || cancellation(p, &inputs)));
    r.checks.push(timed("oversized-frames", || oversized(p, &inputs)));
    r
}

fn manifest(p: &PluginUnderTest) -> Result<String, String> {
    let m = p.manifest();
    m.check()?;
    if !valid_id(&m.id) {
        return Err(format!("id \"{}\" must be reverse-DNS style (com.example.name)", m.id));
    }
    if semver_ok(&m.version).is_none() {
        return Err(format!("version \"{}\" is not semver", m.version));
    }
    if let Some(r) = m.permissions.iter().find(|r| r.reason.trim().is_empty()) {
        return Err(format!("permission {:?} needs a reason the user can read", r.permission));
    }
    if p.launch_options().third_party {
        if let Some(r) = m.permissions.iter().find(|r| !PACKAGE_PERMISSIONS.contains(&r.permission)) {
            return Err(format!(
                "permission {:?} is only available to Debrief's own plugins; read files through the host instead",
                r.permission
            ));
        }
    }
    Ok(format!("{} {} ({} contributions)", m.id, m.version, m.contributions().len()))
}

fn semver_ok(v: &str) -> Option<()> {
    let core = v.split(['-', '+']).next()?;
    let parts: Vec<&str> = core.split('.').collect();
    (parts.len() == 3 && parts.iter().all(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit()))).then_some(())
}

fn expect_typed(what: &str, r: Result<Vec<u8>, IpcError>) -> Result<(), String> {
    match r {
        Err(IpcError::Remote(e)) if e.code == code::INVALID || e.code == code::UNIMPLEMENTED => Ok(()),
        Err(IpcError::Remote(e)) => Err(format!(
            "{what}: answered with error code {} ({}), expected INVALID",
            e.code, e.message
        )),
        Err(e) => Err(format!("{what}: the plugin failed instead of answering with an error: {e}")),
        Ok(_) => Err(format!("{what}: the plugin accepted it")),
    }
}

fn typed_errors(p: &PluginUnderTest) -> Result<String, String> {
    let enc = |req: &PluginRequest| debrief_ipc::wire::encode(req).map_err(|e| e.to_string());
    let file = FileInfo {
        name: "x.log".into(),
        path: "x.log".into(),
        date: None,
        timestamps: p.launch_options().timestamps.clone(),
    };
    expect_typed(
        "an unknown importer",
        p.request_raw(enc(&PluginRequest::Sniff {
            importer: "debrief-plugin-test.no-such-importer".into(),
            file,
            head: b"x".to_vec(),
        })?),
    )?;
    expect_typed(
        "an unknown transform",
        p.request_raw(enc(&PluginRequest::Transform {
            transform: "debrief-plugin-test.no-such-transform".into(),
            source_name: "x".into(),
            rows: Vec::new(),
        })?),
    )?;
    expect_typed(
        "an unknown tool",
        p.request_raw(enc(&PluginRequest::InvokeTool {
            tool: "debrief-plugin-test.no-such-tool".into(),
            op: "go".into(),
            args: debrief_ipc::Json(serde_json::json!({})),
            data_dir: None,
        })?),
    )?;
    expect_typed("a request that does not decode", p.request_raw(vec![0xff; 16]))?;
    if !p.alive() {
        return Err("the plugin stopped after a bad request".into());
    }
    Ok("unknown contributions and malformed requests get typed errors".into())
}

fn bounded(p: &PluginUnderTest, inputs: &[(String, Vec<u8>)]) -> Result<String, String> {
    let mut checked = 0;
    for i in &p.manifest().importers {
        for (name, bytes) in inputs {
            if !i.binary && debrief_plugin_api::text::decode(bytes).is_none() {
                continue;
            }
            let out = p
                .import_within(&i.id, name, bytes, p.launch_options().import_timeout)
                .map_err(|e| format!("importer \"{}\" on {name}: {e:#}", i.id))?;
            if let Some(n) = out.items.iter().find(|n| **n > MAX_EVENTS_PER_ITEM) {
                return Err(format!(
                    "importer \"{}\" sent an item of {n} events (at most {MAX_EVENTS_PER_ITEM})",
                    i.id
                ));
            }
            if let Some(r) = out
                .records
                .iter()
                .find(|r| r.message.len() > debrief_plugin_api::text::MAX_LINE_LEN)
            {
                return Err(format!(
                    "importer \"{}\" produced a message of {} bytes on line {}",
                    i.id,
                    r.message.len(),
                    r.line
                ));
            }
            checked += 1;
        }
    }
    Ok(format!("{checked} imports streamed bounded items"))
}

fn cancellation(p: &PluginUnderTest, inputs: &[(String, Vec<u8>)]) -> Result<String, String> {
    let Some(importer) = p.manifest().importers.first() else {
        return Ok("no importers".into());
    };
    // A large input: the samples repeated to about 16 MiB.
    let (name, sample) = inputs.first().cloned().unwrap_or_else(|| ("synthetic.log".into(), synthetic(2000)));
    let mut big = Vec::new();
    while big.len() < 16 << 20 && !sample.is_empty() {
        big.extend_from_slice(&sample);
        if !sample.ends_with(b"\n") && !importer.binary {
            big.push(b'\n');
        }
    }
    let content = if importer.binary {
        big
    } else {
        debrief_plugin_api::text::decode(&big).map(String::into_bytes).unwrap_or(big)
    };
    phase(format_args!(
        "cancellation: lending {} bytes to importer \"{}\"",
        content.len(),
        importer.id
    ));
    let id = p.host.lend_file(content.clone());
    let req = PluginRequest::Import {
        importer: importer.id.clone(),
        file: FileInfo {
            name,
            path: "big.log".into(),
            date: None,
            timestamps: p.launch_options().timestamps.clone(),
        },
        content: HostFileRef {
            id,
            len: content.len() as u64,
        },
    };
    let result = (|| -> Result<String, String> {
        let mut stream = p.child.peer().stream_request(&req, 1).map_err(|e| e.to_string())?;
        let poll = || match stream.next(Some(Duration::from_millis(1))) {
            Ok(StreamEvent::Done(_)) => Some(Ok("the import ended before it could be cancelled".to_string())),
            Ok(_) => None,
            Err(e) => Some(Err(format!(
                "importer \"{}\" ended with an error while its file was being read ({} of {} bytes): {e}",
                importer.id,
                p.host.read_of(id),
                content.len()
            ))),
        };
        let waiting = Instant::now();
        if let Some(early) = wait_read_whole(&p.host, id, content.len() as u64, &importer.id, p.launch_options().read_stall, poll)? {
            return Ok(early);
        }
        phase(format_args!(
            "cancellation: whole file read after {} ms, sending the cancel",
            waiting.elapsed().as_millis()
        ));
        let started = Instant::now();
        stream.cancel();
        let outcome = |what: &str| {
            phase(format_args!(
                "cancellation: {what} {} ms after the cancel",
                started.elapsed().as_millis()
            ))
        };
        loop {
            match stream.next(Some(Duration::from_millis(100))) {
                Ok(StreamEvent::Done(_)) => {
                    outcome("import done");
                    break;
                }
                Ok(_) => {}
                Err(IpcError::Remote(e)) if e.code == code::CANCELLED => {
                    outcome("import ended as cancelled");
                    break;
                }
                Err(IpcError::Remote(e)) => {
                    outcome(&format!("import failed with {}", e.code));
                    break;
                }
                Err(e) => return Err(format!("the import broke instead of ending: {e}")),
            }
            if started.elapsed() > CANCEL_GRACE + Duration::from_secs(1) {
                outcome("still running");
                return Err(format!(
                    "importer \"{}\" kept running {} s after it was cancelled (Debrief allows {} s)",
                    importer.id,
                    started.elapsed().as_secs(),
                    CANCEL_GRACE.as_secs()
                ));
            }
        }
        Ok(format!("ended {} ms after the cancel", started.elapsed().as_millis()))
    })();
    p.host.take_back(id);
    let detail = result?;
    if !p.alive() {
        return Err("the plugin stopped after a cancelled import".into());
    }
    Ok(detail)
}

/// Waits until the plugin has read the whole of lent file `id` (`len` bytes), the host's signal that the importer has
/// it all (a loaded machine reads it late, and cancelling before the importer has it all ends the read, not the
/// importer's own work). `poll` looks at the import's stream: `Some(Ok(how))` when it ended, `Some(Err(why))` when it
/// broke (the plugin died: the cause is the check's, not a stall's).
/// `Ok(None)`: read whole; `Ok(Some(why))`: the import ended before it could be cancelled; `Err`: it broke, or its
/// read made no progress for `stall` (the bytes read so far are in the message), so the check ends with a cause long
/// before a test runner's kill; a slow read that keeps advancing is waited for. The 20 ms slice only bounds how long
/// to wait before looking at the stream again.
fn wait_read_whole(
    host: &crate::host::FakeHost,
    id: u64,
    len: u64,
    importer: &str,
    stall: Duration,
    mut poll: impl FnMut() -> Option<Result<String, String>>,
) -> Result<Option<String>, String> {
    // Counted by file, not as a difference of the host's total: the plugin may read before this wait begins (it gets
    // the request first), and on a loaded machine it did, so a baseline taken here missed a chunk and waited for it.
    let mut seen = host.read_of(id);
    phase(format_args!("cancellation: {seen} of {len} bytes already read when the wait began"));
    let mut moved = Instant::now();
    let mut reported = Instant::now();
    let waiting = Instant::now();
    while !host.wait_read_of(id, len, Duration::from_millis(20)) {
        match poll() {
            Some(Ok(how)) => {
                phase(format_args!("cancellation: {how}"));
                return Ok(Some(how));
            }
            Some(Err(why)) => {
                phase(format_args!("cancellation: {why}"));
                return Err(why);
            }
            None => {}
        }
        let now = host.read_of(id);
        if now != seen {
            seen = now;
            moved = Instant::now();
        }
        if reported.elapsed() >= Duration::from_secs(2) {
            reported = Instant::now();
            phase(format_args!(
                "cancellation: read {seen} of {len} bytes after {} ms",
                waiting.elapsed().as_millis()
            ));
        }
        if moved.elapsed() >= stall {
            phase(format_args!("cancellation: read stalled at {seen} of {len} bytes"));
            return Err(format!(
                "importer \"{importer}\" read {seen} of {len} bytes and then made no progress for {} s (hung)",
                stall.as_millis() as f64 / 1000.0
            ));
        }
    }
    Ok(None)
}

/// The frame limit of the `oversized-frames` check.
const SMALL_FRAME: usize = 64 * 1024;

fn oversized(p: &PluginUnderTest, inputs: &[(String, Vec<u8>)]) -> Result<String, String> {
    let Some(importer) = p.manifest().importers.first() else {
        return Ok("no importers".into());
    };
    let small = p
        .sibling(Launch {
            max_frame: SMALL_FRAME,
            granted: Some(p.granted().to_vec()),
            ..p.launch_options().clone()
        })
        .map_err(|e| format!("cannot start a second instance: {e:#}"))?;
    let (name, sample) = inputs.first().cloned().unwrap_or_else(|| ("synthetic.log".into(), synthetic(2000)));
    let mut big = sample.clone();
    while big.len() < 2 << 20 && !sample.is_empty() {
        big.extend_from_slice(&sample);
    }
    // Its reads and answer cannot fit in 64 KiB frames: a typed error (or an import that produced nothing) is fine, a crash or
    // a hang is not. The request itself is small: the content is read back through the host in chunks.
    let r = small.import_within(&importer.id, &name, &big, p.launch_options().import_timeout);
    // Hung is what the call says (it did not answer), never a stopwatch reading taken around it.
    if let Err(e) = &r {
        if e.to_string().contains("did not answer") {
            return Err(format!("the import hung on a small frame limit: {e:#}"));
        }
    }
    if !small.alive() {
        return Err(format!("the plugin died when its answer did not fit a frame ({:?})", r.err()));
    }
    Ok(match r {
        Ok(out) => format!("{} records fit in 64 KiB frames", out.records.len()),
        Err(e) => format!("refused with a typed error: {e:#}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::FakeHost;
    use debrief_plugin_api::protocol::READ_CHUNK;

    fn host() -> FakeHost {
        let host = FakeHost::default();
        host.stall_reads_after(None);
        host.fail_reads_after(None);
        host
    }

    #[test]
    fn reads_served_before_the_wait_began_count_toward_the_whole_file() {
        // A loaded machine: the plugin reads the lent file (its first chunk, or all of it) before the check starts
        // waiting. Those bytes are this file's too, so the wait ends at once; it is never judged a stall.
        let host = host();
        let len = 3 * u64::from(READ_CHUNK) + 4;
        let id = host.lend_file(vec![b'x'; len as usize]);
        let mut offset = 0;
        while offset < len {
            offset += host.read_file(id, offset, READ_CHUNK).unwrap().len() as u64;
        }
        let got = wait_read_whole(&host, id, len, "fixture", Duration::from_millis(200), || None);
        assert_eq!(got, Ok(None));
    }

    #[test]
    fn reads_of_another_lent_file_do_not_count() {
        let host = host();
        let other = host.lend_file(vec![b'y'; 8]);
        let id = host.lend_file(vec![b'x'; 8]);
        assert_eq!(host.read_file(other, 0, 8).unwrap().len(), 8);
        let got = wait_read_whole(&host, id, 8, "fixture", Duration::from_millis(200), || None);
        let e = got.unwrap_err();
        assert!(e.contains("read 0 of 8 bytes") && e.contains("made no progress"), "{e}");
    }

    #[test]
    fn an_import_that_broke_ends_the_wait_with_its_cause() {
        let host = host();
        let id = host.lend_file(vec![b'x'; 8]);
        let got = wait_read_whole(&host, id, 8, "fixture", Duration::from_secs(3600), || {
            Some(Err("the plugin died".into()))
        });
        assert_eq!(got, Err("the plugin died".into()));
    }
}
