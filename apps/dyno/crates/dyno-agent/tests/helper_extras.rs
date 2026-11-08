#![allow(clippy::unwrap_used, clippy::expect_used)]

//! What keeps a Windows computer awake end to end (helper-wiring plan 2.9): a real agent over the
//! fake platform whose helper is a real `Dispatcher` (`common/helper_rig.rs`), with the Windows tools refusing as they do for a
//! standard account. The same jobs the Sleep tab and "Check now" start give the owner the helper's lists, a standard user the counts
//! and who may see more, and a computer without the helper today's refusal with the way to turn the helper on.

use dyno_agent::cli::{connect, run_job};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::extras::sleep::SleepReport;
use dyno_core::helper::HelperHost;
use dyno_core::helper_extras::{Requester, WakeRequest};
use dyno_core::jobs::{JobOutput, JobSpec};
use dyno_core::ports::PlatformError;
use dyno_core::read_via::ReadVia;
use dyno_platform::fake::Fake;
use dyno_platform::runner::{Tool, ToolOutput, ToolRunner};
use std::sync::Arc;
use std::time::Duration;

#[path = "common/helper_rig.rs"]
mod helper_rig;
use helper_rig::{dispatcher, wiring, Caller, DispatcherTransport, Readers};

const WITHIN: Duration = Duration::from_secs(60);

/// Every Windows tool refuses, as for an account that is not an administrator.
struct Refusing;
impl ToolRunner for Refusing {
    fn run_tool(&self, _: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
        Err(PlatformError::AccessDenied("Access is denied.".into()))
    }
}

struct Rig {
    running: Option<Running>,
    dirs: Dirs,
    _tmp: tempfile::TempDir,
}

impl Rig {
    fn start(caller: Caller, up: bool) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        let readers = Readers {
            wake_requests: vec![
                WakeRequest {
                    category: "SYSTEM".into(),
                    requester: Requester::Process,
                    name: "\\Device\\HarddiskVolume3\\Tools\\player.exe".into(),
                    reason: Some("Playing video".into()),
                },
                WakeRequest {
                    category: "DISPLAY".into(),
                    requester: Requester::Driver,
                    name: "Display driver".into(),
                    reason: None,
                },
            ],
            ..Readers::default()
        };
        let transport = DispatcherTransport::new(dispatcher(HelperHost::Windows, readers, tmp.path()), caller);
        transport.up.store(up, std::sync::atomic::Ordering::SeqCst);
        let running = Running::start_with_helper(
            dirs.clone(),
            fake.platform(),
            Box::new(fake.clone()),
            Box::new(|_| Ok(())),
            wiring(transport, HelperHost::Windows),
        )
        .unwrap();
        let x = &running.shared.extras;
        x.use_os("windows");
        x.use_tools(Arc::new(Refusing));
        Rig {
            running: Some(running),
            dirs,
            _tmp: tmp,
        }
    }

    fn sleep(&self) -> SleepReport {
        let peer = connect(&self.dirs).unwrap().0;
        let JobOutput::Sleep(s) = run_job(&peer, JobSpec::SleepBlockers, WITHIN).unwrap() else {
            panic!("sleep")
        };
        *s
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

#[test]
fn the_owner_gets_the_requesters_from_the_helper() {
    let rig = Rig::start(Caller::Owner, true);
    let s = rig.sleep();
    assert_eq!(s.blockers_via, ReadVia::Helper, "{:?}", s.blockers_unavailable);
    let names: Vec<&str> = s.blockers.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, vec!["player.exe", "Display driver"]);
    assert!(s.blockers_unavailable.is_none());
}

#[test]
fn a_standard_user_gets_the_counts_and_is_told_who_sees_more() {
    let rig = Rig::start(Caller::Other, true);
    let s = rig.sleep();
    assert!(s.blockers.is_empty(), "no requester names reach a standard user: {:?}", s.blockers);
    let why = s.blockers_unavailable.clone().unwrap_or_default();
    assert!(why.contains("2 requests") && why.contains("administrator"), "{why}");
}

#[test]
fn without_the_helper_the_refusal_stays_and_the_source_asks_for_the_helper() {
    let rig = Rig::start(Caller::Owner, false);
    let s = rig.sleep();
    assert!(s.blockers.is_empty());
    assert!(s
        .blockers_unavailable
        .unwrap_or_default()
        .contains("Windows only lets an administrator list what keeps it awake"));
    assert!(matches!(s.blockers_via, ReadVia::NeedsHelper { .. }), "{:?}", s.blockers_via);
}
