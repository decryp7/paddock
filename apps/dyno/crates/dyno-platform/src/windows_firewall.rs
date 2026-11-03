//! Windows Defender Firewall for the agent (ADR 0027 WP-9, ADR 0031 section 4): the pure assembly of a [`FirewallSource`] from what
//! `INetFwPolicy2` reports, behind a port so tests use invented records. The COM adapter is `windows::firewall_com`. Needs no
//! rights and works in every logon type (WP-0: a standard user read 3 profiles and 494 rules; the CIM cmdlets fail over SSH).

use crate::parse::firewall::{map_windows_rule, RawWinRule};
use dyno_core::firewall::{
    FirewallProfile, FirewallReader, FirewallSource, FirewallSwitch, ProfileState, RuleAction, RuleSource, MAX_FIREWALL_RULES,
};
use dyno_core::ports::PlatformError;

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

/// Reads Windows Defender Firewall through a [`WinFirewallPort`].
pub struct WindowsFirewallReader<P: WinFirewallPort>(pub P);

impl<P: WinFirewallPort> FirewallReader for WindowsFirewallReader<P> {
    fn read(&self) -> Result<FirewallSource, PlatformError> {
        let (profiles, rules) = self.0.read()?;
        let v = assemble(&profiles, &rules);
        log::info!(Profiles = v.states.len(), Rules = v.rule_count, Truncated = v.truncated; "Windows firewall read");
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn the_reader_passes_errors_on() {
        let ok = WindowsFirewallReader(Fake(Ok((vec![profile(FirewallProfile::Public, true, true)], WinRules::default()))));
        assert_eq!(ok.read().unwrap().states.len(), 1);
        let down = WindowsFirewallReader(Fake(Err(PlatformError::Failed("COM said no".into()))));
        assert!(matches!(down.read(), Err(PlatformError::Failed(m)) if m == "COM said no"));
    }
}
