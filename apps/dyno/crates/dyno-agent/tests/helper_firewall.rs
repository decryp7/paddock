#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Firewall through a REAL helper dispatcher (helper-wiring plan section 2.2 and 4.2): the wire, the restriction table's counts for a
//! caller who is not the owner, and the agent's centre (which sets `via` itself) are the real ones; `nft` is a fixed reader.

use dyno_agent::firewall::FirewallCentre;
use dyno_agent::helper_first::CentrePort;
use dyno_core::firewall::{FirewallProfile, FirewallRule, FirewallSource, RuleAction, RuleDirection, RuleSource};
use dyno_core::helper::HelperHost;
use dyno_core::read_via::ReadVia;
use std::sync::Arc;

#[path = "common/helper_rig.rs"]
mod helper_rig;
use helper_rig::{centre_over, dispatcher, Caller, DispatcherTransport, Readers};

fn nft() -> FirewallSource {
    let rule = |i: usize| FirewallRule {
        name: format!("rule {i}"),
        direction: RuleDirection::In,
        action: RuleAction::Allow,
        enabled: true,
        program: None,
        ports: Some("22".into()),
        addresses: None,
        profile: FirewallProfile::All,
        source: RuleSource::Nftables,
        group: None,
        service: None,
        protocol: None,
    };
    FirewallSource::new(RuleSource::Nftables).with_rules((0..3).map(rule).collect())
}

fn view_as(caller: Caller) -> FirewallSource {
    let dir = std::env::temp_dir().join(format!("dyno-helper-firewall-{}-{:?}", std::process::id(), caller));
    std::fs::create_dir_all(&dir).unwrap();
    let d = dispatcher(
        HelperHost::Linux,
        Readers {
            firewall: nft(),
            ..Readers::default()
        },
        &dir,
    );
    let c = Arc::new(centre_over(DispatcherTransport::new(d, caller), HelperHost::Linux));
    let centre = FirewallCentre::new(HelperHost::Linux, vec![], vec![RuleSource::Nftables]);
    centre.bind_helper(Arc::new(CentrePort(c)));
    let v = centre.view().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    v.sources.into_iter().next().unwrap()
}

#[test]
fn the_owner_gets_the_rules_and_the_card_says_the_helper_read_them() {
    let s = view_as(Caller::Owner);
    assert_eq!((s.rules.len(), s.rules_hidden, s.via), (3, false, ReadVia::Helper));
}

#[test]
fn another_account_gets_the_count_and_not_the_rules() {
    let s = view_as(Caller::Other);
    assert_eq!((s.rule_count, s.rules.len(), s.rules_hidden, s.via), (3, 0, true, ReadVia::Helper));
}
