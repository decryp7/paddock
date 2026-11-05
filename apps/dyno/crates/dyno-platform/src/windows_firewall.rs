//! Windows Defender Firewall for the agent (ADR 0027 WP-9, ADR 0031 section 4): the pure assembly of a [`FirewallSource`] from what
//! `INetFwPolicy2` reports, behind a port so tests use invented records. The COM adapter is `windows::firewall_com`. Needs no
//! rights and works in every logon type (WP-0: a standard user read 3 profiles and 494 rules; the CIM cmdlets fail over SSH).

use crate::parse::firewall::{map_windows_rule, RawWinRule};
use crate::parse::win_indirect::{Field, IndirectLoader, IndirectNames, ReadStats};
use dyno_core::firewall::{
    FirewallProfile, FirewallReader, FirewallSource, FirewallSwitch, ProfileState, RuleAction, RuleSource, MAX_FIREWALL_RULES,
};
use dyno_core::ports::PlatformError;
use paddock_jobs::time::Clock;
use std::sync::Arc;

/// One profile as `INetFwPolicy2` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinProfile {
    /// Domain, private or public.
    pub profile: FirewallProfile,
    /// `FirewallEnabled`.
    pub enabled: bool,
    /// `BlockAllInboundTraffic`.
    pub block_all_inbound: bool,
    /// `DefaultInboundAction`.
    pub default_in: RuleAction,
    /// `DefaultOutboundAction`.
    pub default_out: RuleAction,
    /// Whether the profile is active now (`CurrentProfileTypes`).
    pub current: bool,
}

/// The rules read, and how many there are in all.
#[derive(Debug, Clone, Default)]
pub struct WinRules {
    /// At most [`MAX_FIREWALL_RULES`] rules.
    pub rules: Vec<RawWinRule>,
    /// `INetFwRules.Count`.
    pub total: u32,
}

/// What the reader needs from COM.
pub trait WinFirewallPort: Send + Sync {
    /// The three profiles, and every rule up to the cap.
    fn read(&self) -> Result<(Vec<WinProfile>, WinRules), PlatformError>;
}

/// Builds the source: state per profile, `BlockAll` when an enabled profile blocks all inbound traffic, defaults from the active
/// profile (else the first), and the rules in the one model.
pub fn assemble(profiles: &[WinProfile], rules: &WinRules) -> FirewallSource {
    let mut v = FirewallSource::new(RuleSource::WindowsFirewall);
    v.states = profiles
        .iter()
        .map(|p| ProfileState {
            profile: p.profile,
            enabled: p.enabled,
        })
        .collect();
    v.switches
        .push((FirewallSwitch::BlockAll, profiles.iter().any(|p| p.enabled && p.block_all_inbound)));
    if let Some(p) = profiles.iter().find(|p| p.current).or(profiles.first()) {
        v.default_in = Some(p.default_in);
        v.default_out = Some(p.default_out);
    }
    let mapped: Vec<_> = rules.rules.iter().take(MAX_FIREWALL_RULES).map(map_windows_rule).collect();
    let mut v = v.with_rules(mapped);
    v.rule_count = rules.total.max(v.rule_count);
    v.truncated = v.rule_count.saturating_sub(u32::try_from(v.rules.len()).unwrap_or(u32::MAX));
    v
}

/// Reads Windows Defender Firewall through a [`WinFirewallPort`]. Rule names and groups that Windows stores as indirect strings
/// (`@{...}`, `@file.dll,-id`) are resolved once each, within a budget, and read as a fallback when that fails.
pub struct WindowsFirewallReader<P: WinFirewallPort> {
    port: P,
    names: IndirectNames,
    clock: Arc<dyn Clock>,
}

impl<P: WinFirewallPort> WindowsFirewallReader<P> {
    /// A reader over `port` that resolves indirect names with `loader`, timed by `clock`.
    pub fn new(port: P, loader: Box<dyn IndirectLoader>, clock: Arc<dyn Clock>) -> Self {
        WindowsFirewallReader {
            port,
            names: IndirectNames::new(loader, clock.clone()),
            clock,
        }
    }

    fn readable(&self, mut rules: WinRules) -> (WinRules, ReadStats) {
        let mut budget = self.names.budget();
        for r in &mut rules.rules {
            r.name = self.names.name(&r.name, Field::Name, &mut budget);
            if let Some(g) = &r.group {
                r.group = Some(self.names.name(g, Field::Group, &mut budget));
            }
        }
        let stats = budget.stats().clone();
        (rules, stats)
    }
}

impl<P: WinFirewallPort> FirewallReader for WindowsFirewallReader<P> {
    fn read(&self) -> Result<FirewallSource, PlatformError> {
        let started = self.clock.monotonic();
        let (profiles, rules) = self.port.read()?;
        let read_ms = self.clock.monotonic().saturating_sub(started).as_millis() as u64;
        let named = self.clock.monotonic();
        let (rules, names) = self.readable(rules);
        let names_ms = self.clock.monotonic().saturating_sub(named).as_millis() as u64;
        let v = assemble(&profiles, &rules);
        log::info!(Profiles = v.states.len(), Rules = v.rule_count, Truncated = v.truncated, ReadMs = read_ms; "Windows firewall read");
        if names.resolved + names.fallbacks > 0 {
            let first_error = names.first_error.as_deref().unwrap_or("none");
            log::info!(
                Resolved = names.resolved,
                Fallbacks = names.fallbacks,
                OverBudget = names.over_budget,
                TimedOut = names.timed_out,
                Lookups = names.lookups,
                FirstError = first_error,
                Ms = names_ms,
                Cached = self.names.cached();
                "Firewall rule names resolved"
            );
            if names.over_budget > 0 {
                log::warn!(OverBudget = names.over_budget, Ms = names_ms; "The name budget ran out; the rest show a fallback until the next read");
            }
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::win_indirect::{Loaded, NoLoader};
    use paddock_jobs::time::FakeClock;

    fn profile(p: FirewallProfile, enabled: bool, current: bool) -> WinProfile {
        WinProfile {
            profile: p,
            enabled,
            block_all_inbound: false,
            default_in: RuleAction::Block,
            default_out: RuleAction::Allow,
            current,
        }
    }

    struct Fake(Result<(Vec<WinProfile>, WinRules), PlatformError>);
    impl WinFirewallPort for Fake {
        fn read(&self) -> Result<(Vec<WinProfile>, WinRules), PlatformError> {
            self.0.clone()
        }
    }

    #[test]
    fn profiles_defaults_switches_and_counts_come_through() {
        let mut public = profile(FirewallProfile::Public, true, true);
        public.block_all_inbound = true;
        public.default_out = RuleAction::Block;
        let profiles = [
            profile(FirewallProfile::Domain, false, false),
            profile(FirewallProfile::Private, true, false),
            public,
        ];
        let rule = |n: &str| RawWinRule {
            name: n.into(),
            direction: 1,
            action: 1,
            enabled: true,
            profiles: 0x7fff_ffff,
            ..Default::default()
        };
        let rules = WinRules {
            rules: vec![rule("a"), rule("b")],
            total: 7,
        };
        let v = assemble(&profiles, &rules);
        assert_eq!(v.states.iter().map(|s| s.enabled).collect::<Vec<_>>(), [false, true, true]);
        assert_eq!(v.switches, [(FirewallSwitch::BlockAll, true)]);
        assert_eq!(
            (v.default_in, v.default_out),
            (Some(RuleAction::Block), Some(RuleAction::Block)),
            "from the active profile"
        );
        assert_eq!((v.rules.len(), v.rule_count, v.truncated), (2, 7, 5));
    }

    #[test]
    fn a_disabled_profile_does_not_set_block_all_and_no_profiles_is_still_a_source() {
        let mut off = profile(FirewallProfile::Public, false, false);
        off.block_all_inbound = true;
        assert_eq!(assemble(&[off], &WinRules::default()).switches, [(FirewallSwitch::BlockAll, false)]);
        let v = assemble(&[], &WinRules::default());
        assert_eq!((v.states.len(), v.default_in), (0, None));
    }

    fn reader(port: Fake) -> WindowsFirewallReader<Fake> {
        WindowsFirewallReader::new(port, Box::new(NoLoader), Arc::new(FakeClock::new()))
    }

    #[test]
    fn the_reader_passes_errors_on() {
        let ok = reader(Fake(Ok((vec![profile(FirewallProfile::Public, true, true)], WinRules::default()))));
        assert_eq!(ok.read().unwrap().states.len(), 1);
        let down = reader(Fake(Err(PlatformError::Failed("COM said no".into()))));
        assert!(matches!(down.read(), Err(PlatformError::Failed(m)) if m == "COM said no"));
    }

    struct Resolves;
    impl IndirectLoader for Resolves {
        fn load(&self, s: &str) -> Loaded {
            if s.contains("OOBE") {
                Loaded::Text("Out of box experience".into())
            } else {
                Loaded::Unresolvable("0x80070002".into())
            }
        }
    }

    #[test]
    fn indirect_names_and_groups_are_readable_and_the_list_never_fails() {
        let rule = |name: &str, group: Option<&str>| RawWinRule {
            name: name.into(),
            group: group.map(String::from),
            direction: 1,
            action: 1,
            enabled: true,
            profiles: 0x7fff_ffff,
            ..Default::default()
        };
        let rules = WinRules {
            rules: vec![
                rule(
                    "@{MicrosoftWindows.Client.OOBE_1000.1_x64__cw5n1h2txyewy?ms-resource://X/Resources/N}",
                    Some("@FirewallAPI.dll,-23255"),
                ),
                rule("@{Contoso.Notes_1.2_x64__abc?ms-resource://Contoso.Notes/Resources/N}", None),
                rule("Core Networking", Some("Core Networking")),
            ],
            total: 3,
        };
        let r = WindowsFirewallReader::new(Fake(Ok((vec![], rules))), Box::new(Resolves), Arc::new(FakeClock::new()));
        let v = r.read().unwrap();
        let names: Vec<_> = v.rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Out of box experience", "Contoso Notes (app rule)", "Core Networking"]);
        assert_eq!(v.rules[0].group.as_deref(), Some("Windows rule group (Firewall API 23255)"));
        assert!(v
            .rules
            .iter()
            .all(|r| !r.name.contains("@{") && !r.group.as_deref().unwrap_or("").starts_with('@')));
    }
}
