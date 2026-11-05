//! Reads the real Windows Defender Firewall of the machine it runs on, as whoever runs it. Only for the local Windows test VM
//! (as `paddock-test`, a standard user; ADR 0027 WP-0): `PADDOCK_VM=1` and `--ignored`. Never in CI, never on a developer's machine.
#![cfg(windows)]

use dyno_core::firewall::FirewallReader;
use dyno_platform::windows::firewall_com::ComFirewall;
use dyno_platform::windows::indirect::ShellIndirect;
use dyno_platform::windows_firewall::WindowsFirewallReader;

#[test]
#[ignore = "reads the firewall of this machine: the Windows test VM only (PADDOCK_VM=1)"]
fn a_standard_user_reads_profiles_and_rules() {
    if std::env::var("PADDOCK_VM").as_deref() != Ok("1") {
        eprintln!("skipped: PADDOCK_VM is not 1");
        return;
    }
    let source = match WindowsFirewallReader::new(
        ComFirewall,
        Box::new(ShellIndirect),
        std::sync::Arc::new(paddock_jobs::time::SystemClock),
    )
    .read()
    {
        Ok(s) => s,
        Err(e) => panic!("the firewall should be readable as a standard user: {e}"),
    };
    println!(
        "profiles={} rules={} listed={}",
        source.states.len(),
        source.rule_count,
        source.rules.len()
    );
    assert!(!source.states.is_empty(), "no profiles");
    assert!(source.rule_count > 0 && !source.rules.is_empty(), "no rules");
    let raw: Vec<_> = source
        .rules
        .iter()
        .filter(|r| r.name.starts_with("@{") || r.name.starts_with('@'))
        .map(|r| r.name.as_str())
        .collect();
    assert!(raw.is_empty(), "rule names still raw: {raw:?}");
    let resolved = source.rules.iter().filter(|r| r.name.contains("(app rule)")).count();
    println!("fallback app-rule names={resolved}");
}
