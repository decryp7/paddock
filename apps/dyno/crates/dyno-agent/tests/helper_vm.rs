//! Drives the real helper on the machine it runs on: reads its state and, when `HELPER_ACTION` names one, runs that action as the
//! agent's job (turnOn, repair, update, turnOff), printing every phase. Only for the local test VMs (`PADDOCK_VM=1`, `--ignored`);
//! the OS's permission prompt is real, so someone (or the VM's GUI automation) answers it. Never in CI, never on a developer's
//! machine (ER-14).

use dyno_agent::helper_os;
use dyno_core::helper_wire::HelperAction;
use paddock_jobs::{EventSink, JobRegistry, JobSnapshot, SystemClock};
use std::sync::Arc;

struct Print;
impl EventSink for Print {
    fn emit(&self, s: &JobSnapshot) {
        println!("job {:?}", s.state);
    }
}

fn action() -> Option<HelperAction> {
    match std::env::var("HELPER_ACTION").ok()?.as_str() {
        "turnOn" => Some(HelperAction::TurnOn),
        "repair" => Some(HelperAction::Repair),
        "update" => Some(HelperAction::Update),
        "turnOff" => Some(HelperAction::TurnOff),
        other => panic!("HELPER_ACTION {other} is not turnOn, repair, update or turnOff"),
    }
}

#[test]
#[ignore = "drives the real helper and the OS's prompt: the test VMs only (PADDOCK_VM=1)"]
fn the_helper_state_and_one_action_on_this_machine() {
    if std::env::var("PADDOCK_VM").as_deref() != Ok("1") {
        eprintln!("skipped: PADDOCK_VM is not 1");
        return;
    }
    let centre = helper_os::centre();
    let before = centre.status();
    println!("state before: {:?}: {}", before.state, before.message);
    println!("detail before: {:?}", before.detail);
    if let Some(action) = action() {
        let registry = JobRegistry::with_clock(Arc::new(Print), Arc::new(SystemClock), 0);
        let ctx = registry.register("helper", "Helper", None);
        let out = ctx.run(|ctx| centre.run(ctx, action));
        match out {
            Ok(s) => println!("action {action:?} finished: {:?}: {}", s.state, s.message),
            Err(e) => println!("action {action:?} failed: {e:?}"),
        }
    }
    let after = centre.status();
    println!(
        "state after: {:?} version={:?} recent={}",
        after.state,
        after.version,
        after.recent.len()
    );
}
