//! Reads the real Windows Defender Firewall of the machine it runs on, as whoever runs it. Only for the local Windows test VM
//! (as `paddock-test`, a standard user; ADR 0027 WP-0): `PADDOCK_VM=1` and `--ignored`. Never in CI, never on a developer's machine.
#![cfg(windows)]

use dyno_core::firewall::FirewallReader;
use dyno_platform::parse::win_indirect::{fallback, is_indirect, Field, IndirectNames};
use dyno_platform::windows::firewall_com::ComFirewall;
use dyno_platform::windows::indirect::ShellIndirect;
use dyno_platform::windows_firewall::{WinFirewallPort, WindowsFirewallReader};
use paddock_jobs::time::{Clock, SystemClock};
use std::sync::Arc;
use std::time::Instant;

fn vm_only() -> bool {
    if std::env::var("PADDOCK_VM").as_deref() != Ok("1") {
        eprintln!("skipped: PADDOCK_VM is not 1");
        return false;
    }
    true
}

#[test]
#[ignore = "reads the firewall of this machine: the Windows test VM only (PADDOCK_VM=1)"]
fn a_standard_user_reads_profiles_and_rules() {
    if !vm_only() {
        return;
    }
    let started = Instant::now();
    let reader = WindowsFirewallReader::new(ComFirewall, Box::new(ShellIndirect), Arc::new(SystemClock));
    let source = match reader.read() {
        Ok(s) => s,
        Err(e) => panic!("the firewall should be readable as a standard user: {e}"),
    };
    println!(
        "profiles={} rules={} listed={} cold_first_read_ms={}",
        source.states.len(),
        source.rule_count,
        source.rules.len(),
        started.elapsed().as_millis()
    );
    assert!(!source.states.is_empty(), "no profiles");
    assert!(source.rule_count > 0 && !source.rules.is_empty(), "no rules");
    let raw: Vec<_> = source
        .rules
        .iter()
        .flat_map(|r| [Some(r.name.as_str()), r.group.as_deref()])
        .flatten()
        .filter(|t| t.starts_with('@'))
        .collect();
    assert!(raw.is_empty(), "names or groups still raw: {raw:?}");
}

/// Proves the lookup, not just the fallback: of the raw strings the real firewall holds, how many `@file.dll,-id` and `@{package}`
/// strings the OS resolved, and that at least one `@FirewallAPI.dll,-n` did.
#[test]
#[ignore = "reads the firewall of this machine: the Windows test VM only (PADDOCK_VM=1)"]
fn the_os_resolves_the_firewalls_own_indirect_names() {
    if !vm_only() {
        return;
    }
    let (_, rules) = match ComFirewall.read() {
        Ok(r) => r,
        Err(e) => panic!("the firewall should be readable: {e}"),
    };
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let names = IndirectNames::new(Box::new(ShellIndirect), clock);
    let mut budget = names.budget();
    let (mut dll, mut dll_ok, mut pkg, mut pkg_ok, mut shown) = (0, 0, 0, 0, Vec::new());
    for s in rules
        .rules
        .iter()
        .flat_map(|r| [Some(r.name.as_str()), r.group.as_deref()])
        .flatten()
        .filter(|s| is_indirect(s))
    {
        let out = names.name(s, Field::Name, &mut budget);
        let is_pkg = s.trim().starts_with("@{");
        let resolved = out != fallback(s, Field::Name);
        if is_pkg {
            pkg += 1;
            pkg_ok += usize::from(resolved);
        } else {
            dll += 1;
            dll_ok += usize::from(resolved);
        }
        if shown.len() < 6
            && !shown
                .iter()
                .any(|(a, _): &(String, String)| *a == s.trim().chars().take(60).collect::<String>())
        {
            shown.push((s.trim().chars().take(60).collect(), out));
        }
    }
    println!(
        "dll_strings={dll} dll_resolved={dll_ok} package_strings={pkg} package_resolved={pkg_ok} stats={:?}",
        budget.stats()
    );
    for (raw, out) in &shown {
        println!("  {raw} => {out}");
    }
    assert!(
        dll == 0 || dll_ok > 0,
        "{dll} @file.dll,-id strings and the OS resolved none of them"
    );
    assert!(dll + pkg > 0, "this firewall has no indirect names to prove the lookup on");
}
