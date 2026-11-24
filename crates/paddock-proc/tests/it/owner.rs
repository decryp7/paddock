//! ADR 0112 WP-1: the owner trait and `own::Unit`. The membership rule is pure (`members` over rows), so most tests inject a
//! table; the last one reads the real process table once, with a child of its own as the member and a second child that
//! carries no marker as the positive control that must stay out.

use paddock_proc::own::{Owned, Seen as Row, Unit, Waited, Why, members};
use paddock_proc::{ProcessId, StartTime};
use std::time::Duration;

const SINCE: u64 = 1_000;

fn id(pid: u32, started: u64) -> ProcessId {
    ProcessId::new(pid, StartTime::from_micros(started))
}

fn row(pid: u32, started: u64) -> Row {
    Row {
        id: id(pid, started),
        parent: Some(500),
        group: Some(pid),
        same_user: true,
        zombie: false,
        carries: false,
        names: false,
    }
}

fn unit() -> Unit {
    Unit::new(StartTime::from_micros(SINCE), 42)
}

fn pids(table: &[Row], unit: &Unit) -> Vec<(u32, Why)> {
    members(table, unit)
        .into_iter()
        .map(|m| (m.id.pid(), m.why))
        .collect()
}

#[test]
fn a_process_that_only_names_a_folder_is_a_member_only_when_it_is_an_orphan_or_the_owners_child() {
    let mut u = unit();
    u.add_folder("/scratch/reaper-1");
    let mut named = row(7, 2_000);
    named.names = true;
    assert!(pids(std::slice::from_ref(&named), &u).is_empty());
    named.parent = Some(1);
    assert_eq!(pids(std::slice::from_ref(&named), &u), [(7, Why::Folder)]);
    named.parent = Some(42);
    assert_eq!(pids(&[named], &u), [(7, Why::Folder)]);
}

#[test]
fn nothing_older_than_the_unit_or_of_another_user_is_chosen_by_marker_group_or_folder() {
    let mut u = unit();
    u.add_group(id(9, 999));
    let mut old = row(9, 999);
    old.carries = true;
    old.names = true;
    old.parent = Some(1);
    assert!(pids(&[old], &u).is_empty(), "started before the unit");
    if cfg!(unix) {
        let mut other = row(9, 2_000);
        other.same_user = false;
        other.carries = true;
        assert!(pids(&[other], &u).is_empty(), "another user's process");
    }
}

#[test]
fn a_group_counts_while_its_leader_is_the_process_seen_or_no_process_holds_its_pid() {
    let mut u = unit();
    let leader = row(10, 2_000);
    let mut member = row(12, 2_200);
    member.group = Some(10);
    u.add_group(leader.id);
    let chosen = |table: &[Row]| -> Vec<u32> { pids(table, &u).iter().map(|m| m.0).collect() };
    assert_eq!(chosen(&[leader.clone(), member.clone()]), [10, 12]);
    // The leader is gone and nobody holds pid 10: the group keeps its id while it has members.
    assert_eq!(chosen(std::slice::from_ref(&member)), [12]);
    // Pid 10 is another process now: the group id may be someone else's.
    let mut other = row(10, 9_000);
    other.group = Some(10);
    assert!(chosen(&[other, member]).is_empty());
}

#[test]
fn zombies_the_owner_and_spared_identities_are_never_members_even_with_the_marker() {
    let u = {
        let mut u = unit();
        u.spare(id(8, 3_000));
        u
    };
    let mut zombie = row(7, 2_000);
    zombie.carries = true;
    zombie.zombie = true;
    let mut owner = row(42, 10);
    owner.carries = true;
    let mut spared = row(8, 3_000);
    spared.carries = true;
    let mut control = row(9, 3_100);
    control.carries = true;
    // The positive control: the same marker on an ordinary row does choose it, so the three above were really excluded.
    assert_eq!(
        pids(&[zombie, owner, spared, control], &u),
        [(9, Why::Marker)]
    );
}

#[test]
fn a_spared_identity_is_spared_only_while_its_pid_holds_the_same_start_time() {
    let mut u = unit();
    u.spare(id(8, 3_000));
    let mut reused = row(8, 4_000);
    reused.carries = true;
    assert_eq!(pids(&[reused], &u), [(8, Why::Marker)]);
}

#[test]
fn roots_descendants_groups_and_marker_carriers_are_chosen_once_each_with_the_first_reason() {
    let mut u = unit();
    let root = row(10, 2_000);
    let mut desc = row(11, 2_100);
    desc.group = Some(10);
    let mut grouped = row(12, 2_200);
    grouped.group = Some(10);
    let mut carrier = row(13, 2_300);
    carrier.carries = true;
    u.add_root(root.id);
    u.add_below([desc.id]);
    u.add_group(root.id);
    assert_eq!(
        pids(&[root, desc, grouped, carrier], &u),
        [
            (10, Why::Root),
            (11, Why::Below),
            (12, Why::Group),
            (13, Why::Marker)
        ]
    );
}

#[test]
fn a_row_with_no_fact_in_its_favour_is_not_a_member_of_a_unit_that_holds_everything_else() {
    // The control of the controls above: it has no root, group, marker or folder, so a full unit still leaves it out.
    let mut u = unit();
    u.add_root(id(10, 2_000));
    u.add_group(id(11, 2_000));
    u.add_folder("/scratch/reaper-1");
    assert!(pids(&[row(7, 2_000)], &u).is_empty());
}

struct Fake {
    running: bool,
    ended: Option<u32>,
}

impl Owned for Fake {
    type Ended = u32;
    fn wait(&mut self, _bound: Duration) -> Waited {
        if self.running {
            Waited::Bound
        } else {
            Waited::Gone
        }
    }
    fn result(&self) -> Option<&u32> {
        self.ended.as_ref()
    }
    fn end(&mut self) -> &u32 {
        self.running = false;
        self.ended.get_or_insert(1)
    }
}

#[test]
fn the_trait_reads_a_result_only_after_end_and_waits_end_at_the_bound() {
    let mut f = Fake {
        running: true,
        ended: None,
    };
    assert_eq!(f.wait(Duration::ZERO), Waited::Bound);
    assert!(f.result().is_none());
    assert_eq!(*f.end(), 1);
    assert_eq!(f.wait(Duration::ZERO), Waited::Gone);
    assert_eq!(f.result(), Some(&1));
}

#[cfg(unix)]
mod real_table {
    use super::*;
    use std::process::Stdio;

    const VAR: &str = "PADDOCK_OWNER_TEST_MARKER";

    const HOLD: &str = "PADDOCK_OWNER_TEST_HOLD";

    /// Not a test when run on its own: as a child of the test below (the HOLD variable set) it waits until its standard input
    /// closes, which happens when the parent ends or kills it. A program Apple ships hides its environment from `ps` on macOS
    /// (ADR 0112), so the member is this test binary, not `sleep`.
    #[test]
    fn hold_until_stdin_closes_when_started_as_a_child() {
        if std::env::var_os(HOLD).is_some() {
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        }
    }

    fn holder(marker: Option<&str>) -> std::process::Child {
        let exe = std::env::current_exe().expect("test binary path");
        let mut c = paddock_proc::command(exe);
        c.args([
            "--exact",
            "owner::real_table::hold_until_stdin_closes_when_started_as_a_child",
        ])
        .env(HOLD, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        if let Some(m) = marker {
            c.env(VAR, m);
        }
        c.spawn().expect("the test binary starts")
    }

    #[test]
    fn the_real_table_gives_the_marked_child_and_not_the_unmarked_one() {
        // Linux reads a start time from a boot time in whole seconds: one second of slack, as the Reaper takes.
        let since = StartTime::from_micros(
            paddock_proc::own::now()
                .as_micros()
                .saturating_sub(1_000_000),
        );
        let value = format!("{}-{}", std::process::id(), since.as_micros());
        let (mut mine, mut other) = (holder(Some(&value)), holder(None));
        let (mine_id, other_id) = (
            ProcessId::of(mine.id()).expect("identity"),
            ProcessId::of(other.id()).expect("identity"),
        );
        let mut u = Unit::new(since, std::process::id());
        u.set_marker(VAR, &value);
        // Between fork and exec the child still holds its parent's environment: wait on the condition (bounded), never a pause.
        let began = std::time::Instant::now();
        let found = loop {
            let found: Vec<ProcessId> = members(&u.look(), &u).iter().map(|m| m.id).collect();
            if found.contains(&mine_id) || began.elapsed() > Duration::from_secs(10) {
                break found;
            }
            std::thread::yield_now();
        };
        let _ = (mine.kill(), other.kill());
        let _ = (mine.wait(), other.wait());
        assert!(found.contains(&mine_id), "the marked child is a member");
        assert!(!found.contains(&other_id), "the unmarked child is not");
    }
}
