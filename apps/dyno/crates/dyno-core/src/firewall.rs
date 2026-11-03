//! One model of a firewall for every OS (ADR 0027 WP-9): per-profile state, default policies, and bounded rules with a direction,
//! an action, a program, ports, addresses, a profile, whether the rule is on, and where it came from. The OS readers (the
//! agent's Windows and macOS Application Firewall readers, the helper's `pf` and `nft` readers) all produce a [`FirewallView`].

use crate::helper_policy::Access;
use crate::ports::PlatformError;
use serde::{Deserialize, Serialize};

/// The most rules one view carries.
pub const MAX_FIREWALL_RULES: usize = 4_096;
/// The longest text field of a rule.
pub const MAX_RULE_TEXT: usize = 256;

/// Which way a rule filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleDirection {
    /// Incoming connections.
    In,
    /// Outgoing connections.
    Out,
    /// Both, or forwarded traffic.
    Any,
}

/// What a rule does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleAction {
    /// Lets the traffic through.
    Allow,
    /// Stops it (block, drop or reject).
    Block,
}

/// A Windows network profile; every other firewall has one profile, [`FirewallProfile::All`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FirewallProfile {
    /// Domain networks.
    Domain,
    /// Private networks.
    Private,
    /// Public networks.
    Public,
    /// Every profile.
    All,
}

/// Where a rule came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleSource {
    /// Windows Defender Firewall (`INetFwPolicy2`).
    WindowsFirewall,
    /// The macOS Application Firewall (`socketfilterfw`).
    ApplicationFirewall,
    /// macOS `pf`.
    Pf,
    /// Linux nftables.
    Nftables,
}

/// One firewall rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallRule {
    /// A name for the list: the rule's own name, or its place in the rule set.
    pub name: String,
    /// The direction.
    pub direction: RuleDirection,
    /// The action.
    pub action: RuleAction,
    /// Whether the rule is switched on.
    pub enabled: bool,
    /// The program it names, when it names one.
    pub program: Option<String>,
    /// The ports it names, as the firewall prints them.
    pub ports: Option<String>,
    /// The addresses it names, as the firewall prints them.
    pub addresses: Option<String>,
    /// The profile.
    pub profile: FirewallProfile,
    /// Where it came from.
    pub source: RuleSource,
}

/// Whether the firewall is on for one profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileState {
    /// The profile.
    pub profile: FirewallProfile,
    /// Whether the firewall is on for it.
    pub enabled: bool,
}

/// A switch some firewalls have besides rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FirewallSwitch {
    /// Block all incoming connections (macOS).
    BlockAll,
    /// Do not answer probes (macOS stealth mode).
    Stealth,
}

/// The firewall as one OS reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallView {
    /// Where the data came from.
    pub source: RuleSource,
    /// On or off per profile.
    pub states: Vec<ProfileState>,
    /// Extra switches that are on or off.
    pub switches: Vec<(FirewallSwitch, bool)>,
    /// The default policy for incoming traffic, when the firewall has one.
    pub default_in: Option<RuleAction>,
    /// The default policy for outgoing traffic, when the firewall has one.
    pub default_out: Option<RuleAction>,
    /// How many rules there are (the cap does not limit this count).
    pub rule_count: u32,
    /// The rules, at most [`MAX_FIREWALL_RULES`]; empty when `rules_hidden`.
    pub rules: Vec<FirewallRule>,
    /// Rules left out because of the cap.
    pub truncated: u32,
    /// The rules were withheld because the asking account may see only states and counts.
    pub rules_hidden: bool,
}

impl FirewallView {
    /// A view of `source` with no rules yet.
    pub fn new(source: RuleSource) -> FirewallView {
        FirewallView {
            source,
            states: Vec::new(),
            switches: Vec::new(),
            default_in: None,
            default_out: None,
            rule_count: 0,
            rules: Vec::new(),
            truncated: 0,
            rules_hidden: false,
        }
    }

    /// Takes `rules`, keeping the count and capping the list.
    pub fn with_rules(mut self, mut rules: Vec<FirewallRule>) -> FirewallView {
        self.rule_count = u32::try_from(rules.len()).unwrap_or(u32::MAX);
        self.truncated = u32::try_from(rules.len().saturating_sub(MAX_FIREWALL_RULES)).unwrap_or(u32::MAX);
        rules.truncate(MAX_FIREWALL_RULES);
        self.rules = rules;
        self
    }

    /// States and counts only.
    pub fn counts_only(mut self) -> FirewallView {
        self.rules = Vec::new();
        self.truncated = 0;
        self.rules_hidden = true;
        self
    }

    /// What `access` (the restriction table) lets a caller have of this view; `None` when denied.
    pub fn for_access(self, access: Access) -> Option<FirewallView> {
        match access {
            Access::Full => Some(self),
            Access::CountsOnly => Some(self.counts_only()),
            Access::Denied => None,
        }
    }
}

/// Reads a firewall. Implemented per OS: the agent's readers need no rights, the helper's need root.
pub trait FirewallReader: Send + Sync {
    /// The firewall as the OS reports it.
    fn read(&self) -> Result<FirewallView, PlatformError>;
}

/// Cuts a text field to [`MAX_RULE_TEXT`] characters.
pub fn clip(s: &str) -> String {
    s.trim().chars().take(MAX_RULE_TEXT).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(i: usize) -> FirewallRule {
        FirewallRule {
            name: format!("rule {i}"),
            direction: RuleDirection::In,
            action: RuleAction::Allow,
            enabled: true,
            program: None,
            ports: Some("22".into()),
            addresses: None,
            profile: FirewallProfile::All,
            source: RuleSource::Nftables,
        }
    }

    #[test]
    fn rules_are_capped_but_counted() {
        let v = FirewallView::new(RuleSource::Nftables).with_rules((0..MAX_FIREWALL_RULES + 5).map(rule).collect());
        assert_eq!(
            (v.rules.len(), v.rule_count, v.truncated),
            (MAX_FIREWALL_RULES, MAX_FIREWALL_RULES as u32 + 5, 5)
        );
    }

    #[test]
    fn the_restriction_table_decides_what_a_caller_gets() {
        let v = FirewallView::new(RuleSource::Pf).with_rules(vec![rule(1), rule(2)]);
        let full = v.clone().for_access(Access::Full).unwrap();
        assert_eq!((full.rules.len(), full.rules_hidden), (2, false));
        let counts = v.clone().for_access(Access::CountsOnly).unwrap();
        assert_eq!((counts.rules.len(), counts.rule_count, counts.rules_hidden), (0, 2, true));
        assert!(v.for_access(Access::Denied).is_none());
    }

    #[test]
    fn text_is_clipped() {
        assert_eq!(clip(&"x".repeat(1000)).len(), MAX_RULE_TEXT);
        assert_eq!(clip("  a "), "a");
    }
}
