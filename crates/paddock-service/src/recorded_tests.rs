//! The parsers and backends read the output of the real tools, recorded on the real systems (ADR 0054 invariant 3: a scripted
//! answer is a recording, not a hand-written guess). Each file in `tests/recorded/` starts with `#!` lines that say where,
//! when and with which command it was recorded; the rest is the tool's raw output.

use crate::backend::{Backend, Job, LastExit, Systemd, Target};
use crate::port::Output;
use crate::spec::{Kind, ServiceSpec};
use crate::testing::FakeSystem;

/// The recorded output `name` without its `#!` header.
fn recorded(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/recorded")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(
        text.starts_with("#! recorded "),
        "{name} says where it was recorded"
    );
    text.lines()
        .filter(|l| !l.starts_with("#!"))
        .map(|l| format!("{l}\n"))
        .collect()
}

fn systemd(sys: &FakeSystem) -> Systemd {
    let spec = ServiceSpec::builder("net.decryptology.dyno.helper", "DynoHelper", "dyno-helper")
        .unit("paddock-fixture")
        .kind(Kind::SystemService)
        .build()
        .unwrap();
    sys.put_program("/usr/bin/dyno-helper");
    let b = Systemd::new(
        spec,
        "/etc/systemd/system/paddock-fixture.service".into(),
        Target::exe("/usr/bin/dyno-helper"),
        Box::new(sys.clone()),
    );
    sys.put_file(
        "/etc/systemd/system/paddock-fixture.service",
        &b.definition(),
    );
    b
}

#[test]
fn systemd_255_a_unit_stopped_by_systemctl_stop_is_not_running_with_no_last_exit() {
    let sys = FakeSystem::new();
    let mut b = systemd(&sys);
    sys.answer("systemctl is-active", Output::ok("inactive\n"));
    sys.answer("systemctl is-enabled", Output::ok("static\n"));
    sys.answer(
        "systemctl show",
        Output::ok(&recorded("systemd-255-stopped.txt")),
    );
    let look = b.inspect();
    assert_eq!(
        (look.job, look.last_exit, look.restarts),
        (Job::NotRunning, None, Some(0))
    );
}

#[test]
fn systemd_255_a_unit_killed_from_outside_is_failed_with_the_signal() {
    let sys = FakeSystem::new();
    let mut b = systemd(&sys);
    sys.answer("systemctl is-active", Output::ok("failed\n"));
    sys.answer("systemctl is-enabled", Output::ok("static\n"));
    sys.answer(
        "systemctl show",
        Output::ok(&recorded("systemd-255-killed.txt")),
    );
    let look = b.inspect();
    assert_eq!(look.job, Job::Failed(LastExit::Signal(9)));
    assert_eq!(
        look.unit.unwrap().origin,
        crate::backend::UnitOrigin::Runtime
    );
}

#[test]
fn systemd_255_a_running_unit_has_no_exit_yet() {
    let sys = FakeSystem::new();
    let mut b = systemd(&sys);
    sys.answer("systemctl is-active", Output::ok("active\n"));
    sys.answer("systemctl is-enabled", Output::ok("static\n"));
    sys.answer(
        "systemctl show",
        Output::ok(&recorded("systemd-255-running.txt")),
    );
    let look = b.inspect();
    assert_eq!((look.job, look.last_exit), (Job::Running, None));
}
