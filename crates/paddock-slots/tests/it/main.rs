//! Slots over real lock files in scratch folders (no shared state outside them).
mod holder;
mod hostile;
mod races;

use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Held shared by every test that locks a slot and exclusively by the test that spawns a child process: a lock file
/// open in this process at the moment of the fork would be inherited by the child until it execs, and the lock then
/// looks held after its guard is dropped.
static SPAWN_GATE: RwLock<()> = RwLock::new(());

fn share_gate() -> RwLockReadGuard<'static, ()> {
    SPAWN_GATE.read().unwrap_or_else(|e| e.into_inner())
}

fn exclusive_gate() -> RwLockWriteGuard<'static, ()> {
    SPAWN_GATE.write().unwrap_or_else(|e| e.into_inner())
}
