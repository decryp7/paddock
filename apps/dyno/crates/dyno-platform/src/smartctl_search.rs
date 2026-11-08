//! Where a `smartctl` may already be on this computer (the user's own install included), as an ordered list built from the
//! environment and, on Windows, the registry. Pure over a [`SearchEnv`], so the list is tested with a fake environment.
//!
//! Windows order: `%ProgramFiles%` and `%ProgramFiles(x86)%`, the helper's own copy, the `InstallLocation` the smartmontools
//! installer registered (both registry views), the folders of `PATH`, and last the default folder.

use crate::tool_install::{smartctl_path, PINS};
use dyno_core::helper::HelperHost;
use dyno_core::helper_requests::ToolKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What the search reads from the machine.
pub trait SearchEnv {
    /// An environment variable.
    fn var(&self, name: &str) -> Option<String>;
    /// The `InstallLocation` values the smartmontools installer left in the registry (empty off Windows).
    fn registry_install_locations(&self) -> Vec<String>;
}

/// The real environment.
pub struct SystemEnv;

impl SearchEnv for SystemEnv {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|v| !v.is_empty())
    }

    fn registry_install_locations(&self) -> Vec<String> {
        #[cfg(windows)]
        {
            REGISTRY_QUERIES
                .iter()
                .filter_map(|(key, view)| {
                    let mut cmd = crate::process::command("reg.exe");
                    cmd.args(["query", key, "/v", "InstallLocation", view]);
                    let text = crate::process::output_text(cmd, Duration::from_secs(5)).ok()?;
                    parse_reg_value(&text, "InstallLocation")
                })
                .collect()
        }
        #[cfg(not(windows))]
        {
            let _ = Duration::from_secs(0);
            Vec::new()
        }
    }
}

/// The keys and registry views read for an install. The upstream installer writes its uninstall key without choosing a view,
/// so on 64-bit Windows it lands under `WOW6432Node`; both views are read.
#[cfg(windows)]
const REGISTRY_QUERIES: [(&str, &str); 4] = [
    (r"HKLM\SOFTWARE\smartmontools", "/reg:64"),
    (r"HKLM\SOFTWARE\smartmontools", "/reg:32"),
    (r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\smartmontools", "/reg:64"),
    (r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\smartmontools", "/reg:32"),
];

/// The data of the value `name` in `reg query` output (`    InstallLocation    REG_SZ    C:\Program Files\smartmontools`).
pub fn parse_reg_value(output: &str, name: &str) -> Option<String> {
    output.lines().find_map(|l| {
        let l = l.trim();
        let rest = l.strip_prefix(name)?.trim_start();
        let data = rest.strip_prefix("REG_SZ").or_else(|| rest.strip_prefix("REG_EXPAND_SZ"))?.trim();
        (!data.is_empty() && data.len() < 260).then(|| data.to_string())
    })
}

fn win_join(dir: &str, file: &str) -> PathBuf {
    PathBuf::from(format!("{}\\{}", dir.trim_end_matches(['\\', '/']), file))
}

/// The places to look for smartctl, in stages (cheap ones first), so the registry is read only when nothing before it exists.
fn windows_stages<'a>(env: &'a dyn SearchEnv) -> Vec<Box<dyn Fn() -> Vec<PathBuf> + 'a>> {
    vec![
        Box::new(move || {
            let mut v = Vec::new();
            for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
                if let Some(pf) = env.var(var) {
                    v.push(win_join(&pf, r"smartmontools\bin\smartctl.exe"));
                }
            }
            if let (Some(data), Some(pin)) = (
                env.var("ProgramData"),
                PINS.iter()
                    .find(|p| p.tool == ToolKind::Smartmontools && p.host == HelperHost::Windows),
            ) {
                let tools = win_join(&data, r"Dyno\Helper\tools");
                v.push(smartctl_path(&tools, pin.version));
            }
            v
        }),
        Box::new(move || {
            env.registry_install_locations()
                .iter()
                .flat_map(|loc| [win_join(loc, r"bin\smartctl.exe"), win_join(loc, "smartctl.exe")])
                .collect()
        }),
        Box::new(move || {
            env.var("PATH")
                .map(|p| {
                    p.split(';')
                        .filter(|d| !d.trim().is_empty())
                        .map(|d| win_join(d.trim().trim_matches('"'), "smartctl.exe"))
                        .collect()
                })
                .unwrap_or_default()
        }),
        Box::new(|| vec![PathBuf::from(r"C:\Program Files\smartmontools\bin\smartctl.exe")]),
    ]
}

fn unix_stages<'a>(env: &'a dyn SearchEnv) -> Vec<Box<dyn Fn() -> Vec<PathBuf> + 'a>> {
    vec![Box::new(move || {
        env.var("PATH")
            .map(|p| {
                p.split(':')
                    .filter(|d| !d.is_empty())
                    .map(|d| Path::new(d).join("smartctl"))
                    .collect()
            })
            .unwrap_or_default()
    })]
}

fn stages<'a>(env: &'a dyn SearchEnv, windows: bool) -> Vec<Box<dyn Fn() -> Vec<PathBuf> + 'a>> {
    if windows {
        windows_stages(env)
    } else {
        unix_stages(env)
    }
}

/// Every place to look, in order, without duplicates.
pub fn candidates(env: &dyn SearchEnv, windows: bool) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = Vec::new();
    for stage in stages(env, windows) {
        for p in stage() {
            if !all.contains(&p) {
                all.push(p);
            }
        }
    }
    all
}

/// The first place in the list where `exists` says there is a file; later stages are not computed once one is found.
pub fn first_existing(env: &dyn SearchEnv, windows: bool, exists: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    stages(env, windows)
        .iter()
        .find_map(|stage| stage().into_iter().find(|p| exists(p)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::HashMap;

    struct Fake {
        vars: HashMap<&'static str, &'static str>,
        registry: Vec<&'static str>,
        registry_reads: Cell<u32>,
    }
    impl SearchEnv for Fake {
        fn var(&self, name: &str) -> Option<String> {
            self.vars.get(name).map(|v| v.to_string())
        }
        fn registry_install_locations(&self) -> Vec<String> {
            self.registry_reads.set(self.registry_reads.get() + 1);
            self.registry.iter().map(|s| s.to_string()).collect()
        }
    }
    fn fake() -> Fake {
        Fake {
            vars: HashMap::from([
                ("ProgramFiles", r"C:\Program Files"),
                ("ProgramFiles(x86)", r"C:\Program Files (x86)"),
                ("ProgramData", r"C:\ProgramData"),
                ("PATH", r"C:\Windows\system32;D:\Tools\smart;;"),
            ]),
            registry: vec![r"E:\Apps\smartmontools\", r"C:\Program Files\smartmontools"],
            registry_reads: Cell::new(0),
        }
    }

    #[test]
    fn the_windows_list_has_program_files_the_helper_copy_the_registry_and_path_in_that_order() {
        let list: Vec<String> = candidates(&fake(), true)
            .iter()
            .map(|p| p.to_string_lossy().replace('/', "\\"))
            .collect();
        let at = |s: &str| {
            list.iter()
                .position(|p| p == s)
                .unwrap_or_else(|| panic!("{s} missing from {list:?}"))
        };
        let pf = at(r"C:\Program Files\smartmontools\bin\smartctl.exe");
        let x86 = at(r"C:\Program Files (x86)\smartmontools\bin\smartctl.exe");
        let helper = at(r"C:\ProgramData\Dyno\Helper\tools\smartmontools-7.5\bin\smartctl.exe");
        let reg = at(r"E:\Apps\smartmontools\bin\smartctl.exe");
        let path = at(r"D:\Tools\smart\smartctl.exe");
        assert!(pf < x86 && x86 < helper && helper < reg && reg < path, "{list:?}");
        assert_eq!(
            list.iter()
                .filter(|p| p.as_str() == r"C:\Program Files\smartmontools\bin\smartctl.exe")
                .count(),
            1,
            "no duplicates"
        );
    }

    #[test]
    fn a_users_own_install_in_a_custom_folder_is_found_through_the_registry_and_the_registry_is_read_only_when_needed() {
        let env = fake();
        let found = first_existing(&env, true, &|p| p.to_string_lossy() == r"E:\Apps\smartmontools\bin\smartctl.exe");
        assert_eq!(found, Some(PathBuf::from(r"E:\Apps\smartmontools\bin\smartctl.exe")));
        let env = fake();
        let found = first_existing(&env, true, &|p| {
            p.to_string_lossy() == r"C:\Program Files\smartmontools\bin\smartctl.exe"
        });
        assert!(found.is_some());
        assert_eq!(env.registry_reads.get(), 0, "found in Program Files: the registry is not read");
    }

    #[test]
    fn the_unix_list_is_the_path() {
        let env = Fake {
            vars: HashMap::from([("PATH", "/opt/x/bin::/usr/bin")]),
            registry: vec![],
            registry_reads: Cell::new(0),
        };
        assert_eq!(
            candidates(&env, false),
            [PathBuf::from("/opt/x/bin/smartctl"), PathBuf::from("/usr/bin/smartctl")]
        );
    }

    #[test]
    fn reg_query_output_is_parsed() {
        let out =
            "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\smartmontools\r\n    InstallLocation    REG_SZ    C:\\Program Files\\smartmontools\r\n\r\n";
        assert_eq!(
            parse_reg_value(out, "InstallLocation").as_deref(),
            Some(r"C:\Program Files\smartmontools")
        );
        assert_eq!(
            parse_reg_value("ERROR: The system was unable to find the specified registry key", "InstallLocation"),
            None
        );
        assert_eq!(parse_reg_value("    InstallLocation    REG_SZ    ", "InstallLocation"), None);
    }
}
