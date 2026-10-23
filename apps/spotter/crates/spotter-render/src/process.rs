//! A supervised child process: its whole process tree is ended on drop, on timeout and when it exceeds a memory
//! cap, so nothing leaks. No `unsafe`: Unix signals the process group through `kill(1)`, Windows uses `taskkill`.

use std::collections::HashMap;
use std::process::{Child, Command};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// A child that is killed with its helpers when dropped.
pub struct Supervised {
    child: Option<Child>,
}

impl Supervised {
    /// Starts `cmd` in its own process group (Unix) so the whole browser tree can be ended at once.
    pub fn spawn(mut cmd: Command) -> std::io::Result<Supervised> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        Ok(Supervised { child: Some(cmd.spawn()?) })
    }

    /// The process id while it has not been killed.
    pub fn id(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    /// Whether the process still runs.
    pub fn is_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Asks the process to quit (SIGTERM on Unix; Windows has no polite equivalent without a window) and waits up to
    /// `grace` for it, then ends the tree. For browsers, where an abrupt kill during start-up can leave a copy of the
    /// app behind on macOS. Idempotent.
    pub fn terminate(&mut self, grace: std::time::Duration) {
        #[cfg(unix)]
        if let Some(id) = self.id().filter(|_| self.is_running()) {
            let _ = Command::new("kill")
                .args(["-TERM", &id.to_string()])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            let end = std::time::Instant::now() + grace;
            while self.is_running() && std::time::Instant::now() < end {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        #[cfg(not(unix))]
        let _ = grace;
        self.kill();
    }

    /// Ends the process tree now and reaps it. Idempotent.
    pub fn kill(&mut self) {
        let Some(mut c) = self.child.take() else { return };
        kill_tree(c.id());
        let _ = c.kill();
        let _ = c.wait();
    }
}

impl Drop for Supervised {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Ends `root` and everything below it (descendants are collected first, then the group or tree is killed).
pub fn kill_tree(root: u32) {
    let sys = snapshot();
    let table = table_of(&sys);
    let kids = descendants(&table, root);
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{root}")])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &root.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    // Helpers that left the group (setsid) are still ours by parentage.
    for pid in kids {
        if let Some(p) = sys.process(Pid::from_u32(pid)) {
            p.kill();
        }
    }
}

/// Parents and resident memory only. Reading every process's command line, environment and working directory
/// (`everything()`) walks each process's memory on Windows and takes long enough, once a second per browser and for
/// every kill, to starve a loaded runner while Chrome is still starting.
fn snapshot() -> System {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_memory());
    sys
}

/// [`snapshot`] plus command lines (only leak checks need them).
fn snapshot_with_cmd() -> System {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_memory().with_cmd(sysinfo::UpdateKind::Always),
    );
    sys
}

/// `pid -> (parent, resident bytes)`.
pub type Table = HashMap<u32, (Option<u32>, u64)>;

fn table_of(sys: &System) -> Table {
    sys.processes()
        .iter()
        .map(|(pid, p)| (pid.as_u32(), (p.parent().map(Pid::as_u32), p.memory())))
        .collect()
}

/// Every process below `root` (not including it).
pub fn descendants(table: &Table, root: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut frontier = vec![root];
    while let Some(p) = frontier.pop() {
        for (&pid, &(parent, _)) in table {
            if parent == Some(p) && pid != root && !out.contains(&pid) {
                out.push(pid);
                frontier.push(pid);
            }
        }
    }
    out
}

/// Resident memory of `root` and its descendants, in bytes.
pub fn tree_memory(table: &Table, root: u32) -> u64 {
    let own = table.get(&root).map_or(0, |e| e.1);
    own + descendants(table, root)
        .iter()
        .filter_map(|p| table.get(p))
        .map(|e| e.1)
        .sum::<u64>()
}

/// A live read of [`tree_memory`] for the process tree at `root`.
pub fn current_tree_memory(root: u32) -> u64 {
    tree_memory(&table_of(&snapshot()), root)
}

/// Process ids whose command line mentions `needle` (leak checks: a throwaway profile path is unique to a run).
pub fn processes_mentioning(needle: &str) -> Vec<u32> {
    let sys = snapshot_with_cmd();
    sys.processes()
        .iter()
        .filter(|(_, p)| p.cmd().iter().any(|a| a.to_string_lossy().contains(needle)))
        .map(|(pid, _)| pid.as_u32())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_math() {
        let mut t = Table::new();
        t.insert(1, (None, 10));
        t.insert(2, (Some(1), 20));
        t.insert(3, (Some(2), 30));
        t.insert(4, (Some(9), 400));
        let mut d = descendants(&t, 1);
        d.sort();
        assert_eq!(d, vec![2, 3]);
        assert_eq!(tree_memory(&t, 1), 60);
        assert_eq!(tree_memory(&t, 99), 0);
    }

    #[cfg(unix)]
    fn exists(pid: u32) -> bool {
        current_tree_memory(pid) > 0 && processes_alive(pid)
    }

    #[cfg(unix)]
    fn processes_alive(pid: u32) -> bool {
        let sys = snapshot();
        sys.process(Pid::from_u32(pid))
            .is_some_and(|p| !matches!(p.status(), sysinfo::ProcessStatus::Zombie))
    }

    #[cfg(unix)]
    #[test]
    fn drop_kills_the_child_and_its_group() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 61 & sleep 62; wait"]);
        let s = Supervised::spawn(cmd).unwrap();
        let pid = s.id().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(exists(pid));
        drop(s);
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!processes_alive(pid), "process leaked");
        assert!(processes_mentioning("sleep 61").is_empty() || !processes_mentioning("sleep 61").iter().any(|p| processes_alive(*p)));
    }

    #[cfg(unix)]
    #[test]
    fn terminate_ends_a_polite_process_without_waiting_for_the_grace_period() {
        let mut cmd = Command::new("sleep");
        cmd.arg("64");
        let mut s = Supervised::spawn(cmd).unwrap();
        let t = std::time::Instant::now();
        s.terminate(std::time::Duration::from_secs(10));
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "took {:?}", t.elapsed());
        assert!(!s.is_running());
        s.terminate(std::time::Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn terminate_kills_a_process_that_ignores_sigterm_after_the_grace_period() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "trap '' TERM; while :; do sleep 1; done"]);
        let mut s = Supervised::spawn(cmd).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        s.terminate(std::time::Duration::from_millis(400));
        assert!(!s.is_running());
    }

    #[cfg(unix)]
    #[test]
    fn kill_is_idempotent() {
        let mut cmd = Command::new("sleep");
        cmd.arg("63");
        let mut s = Supervised::spawn(cmd).unwrap();
        assert!(s.is_running());
        s.kill();
        s.kill();
        assert!(!s.is_running());
        assert_eq!(s.id(), None);
    }
}
