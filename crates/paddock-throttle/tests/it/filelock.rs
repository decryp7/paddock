//! The slot locks are kernel file locks held by handle (user decision, 2026-11-26): what that promises, each with a test.
//! 1. a lock folder on a network file system is refused; 2. the throttle is taken on the host around a container, never inside one;
//! 3. the lock is not inherited by a child, and a holder that dies frees its slot at once; 4. a nested run uses its parent's slot.

use crate::helpers::*;
use paddock_throttle::exit;
use paddock_throttle::fsinfo::FsKind;
use paddock_throttle::runner::{RunOpts, run};
use std::sync::Arc;

fn opts() -> RunOpts {
    RunOpts {
        gate: false,
        low: false,
        queue_max: None,
        queue_stall: None,
        run_max: None,
        stall: None,
    }
}

// (1) a network lock folder: refused by a run before anything is queued or run, with the code of "the lock could not be taken".
#[test]
fn a_run_with_its_lock_folder_on_a_network_file_system_is_refused_and_the_command_never_runs() {
    let rig = Rig::new();
    let mut a = rig.agent(11);
    a.h.fs_kind = Arc::new(|_| FsKind::Network("nfs".into()));
    let ran = rig.root().join("ran");
    let args = vec![
        "sh".to_string(),
        "-c".to_string(),
        format!("touch {}", ran.display()),
    ];
    let code = run(&a.h, req("N"), &args, opts(), &|| None);
    assert_eq!(code, exit::LOCK_NOT_TAKEN);
    assert!(a.said().contains("network file system"), "{}", a.said());
    assert!(!ran.exists(), "the command never ran");
    assert_eq!(rig.count("pitcrew-heavy.ticket."), 0);
}

// (2) a container run runs alone, on the host: throttle recognises the command as one (`gate::is_container_command`). The check of
// `tools/linux-container.sh` went with the script (no VM or container starts on the Mac, docs/rules.md rule 101).
#[test]
fn a_container_command_is_recognised_so_it_runs_alone_on_the_host() {
    for cmd in [
        "bash tools/linux-container.sh cargo test",
        "docker run --rm x",
        "podman run x",
        "nerdctl run x",
    ] {
        assert!(
            paddock_throttle::gate::is_container_command(cmd),
            "{cmd}: a container run runs alone, on the host"
        );
    }
}
