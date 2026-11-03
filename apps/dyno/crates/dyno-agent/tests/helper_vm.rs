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

/// Sends the reading requests (processes, counters, memory, sockets, extras) to the running helper and prints what came back as
/// counts and kinds, never contents. Only for the local test VMs.
#[test]
#[ignore = "reads through the real helper: the test VMs only (PADDOCK_VM=1)"]
fn the_helper_readers_on_this_machine() {
    use dyno_agent::helper_link::HelperTransport;
    use dyno_core::helper_requests::{CounterId, CounterQuery, ExtrasKind, ProcessQuery, RequestClass};
    use dyno_core::helper_wire::{HelperReply, HelperRequest};
    if std::env::var("PADDOCK_VM").as_deref() != Ok("1") {
        eprintln!("skipped: PADDOCK_VM is not 1");
        return;
    }
    let t = dyno_agent::helper_os::transport();
    let show = |name: &str, r: Result<HelperReply, dyno_agent::helper_link::TransportError>| match r {
        Ok(HelperReply::Processes(p)) => {
            let mut kinds = std::collections::BTreeMap::new();
            for e in &p.items {
                *kinds
                    .entry(format!("{:?}", e.owner).split('(').next().unwrap_or("").to_string())
                    .or_insert(0u32) += 1;
            }
            println!(
                "{name}: {} items, more={}, others={:?}, truncated={}, owners={kinds:?}",
                p.items.len(),
                p.next.is_some(),
                p.others,
                p.truncated
            );
        }
        Ok(HelperReply::CounterValues(a)) => {
            for x in a {
                println!("{name}: {:?} -> {:?}", x.id, x.value);
            }
        }
        Ok(HelperReply::Memory(m)) => println!("{name}: {m:?}"),
        Ok(HelperReply::SocketOwners(a)) => println!("{name}: {} sockets, others={}, truncated={}", a.owners.len(), a.others, a.truncated),
        Ok(HelperReply::Extras(e)) => println!("{name}: {e:?}"),
        Ok(other) => println!("{name}: unexpected {other:?}"),
        Err(e) => println!("{name}: error {e:?}"),
    };
    show(
        "processes",
        t.call(HelperRequest::Processes(ProcessQuery {
            limit: 1024,
            ..ProcessQuery::default()
        })),
    );
    let ids = vec![CounterId::Thermal, CounterId::CpuEnergy, CounterId::Fans];
    show(
        "counters",
        t.call(HelperRequest::Counters(CounterQuery {
            ids,
            class: RequestClass::User,
        })),
    );
    show("memory", t.call(HelperRequest::MemoryModules));
    show("sockets", t.call(HelperRequest::SocketOwners));
    show("wakeRequests", t.call(HelperRequest::Extras(ExtrasKind::WakeRequests)));
    show("backupVersions", t.call(HelperRequest::Extras(ExtrasKind::BackupVersions)));
}
