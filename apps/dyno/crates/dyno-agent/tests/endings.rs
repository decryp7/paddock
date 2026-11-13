#![allow(clippy::unwrap_used, clippy::expect_used)]

//! How the agent's endpoint ends a connection, driven through the scenarios of the ending harness (ADR 0072 WP-0): the raw
//! frames and the exact end of the stream a client sees. What fails today is recorded per instance in
//! `docs/ipc-endpoint-ratchet.toml`; the test is red without it. The agent's own log is printed if it fails.

use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_platform::fake::Fake;
use paddock_ipc::testing::endings::applicability::Applicability;
use paddock_ipc::testing::endings::ipc::{self, IpcTarget};
use paddock_ipc::testing::endings::ratchet::Ratchet;
use paddock_ipc::testing::endings::{failure_text, run_all, Expect, Observed, Scenario};

#[path = "common/agent_log.rs"]
mod agent_log;

fn scenario(s: Scenario) -> Result<(Observed, Expect), &'static str> {
    let applies = matches!(
        s,
        Scenario::WrongToken
            | Scenario::WrongVersion
            | Scenario::MalformedFirstFrame
            | Scenario::ClientKeepsWriting
            | Scenario::StopWithOpenConnection
    );
    if !applies {
        return Err("this scenario is not driven by the agent rig; if it does not apply, declare it in [[not_applicable]]");
    }
    let tmp = paddock_testkit::private_tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let fake = Fake::new();
    let from = agent_log::lines().len();
    let running = Running::start(dirs, fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
    let target = IpcTarget {
        endpoint: running.discovery.endpoint.clone(),
        token: running.discovery.token().expect("a token"),
        limit: dyno_agent::wire::limit(),
        protocol: dyno_agent::wire::protocol(),
    };
    let result = if s == Scenario::StopWithOpenConnection {
        ipc::stop_with_open_connection(&target, move || running.stop())
    } else {
        let r = ipc::black_box(&target, s).expect("covered");
        running.stop();
        r
    };
    result.map(|(mut o, e)| {
        o.log = agent_log::lines().into_iter().skip(from).collect();
        (o, e)
    })
}

/// The ending test of the register's `dyno-agent` entry.
#[test]
fn dyno_agent_ends_every_connection_right_or_is_in_the_ratchet() {
    let _log = agent_log::on_failure();
    let applicability = Applicability::load_from(env!("CARGO_MANIFEST_DIR")).expect("the register file");
    let reports = run_all("dyno-agent", &applicability, scenario);
    let ratchet = Ratchet::load_from(env!("CARGO_MANIFEST_DIR")).expect("the ratchet file");
    if let Err(e) = ratchet.check("dyno-agent", &reports) {
        panic!("{e}\n\n{}", failure_text(&reports));
    }
}
