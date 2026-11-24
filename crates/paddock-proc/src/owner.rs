//! Start, hold, watch, end (ER-51, ADR 0112): the one small trait every owner of a started thing implements, and the pure rule
//! that says which processes of a table belong to one unit of work.
//!
//! - [`Owned`]: what an owner offers once it holds the handles of what it started: [`Owned::wait`] ends at its bound with an
//!   outcome (never blocks for ever, ER-32), [`Owned::result`] reads what ending reported, [`Owned::end`] ends everything through
//!   the held handles. A test's `Reaper`, the job ledger and the slot holder implement it.
//! - [`Unit`]: what an owner knows about its processes, held by identity ([`ProcessId`]: pid plus start time, rule 53): the roots
//!   it started, the descendants it saw, the groups it made by their leader's identity, a marker its children carry in their
//!   environment, folders it made, when it began, and identities it must spare.
//! - [`members`]: the pure rule over one read of the process table ([`Seen`] rows). It reads no process and signals none.
//!
//! A process is a member only if it started at or after the unit began (except roots and descendants the owner saw itself), runs
//! as this user, and is a root, a recorded descendant, a member of a live group of the unit, a carrier of its marker, or an
//! orphan (or a child of the owner) whose command line names a folder of the unit. Anything else is left alone, whatever its
//! command line says. A group counts only while its leader is the process seen leading it, or while no process holds the leader's
//! pid (a group keeps its id while it has members).

use crate::own::{Seen, look};
use crate::{ProcessId, StartTime};
use std::collections::BTreeSet;
use std::time::Duration;

/// How a [`Owned::wait`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waited {
    /// Everything the owner holds has ended.
    Gone,
    /// The bound passed first; something the owner holds still runs.
    Bound,
}

/// What an owner of started things offers (see the module). The owner holds the handles; nothing here finds a process again by
/// name or search.
pub trait Owned {
    /// What ending reports (for a process owner, which processes it had to end).
    type Ended;

    /// Waits until everything held has ended or `bound` passes, whichever is first.
    fn wait(&mut self, bound: Duration) -> Waited;

    /// What [`Owned::end`] reported, or `None` while it has not run.
    fn result(&self) -> Option<&Self::Ended>;

    /// Ends everything held, through the held handles, and reports it. Ending twice reports the first ending again.
    fn end(&mut self) -> &Self::Ended;
}

/// Why a process is a member of a [`Unit`]: the first reason that holds, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Why {
    /// The owner started it.
    Root,
    /// The owner saw it below a root while it ran.
    Below,
    /// It is in a process group the unit made or saw.
    Group,
    /// Its environment carries the unit's marker.
    Marker,
    /// An orphan, or a child of the owner, whose command line names a folder of the unit.
    Folder,
}

/// One process of a table that belongs to a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member {
    /// Its identity.
    pub id: ProcessId,
    /// Why it belongs.
    pub why: Why,
}

/// What an owner knows about the processes of one unit of work (see the module).
#[derive(Debug, Clone)]
pub struct Unit {
    since: StartTime,
    owner: u32,
    roots: Vec<ProcessId>,
    below: BTreeSet<ProcessId>,
    groups: BTreeSet<ProcessId>,
    spared: BTreeSet<ProcessId>,
    marker: Option<(String, String)>,
    folders: Vec<String>,
}

impl Unit {
    /// A unit that began at `since` and is held by the process `owner` (the caller itself: alive, so its pid is its identity;
    /// it is never a member).
    pub fn new(since: StartTime, owner: u32) -> Unit {
        Unit {
            since,
            owner,
            roots: Vec::new(),
            below: BTreeSet::new(),
            groups: BTreeSet::new(),
            spared: BTreeSet::new(),
            marker: None,
            folders: Vec::new(),
        }
    }

    /// Records a process the owner started.
    pub fn add_root(&mut self, id: ProcessId) {
        if !self.roots.contains(&id) {
            self.roots.push(id);
        }
    }

    /// Records processes the owner saw below a root.
    pub fn add_below(&mut self, ids: impl IntoIterator<Item = ProcessId>) {
        self.below.extend(ids);
    }

    /// Records a process group by the identity of its leader.
    pub fn add_group(&mut self, leader: ProcessId) {
        self.groups.insert(leader);
    }

    /// Records a folder the unit made; an orphan whose command line names it is a member.
    pub fn add_folder(&mut self, folder: &str) {
        self.folders.push(folder.to_owned());
    }

    /// Sets the `NAME=value` the unit's children carry in their environment.
    pub fn set_marker(&mut self, name: &str, value: &str) {
        self.marker = Some((name.to_owned(), value.to_owned()));
    }

    /// Never a member, whatever it carries (a slot holder that leaves its group by design).
    pub fn spare(&mut self, id: ProcessId) {
        self.spared.insert(id);
    }

    /// The identities of the recorded roots, descendants and groups, for an owner that waits on them.
    pub fn roots(&self) -> &[ProcessId] {
        &self.roots
    }

    /// The recorded descendants.
    pub fn below(&self) -> &BTreeSet<ProcessId> {
        &self.below
    }

    /// The recorded group leaders.
    pub fn groups(&self) -> &BTreeSet<ProcessId> {
        &self.groups
    }

    /// One read of the process table with this unit's marker and folders, the input of [`members`].
    pub fn look(&self) -> Vec<Seen> {
        let names: Vec<&str> = self.folders.iter().map(String::as_str).collect();
        look(
            self.marker.as_ref().map(|(k, v)| (k.as_str(), v.as_str())),
            &names,
        )
    }
}

/// The rows of `table` that belong to `unit`, each once, with the first reason that holds. Pure: no process is read or signalled.
pub fn members(table: &[Seen], unit: &Unit) -> Vec<Member> {
    let groups: BTreeSet<u32> = unit
        .groups
        .iter()
        .filter(
            |leader| match table.iter().find(|r| r.id.pid() == leader.pid()) {
                Some(r) => r.id == **leader,
                None => true,
            },
        )
        .map(|leader| leader.pid())
        .collect();
    let mut out = Vec::new();
    for row in table
        .iter()
        .filter(|r| !r.zombie && r.id.pid() != unit.owner && !unit.spared.contains(&r.id))
    {
        let since = row.id.started() >= unit.since;
        let ours = row.same_user || cfg!(windows);
        let orphan_or_owned = row.parent.is_some_and(|p| p == 1 || p == unit.owner);
        let why = if unit.roots.contains(&row.id) {
            Some(Why::Root)
        } else if unit.below.contains(&row.id) {
            Some(Why::Below)
        } else if !ours || !since {
            None
        } else if row.group.is_some_and(|g| groups.contains(&g)) {
            Some(Why::Group)
        } else if row.carries {
            Some(Why::Marker)
        } else if row.names && orphan_or_owned {
            Some(Why::Folder)
        } else {
            None
        };
        if let Some(why) = why {
            out.push(Member { id: row.id, why });
        }
    }
    out
}
