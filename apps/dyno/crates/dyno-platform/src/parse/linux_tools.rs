//! Parsers for the output of the Linux tools Dyno runs (always with a timeout): package managers, `systemctl`,
//! `systemd-analyze`, `loginctl`, `powerprofilesctl` and `xprop`. Pure text in, typed values out.

use dyno_core::facts::PowerPlan;

/// The package that owns a file, from `dpkg -S <path>` (`coreutils: /usr/bin/ls`, `libc6:amd64: /lib/…`).
/// Diversion notes are skipped. Call it on the output of a successful run only (errors go to stderr and a non-zero exit).
pub fn dpkg_owner(output: &str) -> Option<String> {
    let line = output.lines().find(|l| !l.starts_with("diversion by"))?;
    let (pkgs, _) = line.split_once(": ")?;
    // Several packages may own it: "a, b: /path". Name the first.
    let first = pkgs.split(',').next()?.trim();
    let name = first.split(':').next()?.trim();
    (!name.is_empty() && !name.contains(' ')).then(|| name.to_string())
}

/// The package that owns a file, from `rpm -qf <path>` (`bash-5.2.15-3.fc38.x86_64`); the "not owned" message is not
/// a package.
pub fn rpm_owner(output: &str) -> Option<String> {
    let line = output.lines().next()?.trim();
    (!line.is_empty() && !line.contains(' ')).then(|| line.to_string())
}

/// The package that owns a file, from `pacman -Qo <path>` (`/usr/bin/ls is owned by coreutils 9.4-3`).
pub fn pacman_owner(output: &str) -> Option<String> {
    let rest = output.lines().next()?.split_once(" is owned by ")?.1;
    let mut it = rest.split_whitespace();
    let name = it.next()?;
    Some(match it.next() {
        Some(v) => format!("{name} {v}"),
        None => name.to_string(),
    })
}

/// A unit file of `systemctl --user list-unit-files --state=enabled --type=service --no-legend --no-pager`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitFile {
    /// `pipewire.service`.
    pub name: String,
    /// `enabled`, `enabled-runtime`.
    pub state: String,
}

/// Parses the listing (`name state [preset]`).
pub fn unit_files(output: &str) -> Vec<UnitFile> {
    output
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next()?;
            let state = it.next()?;
            // Template units ("foo@.service") are not instances that start by themselves.
            (name.ends_with(".service") && !name.contains("@.") && name.len() <= 256).then(|| UnitFile {
                name: name.to_string(),
                state: state.to_string(),
            })
        })
        .collect()
}

/// Seconds the last boot took from `systemd-analyze time` (`Startup finished in 3.2s (firmware) + 2.1s (loader) +
/// 1.9s (kernel) + 4.3s (initrd) + 12.2s (userspace) = 23.7s`): kernel, initrd and userspace — from the firmware's
/// hand-off to a finished start — without the firmware and boot loader.
pub fn boot_secs(output: &str) -> Option<f32> {
    let line = output.lines().find(|l| l.starts_with("Startup finished in"))?;
    let rest = line.strip_prefix("Startup finished in")?;
    let rest = rest.split('=').next()?;
    let mut total = 0.0f32;
    let mut any = false;
    for part in rest.split('+') {
        let part = part.trim();
        let Some((value, what)) = part.split_once('(') else { continue };
        if !matches!(what.trim_end_matches(')').trim(), "kernel" | "initrd" | "userspace") {
            continue;
        }
        total += duration_secs(value.trim())?;
        any = true;
    }
    any.then_some(total)
}

/// systemd timespans: `1min 3.2s`, `850ms`, `12.2s`.
fn duration_secs(text: &str) -> Option<f32> {
    let mut total = 0.0f32;
    let mut any = false;
    for word in text.split_whitespace() {
        let split = word.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
        let n: f32 = word[..split].parse().ok()?;
        let factor = match &word[split..] {
            "ms" => 0.001,
            "s" => 1.0,
            "min" => 60.0,
            "h" => 3600.0,
            _ => return None,
        };
        total += n * factor;
        any = true;
    }
    (any && total.is_finite()).then_some(total)
}

/// What `loginctl show-session -p IdleHint -p IdleSinceHint` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIdle {
    /// Not idle by the desktop's own threshold.
    Active,
    /// Idle since this many microseconds after the Unix epoch.
    IdleSince(u64),
    /// The session does not report idleness.
    Unknown,
}

/// Parses `IdleHint=no` / `IdleHint=yes` and `IdleSinceHint=<µs since epoch>`.
pub fn session_idle(output: &str) -> SessionIdle {
    let get = |k: &str| output.lines().find_map(|l| l.strip_prefix(k)).map(str::trim);
    match get("IdleHint=") {
        Some("no") => SessionIdle::Active,
        Some("yes") => match get("IdleSinceHint=").and_then(|v| v.parse::<u64>().ok()) {
            Some(us) if us > 0 => SessionIdle::IdleSince(us),
            _ => SessionIdle::Unknown,
        },
        _ => SessionIdle::Unknown,
    }
}

/// The active window id of `xprop -root _NET_ACTIVE_WINDOW` (`_NET_ACTIVE_WINDOW(WINDOW): window id # 0x4a00007`).
pub fn xprop_active_window(output: &str) -> Option<String> {
    let id = output.split("# ").nth(1)?.split([',', ' ']).next()?.trim();
    (id.starts_with("0x") && id.len() > 2 && id != "0x0" && id[2..].bytes().all(|b| b.is_ascii_hexdigit())).then(|| id.to_string())
}

/// The pid of `xprop -id <window> _NET_WM_PID` (`_NET_WM_PID(CARDINAL) = 4242`).
pub fn xprop_pid(output: &str) -> Option<u32> {
    output.split(" = ").nth(1)?.trim().parse().ok().filter(|p| *p > 1)
}

/// The power plan `powerprofilesctl get` names (`power-saver`, `balanced`, `performance`).
pub fn power_profile(output: &str) -> Option<PowerPlan> {
    let id = output.trim();
    let (name, saver) = match id {
        "power-saver" => ("Power saver", true),
        "balanced" => ("Balanced", false),
        "performance" => ("Performance", false),
        _ => return None,
    };
    Some(PowerPlan {
        id: id.to_string(),
        name: name.to_string(),
        power_saver: saver,
    })
}

/// The power plan the CPU frequency policy implies. With the `intel_pstate` / `amd-pstate*` drivers the governor
/// `powersave` is the normal setting and the energy-performance preference tells (`power` = saving); with the
/// generic drivers `powersave` pins the lowest frequency.
pub fn cpufreq_plan(governor: &str, driver: &str, preference: Option<&str>) -> Option<PowerPlan> {
    let governor = governor.trim();
    if governor.is_empty() {
        return None;
    }
    let hardware_managed = driver.trim().starts_with("intel_pstate") || driver.trim().starts_with("amd-pstate");
    let (name, saver) = if hardware_managed {
        match preference.map(str::trim) {
            Some("power") => ("Power saver (energy preference)".to_string(), true),
            Some("performance") => ("Performance (energy preference)".to_string(), false),
            _ => (format!("{governor} (balanced energy preference)"), false),
        }
    } else {
        match governor {
            "powersave" => ("Power saver (powersave governor)".to_string(), true),
            "performance" => ("Performance (performance governor)".to_string(), false),
            g => (format!("{g} governor"), false),
        }
    };
    Some(PowerPlan {
        id: governor.to_string(),
        name,
        power_saver: saver,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_owners() {
        assert_eq!(dpkg_owner("coreutils: /usr/bin/ls\n"), Some("coreutils".into()));
        assert_eq!(dpkg_owner("libc6:amd64: /lib/x86_64-linux-gnu/libc.so.6\n"), Some("libc6".into()));
        assert_eq!(dpkg_owner("diversion by dash from: /bin/sh\ndash: /bin/sh\n"), Some("dash".into()));
        assert_eq!(dpkg_owner(""), None);
        assert_eq!(rpm_owner("bash-5.2.15-3.fc38.x86_64\n"), Some("bash-5.2.15-3.fc38.x86_64".into()));
        assert_eq!(rpm_owner("file /opt/x is not owned by any package\n"), None);
        assert_eq!(rpm_owner(""), None);
        assert_eq!(
            pacman_owner("/usr/bin/ls is owned by coreutils 9.4-3\n"),
            Some("coreutils 9.4-3".into())
        );
        assert_eq!(pacman_owner("error: No package owns /opt/x\n"), None);
    }

    #[test]
    fn user_units() {
        let t =
            "pipewire.service enabled enabled\nsyncthing.service enabled-runtime -\ndbus@.service enabled -\nfoo.socket enabled -\nbad\n";
        let u = unit_files(t);
        assert_eq!(
            u.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(),
            ["pipewire.service", "syncthing.service"]
        );
        assert_eq!(u[1].state, "enabled-runtime");
        assert!(unit_files("").is_empty());
    }

    #[test]
    fn boot_time() {
        let t = "Startup finished in 3.2s (firmware) + 2.1s (loader) + 1.9s (kernel) + 4.3s (initrd) + 1min 12.2s (userspace) = 1min 23.7s\ngraphical.target reached after 1min 10s in userspace.\n";
        assert!((boot_secs(t).unwrap() - 78.4).abs() < 0.01);
        assert!((boot_secs("Startup finished in 2.0s (kernel) + 850ms (userspace) = 2.85s").unwrap() - 2.85).abs() < 0.01);
        assert_eq!(boot_secs("Startup finished in 3.2s (firmware) = 3.2s"), None);
        assert_eq!(boot_secs("Startup finished in xs (kernel)"), None);
        assert_eq!(boot_secs("Bootup is not yet finished"), None);
    }

    #[test]
    fn idle_hints() {
        assert_eq!(session_idle("IdleHint=no\nIdleSinceHint=0\n"), SessionIdle::Active);
        assert_eq!(
            session_idle("IdleHint=yes\nIdleSinceHint=1700000000000000\n"),
            SessionIdle::IdleSince(1_700_000_000_000_000)
        );
        assert_eq!(session_idle("IdleHint=yes\nIdleSinceHint=0\n"), SessionIdle::Unknown);
        assert_eq!(session_idle(""), SessionIdle::Unknown);
    }

    #[test]
    fn x11_active_window() {
        assert_eq!(
            xprop_active_window("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x4a00007\n"),
            Some("0x4a00007".into())
        );
        assert_eq!(xprop_active_window("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x0\n"), None);
        assert_eq!(xprop_active_window("_NET_ACTIVE_WINDOW:  not found."), None);
        assert_eq!(xprop_pid("_NET_WM_PID(CARDINAL) = 4242\n"), Some(4242));
        assert_eq!(xprop_pid("_NET_WM_PID:  not found."), None);
        assert_eq!(xprop_pid("_NET_WM_PID(CARDINAL) = 1"), None);
    }

    #[test]
    fn power_plans() {
        assert!(power_profile("power-saver\n").unwrap().power_saver);
        assert!(!power_profile("balanced").unwrap().power_saver);
        assert_eq!(power_profile("custom"), None);
        assert!(cpufreq_plan("powersave", "acpi-cpufreq", None).unwrap().power_saver);
        assert!(
            !cpufreq_plan("powersave", "intel_pstate", Some("balance_performance"))
                .unwrap()
                .power_saver
        );
        assert!(cpufreq_plan("powersave", "amd-pstate-epp", Some("power")).unwrap().power_saver);
        assert!(!cpufreq_plan("performance", "acpi-cpufreq", None).unwrap().power_saver);
        assert!(!cpufreq_plan("schedutil", "", None).unwrap().power_saver);
        assert_eq!(cpufreq_plan("", "x", None), None);
    }
}
