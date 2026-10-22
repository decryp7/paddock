//! Processes grouped by application: one "Google Chrome" with 40 processes, not 40 `chrome.exe` lines (ADR 0003).
//!
//! A process joins a group by, in order:
//! 1. its macOS app bundle (the outermost `*.app` in its path), so helper apps join their app;
//! 2. its parent's group, when its executable lives under the parent's install folder (VS Code's `rg`, a game's
//!    launcher children) and the parent is not a shell, launcher or system host;
//! 3. its executable path (case-insensitive on Windows paths);
//! 4. its name, when the path is unreadable.

use crate::process::{Pid, ProcessSample};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// One application with all of its processes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppGroup {
    /// Stable key (bundle, executable path or name, normalised).
    pub id: String,
    /// Friendly name, e.g. "Google Chrome".
    pub name: String,
    /// The main executable (the group's first or root process), when known.
    pub exe: Option<String>,
    /// Every process in the group; the root process first.
    pub pids: Vec<Pid>,
    /// Sum of the members' CPU, percent of the machine.
    pub cpu: f32,
    /// Sum of the members' resident memory in bytes.
    pub memory: u64,
    /// Sum of the members' disk throughput in bytes per second.
    pub disk_bps: f64,
}

/// Parents whose children are never pulled into their group: shells, launchers, service hosts.
const NEVER_ADOPT: &[&str] = &[
    "explorer.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "windowsterminal.exe",
    "wininit.exe",
    "services.exe",
    "svchost.exe",
    "launchd",
    "bash",
    "zsh",
    "sh",
    "fish",
    "terminal",
    "iterm2",
    "userinit.exe",
    "winlogon.exe",
    "taskhostw.exe",
    "sihost.exe",
    "steam.exe",
];

/// Friendly names of well-known executables (lower-case file names).
const FRIENDLY: &[(&str, &str)] = &[
    ("chrome.exe", "Google Chrome"),
    ("msedge.exe", "Microsoft Edge"),
    ("msedgewebview2.exe", "Edge WebView2"),
    ("firefox.exe", "Firefox"),
    ("code.exe", "Visual Studio Code"),
    ("devenv.exe", "Visual Studio"),
    ("svchost.exe", "Windows services"),
    ("explorer.exe", "Windows Explorer"),
    ("teams.exe", "Microsoft Teams"),
    ("ms-teams.exe", "Microsoft Teams"),
    ("slack.exe", "Slack"),
    ("discord.exe", "Discord"),
    ("spotify.exe", "Spotify"),
    ("msmpeng.exe", "Microsoft Defender"),
    ("searchindexer.exe", "Windows Search"),
    ("dwm.exe", "Desktop Window Manager"),
    ("rustc.exe", "Rust compiler"),
    ("cargo.exe", "Cargo"),
    ("node.exe", "Node.js"),
    ("docker desktop.exe", "Docker Desktop"),
    ("vmmem", "WSL / virtual machines"),
    ("vmmemwsl", "WSL / virtual machines"),
    ("tiworker.exe", "Windows Update (installer)"),
    ("mousocoreworker.exe", "Windows Update"),
    ("windowsterminal.exe", "Windows Terminal"),
    ("kernel_task", "macOS kernel"),
    ("windowserver", "macOS window server"),
    ("mds_stores", "Spotlight indexing"),
    ("softwareupdated", "macOS software update"),
];

/// The file-name part of a path, using both separators.
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// A friendly name for an executable name (`chrome.exe` → "Google Chrome", `foo-bar.exe` → "foo-bar").
pub fn friendly_name(exe_name: &str) -> String {
    let lower = exe_name.to_ascii_lowercase();
    if let Some((_, f)) = FRIENDLY.iter().find(|(k, _)| *k == lower) {
        return (*f).to_string();
    }
    let stem = exe_name
        .strip_suffix(".exe")
        .or_else(|| exe_name.strip_suffix(".EXE"))
        .unwrap_or(exe_name);
    let mut chars = stem.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => "Unknown".into(),
    }
}

/// The outermost macOS app bundle in a path: `/Applications/Slack.app/Contents/…` → `/Applications/Slack.app`.
pub fn app_bundle(path: &str) -> Option<&str> {
    let idx = path.find(".app/")?;
    Some(&path[..idx + 4])
}

/// The normalised folder of an executable (lower-case, `/` separators, without the file name).
fn folder(path: &str) -> String {
    let norm = normalise(path);
    match norm.rfind('/') {
        Some(i) => norm[..i].to_string(),
        None => String::new(),
    }
}

/// Lower-case with `/` separators: paths compare case-insensitively (Windows and default macOS volumes are).
fn normalise(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

/// The key a process groups by on its own (rules 1, 3, 4).
fn own_key(p: &ProcessSample) -> String {
    match &p.exe {
        Some(exe) => match app_bundle(exe) {
            Some(bundle) => format!("bundle:{}", normalise(bundle)),
            None => format!("exe:{}", normalise(exe)),
        },
        None => format!("name:{}", p.name.to_lowercase()),
    }
}

/// Display name for a group key's first member.
fn display_name(p: &ProcessSample) -> String {
    if let Some(bundle) = p.exe.as_deref().and_then(app_bundle) {
        let name = file_name(bundle);
        return name.strip_suffix(".app").unwrap_or(name).to_string();
    }
    friendly_name(&p.name)
}

/// Whether `child` lives inside `parent`'s install folder (rule 2).
fn lives_under(child: &ProcessSample, parent: &ProcessSample) -> bool {
    match (&child.exe, &parent.exe) {
        (Some(c), Some(p)) => {
            let pf = folder(p);
            // A bare drive or root is no install folder.
            pf.len() > 3 && pf.matches('/').count() >= 1 && normalise(c).starts_with(&format!("{pf}/"))
        }
        _ => false,
    }
}

/// Groups processes by application, busiest (CPU, then memory) first.
pub fn group(processes: &[ProcessSample]) -> Vec<AppGroup> {
    let by_pid: HashMap<Pid, &ProcessSample> = processes.iter().map(|p| (p.pid, p)).collect();
    let mut keys: HashMap<Pid, String> = HashMap::with_capacity(processes.len());
    for p in processes {
        let key = resolve_key(p, &by_pid, &mut keys, 0);
        keys.insert(p.pid, key);
    }
    let mut groups: BTreeMap<String, AppGroup> = BTreeMap::new();
    // Roots (whose parent is not in the same group) first, so the group's exe and name come from its root.
    let mut ordered: Vec<&ProcessSample> = processes.iter().collect();
    ordered.sort_by_key(|p| {
        let key = &keys[&p.pid];
        let is_child = p.parent.and_then(|pp| keys.get(&pp)).is_some_and(|k| k == key);
        (is_child, p.started, p.pid)
    });
    for p in ordered {
        let key = keys[&p.pid].clone();
        let g = groups.entry(key.clone()).or_insert_with(|| AppGroup {
            id: key,
            name: display_name(p),
            exe: p.exe.clone(),
            pids: Vec::new(),
            cpu: 0.0,
            memory: 0,
            disk_bps: 0.0,
        });
        g.pids.push(p.pid);
        g.cpu += p.cpu;
        g.memory += p.memory;
        g.disk_bps += p.disk_read_bps + p.disk_write_bps;
    }
    let mut out: Vec<AppGroup> = groups.into_values().collect();
    out.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.memory.cmp(&a.memory)).then(a.name.cmp(&b.name)));
    out
}

/// The group key of `p`, following parents for rule 2. `depth` bounds the walk (pid cycles, very deep trees).
fn resolve_key(p: &ProcessSample, by_pid: &HashMap<Pid, &ProcessSample>, cache: &mut HashMap<Pid, String>, depth: usize) -> String {
    if let Some(k) = cache.get(&p.pid) {
        return k.clone();
    }
    let own = own_key(p);
    if own.starts_with("bundle:") || depth > 32 {
        return own;
    }
    if let Some(parent) = p.parent.filter(|pp| *pp != p.pid).and_then(|pp| by_pid.get(&pp)) {
        // A parent started after the child is a reused pid, not the real parent.
        let real = parent.started <= p.started;
        let adopts = !NEVER_ADOPT.contains(&parent.name.to_lowercase().as_str());
        if real && adopts && lives_under(p, parent) {
            let k = resolve_key(parent, by_pid, cache, depth + 1);
            cache.insert(parent.pid, k.clone());
            return k;
        }
    }
    own
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::fixtures::proc;
    use crate::time::Timestamp;

    fn with_exe(mut p: ProcessSample, exe: Option<&str>) -> ProcessSample {
        p.exe = exe.map(str::to_string);
        p
    }

    #[test]
    fn many_chromes_are_one_app() {
        let mut ps = vec![with_exe(
            proc(10, Some(1), "chrome.exe", 5.0, 100),
            Some("C:\\Program Files\\Google\\Chrome\\chrome.exe"),
        )];
        for pid in 11..50 {
            ps.push(with_exe(
                proc(pid, Some(10), "chrome.exe", 1.0, 50),
                Some("c:\\program files\\google\\chrome\\CHROME.EXE"),
            ));
        }
        ps.push(proc(60, Some(1), "notepad.exe", 0.5, 10));
        let g = group(&ps);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].name, "Google Chrome");
        assert_eq!(g[0].pids.len(), 40);
        assert_eq!(g[0].pids[0], 10, "the root comes first");
        assert!((g[0].cpu - 44.0).abs() < 1e-3);
        assert_eq!(g[0].memory, (100 + 39 * 50) << 20);
        assert_eq!(g[1].name, "Notepad");
    }

    #[test]
    fn children_in_the_install_folder_join_their_parent() {
        let code = with_exe(proc(10, Some(1), "Code.exe", 1.0, 100), Some("C:\\Apps\\VS Code\\Code.exe"));
        let rg = with_exe(proc(11, Some(10), "rg.exe", 3.0, 10), Some("C:\\Apps\\VS Code\\resources\\rg.exe"));
        let grandchild = with_exe(
            proc(12, Some(11), "x.exe", 0.0, 1),
            Some("C:\\Apps\\VS Code\\resources\\bin\\x.exe"),
        );
        let outside = with_exe(proc(13, Some(10), "git.exe", 2.0, 5), Some("C:\\Git\\git.exe"));
        let g = group(&[code, rg, grandchild, outside]);
        let vs = g.iter().find(|g| g.name == "Visual Studio Code").unwrap();
        assert_eq!(vs.pids, vec![10, 11, 12]);
        assert!(g.iter().any(|g| g.name == "Git"));
    }

    #[test]
    fn shells_and_service_hosts_never_adopt() {
        let explorer = with_exe(proc(5, Some(1), "explorer.exe", 0.0, 50), Some("C:\\Windows\\explorer.exe"));
        let child = with_exe(proc(6, Some(5), "notepad.exe", 0.0, 5), Some("C:\\Windows\\notepad.exe"));
        let g = group(&[explorer, child]);
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn reused_parent_pids_and_cycles_are_ignored() {
        let mut parent = with_exe(proc(5, Some(6), "app.exe", 0.0, 1), Some("C:\\A\\app.exe"));
        let mut child = with_exe(proc(6, Some(5), "helper.exe", 0.0, 1), Some("C:\\A\\sub\\helper.exe"));
        parent.started = Timestamp(100);
        child.started = Timestamp(50); // the "parent" started later: a reused pid
        assert_eq!(group(&[parent.clone(), child.clone()]).len(), 2);
        child.started = Timestamp(200);
        let g = group(&[parent, child]);
        assert_eq!(g.len(), 1);
        let self_parent = with_exe(proc(9, Some(9), "x.exe", 0.0, 1), Some("C:\\X\\x.exe"));
        assert_eq!(group(&[self_parent]).len(), 1);
    }

    #[test]
    fn mac_helpers_join_their_bundle() {
        let main = with_exe(proc(1, None, "Slack", 1.0, 1), Some("/Applications/Slack.app/Contents/MacOS/Slack"));
        let helper = with_exe(
            proc(2, Some(1), "Slack Helper", 1.0, 1),
            Some("/Applications/Slack.app/Contents/Frameworks/Slack Helper.app/Contents/MacOS/Slack Helper"),
        );
        let g = group(&[main, helper]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].name, "Slack");
        assert_eq!(app_bundle("/usr/bin/ls"), None);
    }

    #[test]
    fn unreadable_paths_group_by_name_and_names_are_friendly() {
        let a = with_exe(proc(1, None, "System", 0.0, 1), None);
        let b = with_exe(proc(2, None, "system", 0.0, 1), None);
        let g = group(&[a, b]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].exe, None);
        assert_eq!(friendly_name("foo.exe"), "Foo");
        assert_eq!(friendly_name("BAR.EXE"), "BAR");
        assert_eq!(friendly_name(""), "Unknown");
        assert_eq!(friendly_name("MsMpEng.exe"), "Microsoft Defender");
        assert_eq!(file_name("a/b\\c.exe"), "c.exe");
        assert_eq!(folder("x.exe"), "");
    }

    #[test]
    fn root_folders_are_not_install_folders_and_deep_chains_stop() {
        let parent = with_exe(proc(1, None, "a.exe", 0.0, 1), Some("C:\\a.exe"));
        let child = with_exe(proc(2, Some(1), "b.exe", 0.0, 1), Some("C:\\x\\b.exe"));
        assert_eq!(group(&[parent, child]).len(), 2);
        // A chain of 40 nested helpers: grouping terminates and still merges the near ones.
        let mut ps = vec![with_exe(proc(1, None, "root.exe", 0.0, 1), Some("C:\\R\\root.exe"))];
        let mut dir = String::from("C:\\R");
        for pid in 2..42u32 {
            dir.push_str("\\d");
            ps.push(with_exe(proc(pid, Some(pid - 1), "h.exe", 0.0, 1), Some(&format!("{dir}\\h.exe"))));
        }
        let g = group(&ps);
        assert!(g.iter().map(|g| g.pids.len()).max().unwrap() > 30);
    }
}
