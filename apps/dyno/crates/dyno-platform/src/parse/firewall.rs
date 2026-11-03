//! Bounded parsers for what firewalls print (ADR 0027 WP-9): `socketfilterfw` (macOS Application Firewall), `pfctl` (macOS `pf`),
//! `nft -j list ruleset` (Linux), and the mapping of Windows' `INetFwRule` properties. None panics on any input; every one caps its
//! output. Rule addresses are data for the answer only: nothing here logs them.

use dyno_core::firewall::{
    clip, FirewallProfile, FirewallRule, FirewallSource, FirewallSwitch, ProfileState, RuleAction, RuleDirection, RuleSource,
    MAX_FIREWALL_RULES,
};
use serde_json::Value;

/// The most lines or JSON items read from one output.
const MAX_ITEMS: usize = 100_000;
/// The most bytes of `nft -j` output read (the helper's reply frame is 4 MB).
pub const MAX_NFT_BYTES: usize = 4 * 1024 * 1024;

/// Why an output could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirewallParseError {
    /// The output is larger than the limit.
    TooLarge,
    /// The output is not what the tool prints.
    Unrecognised(String),
}

impl std::fmt::Display for FirewallParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FirewallParseError::TooLarge => f.write_str("The firewall's rule list is larger than Dyno reads."),
            FirewallParseError::Unrecognised(w) => write!(f, "The firewall tool printed something Dyno does not recognise ({w})."),
        }
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .chars()
        .take(80)
        .collect()
}

// ---------------------------------------------------------------- macOS Application Firewall

/// `socketfilterfw --getglobalstate`: "Firewall is enabled. (State = 1)". `None` when the line is not recognised.
pub fn parse_alf_global_state(text: &str) -> Option<bool> {
    let t = text.to_ascii_lowercase();
    let state = t
        .split("state =")
        .nth(1)
        .map(|s| s.trim_start().chars().take_while(char::is_ascii_digit).collect::<String>());
    match state.as_deref() {
        Some("0") => Some(false),
        Some(n) if !n.is_empty() => Some(true),
        _ if t.contains("disabled") => Some(false),
        _ if t.contains("enabled") => Some(true),
        _ => None,
    }
}

/// `--getblockall` ("Block all DISABLED!") and `--getstealthmode` ("Stealth mode enabled"): on or off, `None` if unclear.
pub fn parse_alf_switch(text: &str) -> Option<bool> {
    let t = text.to_ascii_lowercase();
    if t.contains("disabled") {
        Some(false)
    } else if t.contains("enabled") {
        Some(true)
    } else {
        None
    }
}

/// `socketfilterfw --listapps`: "N :  /path/App.app" followed by "( Allow incoming connections )" or "( Block ... )".
pub fn parse_alf_apps(text: &str) -> Vec<FirewallRule> {
    let mut out: Vec<FirewallRule> = Vec::new();
    let mut path: Option<String> = None;
    for line in text.lines().take(MAX_ITEMS) {
        let t = line.trim();
        if let Some((num, rest)) = t.split_once(" :") {
            if !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()) {
                path = Some(clip(rest));
                continue;
            }
        }
        if t.starts_with('(') {
            let lower = t.to_ascii_lowercase();
            let action = if lower.contains("block") {
                Some(RuleAction::Block)
            } else if lower.contains("allow") {
                Some(RuleAction::Allow)
            } else {
                None
            };
            if let (Some(p), Some(action)) = (path.take(), action) {
                let name = p.rsplit('/').next().unwrap_or(&p).trim_end_matches(".app").to_string();
                out.push(FirewallRule {
                    name: clip(&name),
                    direction: RuleDirection::In,
                    action,
                    enabled: true,
                    program: Some(p),
                    ports: None,
                    addresses: None,
                    profile: FirewallProfile::All,
                    source: RuleSource::ApplicationFirewall,
                    group: None,
                    service: None,
                    protocol: None,
                });
            }
        }
    }
    out
}

/// The Application Firewall as a view from the four outputs.
pub fn alf_view(global: &str, block_all: &str, stealth: &str, apps: &str) -> Result<FirewallSource, FirewallParseError> {
    let enabled = parse_alf_global_state(global).ok_or_else(|| FirewallParseError::Unrecognised(first_line(global)))?;
    let mut v = FirewallSource::new(RuleSource::ApplicationFirewall).with_rules(parse_alf_apps(apps));
    v.states.push(ProfileState {
        profile: FirewallProfile::All,
        enabled,
    });
    if let Some(b) = parse_alf_switch(block_all) {
        v.switches.push((FirewallSwitch::BlockAll, b));
    }
    if let Some(s) = parse_alf_switch(stealth) {
        v.switches.push((FirewallSwitch::Stealth, s));
    }
    Ok(v)
}

// ---------------------------------------------------------------- pf

/// `pfctl -s info`: "Status: Enabled for 0 days ..." or "Status: Disabled".
pub fn parse_pf_status(text: &str) -> Option<bool> {
    let line = text.lines().map(str::trim).find(|l| l.starts_with("Status:"))?;
    let rest = line.trim_start_matches("Status:").trim().to_ascii_lowercase();
    if rest.starts_with("enabled") {
        Some(true)
    } else if rest.starts_with("disabled") {
        Some(false)
    } else {
        None
    }
}

const PF_STOP: [&str; 14] = [
    "port", "flags", "keep", "modulate", "synproxy", "queue", "label", "user", "group", "tagged", "tag", "os", "to", "from",
];

/// The words after `key` up to the next keyword: one word, `= word`, or a `{ a b }` list kept whole.
fn pf_value(tokens: &[&str], key: &str) -> Option<String> {
    let at = tokens.iter().position(|t| *t == key)?;
    let (mut words, mut depth): (Vec<&str>, i32) = (Vec::new(), 0);
    for t in &tokens[at + 1..] {
        match *t {
            "{" => {
                depth += 1;
                words.push(t);
            }
            "}" => {
                depth -= 1;
                words.push(t);
                if depth <= 0 {
                    break;
                }
            }
            _ if depth > 0 => words.push(t),
            "=" => words.push(t),
            s if PF_STOP.contains(&s) => break,
            _ => {
                words.push(t);
                break;
            }
        }
    }
    let joined = words.join(" ");
    (!joined.is_empty()).then(|| clip(&joined))
}

/// `pfctl -s rules`: one rule per line. Lines that are not filter rules (anchors, `scrub`, `set`, ALTQ notices) are skipped.
pub fn parse_pf_rules(text: &str) -> Vec<FirewallRule> {
    let mut out = Vec::new();
    for line in text.lines().take(MAX_ITEMS) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let action = match tokens.first().copied() {
            Some("pass") => RuleAction::Allow,
            Some("block") => RuleAction::Block,
            _ => continue,
        };
        let direction = if tokens.contains(&"in") {
            RuleDirection::In
        } else if tokens.contains(&"out") {
            RuleDirection::Out
        } else {
            RuleDirection::Any
        };
        let from = pf_value(&tokens, "from");
        let to = pf_value(&tokens, "to");
        let addresses = match (from, to) {
            (Some(f), Some(t)) => Some(clip(&format!("from {f} to {t}"))),
            (Some(f), None) => Some(clip(&format!("from {f}"))),
            (None, Some(t)) => Some(clip(&format!("to {t}"))),
            (None, None) => None,
        }
        .filter(|a| a != "from any to any");
        out.push(FirewallRule {
            name: format!("pf rule {}", out.len() + 1),
            direction,
            action,
            enabled: true,
            program: None,
            ports: pf_value(&tokens, "port"),
            addresses,
            profile: FirewallProfile::All,
            source: RuleSource::Pf,
            group: None,
            service: None,
            protocol: pf_value(&tokens, "proto"),
        });
        if out.len() > MAX_FIREWALL_RULES + 1024 {
            break;
        }
    }
    out
}

/// `pfctl -s info` plus `pfctl -s rules` as a view.
pub fn pf_view(info: &str, rules: &str) -> Result<FirewallSource, FirewallParseError> {
    let enabled = parse_pf_status(info).ok_or_else(|| FirewallParseError::Unrecognised(first_line(info)))?;
    let mut v = FirewallSource::new(RuleSource::Pf).with_rules(parse_pf_rules(rules));
    v.states.push(ProfileState {
        profile: FirewallProfile::All,
        enabled,
    });
    Ok(v)
}

// ---------------------------------------------------------------- nftables

fn nft_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(clip(s)),
        Value::Number(n) => Some(n.to_string()),
        Value::Object(o) => {
            if let Some(p) = o.get("prefix") {
                return Some(clip(&format!("{}/{}", p["addr"].as_str()?, p["len"].as_u64()?)));
            }
            if let Some(r) = o.get("range").and_then(Value::as_array) {
                return Some(clip(&format!("{}-{}", nft_text(r.first()?)?, nft_text(r.get(1)?)?)));
            }
            if let Some(s) = o.get("set").and_then(Value::as_array) {
                let items: Vec<String> = s.iter().take(32).filter_map(nft_text).collect();
                return Some(clip(&items.join(",")));
            }
            None
        }
        Value::Array(a) => Some(clip(&a.iter().take(32).filter_map(nft_text).collect::<Vec<_>>().join(","))),
        _ => None,
    }
}

fn nft_policy(p: Option<&str>) -> Option<RuleAction> {
    match p {
        Some("accept") => Some(RuleAction::Allow),
        Some("drop" | "reject") => Some(RuleAction::Block),
        _ => None,
    }
}

/// `nft -j list ruleset` as a view: rules of base chains with a hook (`input`, `output`, `forward`) and their policies.
pub fn parse_nft(json: &str) -> Result<FirewallSource, FirewallParseError> {
    if json.len() > MAX_NFT_BYTES {
        return Err(FirewallParseError::TooLarge);
    }
    let v: Value = serde_json::from_str(json).map_err(|_| FirewallParseError::Unrecognised(first_line(json)))?;
    let items = v["nftables"]
        .as_array()
        .ok_or_else(|| FirewallParseError::Unrecognised(first_line(json)))?;
    // First pass: which chains are hooked, and with which policy.
    let mut hooks: Vec<(String, String, String, String, Option<RuleAction>)> = Vec::new();
    let mut view = FirewallSource::new(RuleSource::Nftables);
    for item in items.iter().take(MAX_ITEMS) {
        let Some(c) = item.get("chain") else { continue };
        let Some(hook) = c["hook"].as_str() else { continue };
        let (fam, table, name) = (c["family"].as_str(), c["table"].as_str(), c["name"].as_str());
        let policy = nft_policy(c["policy"].as_str());
        match hook {
            "input" => view.default_in = view.default_in.or(policy),
            "output" => view.default_out = view.default_out.or(policy),
            _ => {}
        }
        if let (Some(f), Some(t), Some(n)) = (fam, table, name) {
            hooks.push((f.into(), t.into(), n.into(), hook.into(), policy));
        }
    }
    let mut rules = Vec::new();
    for item in items.iter().take(MAX_ITEMS) {
        let Some(r) = item.get("rule") else { continue };
        let key = (
            r["family"].as_str().unwrap_or(""),
            r["table"].as_str().unwrap_or(""),
            r["chain"].as_str().unwrap_or(""),
        );
        let Some(hook) = hooks
            .iter()
            .find(|h| (h.0.as_str(), h.1.as_str(), h.2.as_str()) == key)
            .map(|h| h.3.as_str())
        else {
            continue;
        };
        let exprs = r["expr"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let action = exprs.iter().rev().find_map(|e| {
            let o = e.as_object()?;
            if o.contains_key("accept") {
                Some(RuleAction::Allow)
            } else if o.contains_key("drop") || o.contains_key("reject") {
                Some(RuleAction::Block)
            } else {
                None
            }
        });
        let Some(action) = action else { continue };
        let (mut ports, mut addrs, mut protocols) = (Vec::new(), Vec::new(), Vec::<String>::new());
        for e in exprs {
            let m = &e["match"];
            let (left, right) = (&m["left"]["payload"], &m["right"]);
            if let (Some(field), Some(val)) = (left["field"].as_str(), nft_text(right)) {
                let proto = left["protocol"].as_str().unwrap_or("");
                match field {
                    "dport" | "sport" => {
                        ports.push(format!("{proto} {field} {val}"));
                        if !proto.is_empty() && !protocols.iter().any(|p| p == proto) {
                            protocols.push(proto.to_string());
                        }
                    }
                    "saddr" | "daddr" => addrs.push(format!("{field} {val}")),
                    _ => {}
                }
            }
        }
        rules.push(FirewallRule {
            name: clip(&format!(
                "{} {} #{}",
                r["table"].as_str().unwrap_or("?"),
                r["chain"].as_str().unwrap_or("?"),
                r["handle"]
            )),
            direction: match hook {
                "input" => RuleDirection::In,
                "output" => RuleDirection::Out,
                _ => RuleDirection::Any,
            },
            action,
            enabled: true,
            program: None,
            ports: (!ports.is_empty()).then(|| clip(&ports.join("; "))),
            addresses: (!addrs.is_empty()).then(|| clip(&addrs.join("; "))),
            profile: FirewallProfile::All,
            source: RuleSource::Nftables,
            group: None,
            service: None,
            protocol: protocols.into_iter().next(),
        });
        if rules.len() > MAX_FIREWALL_RULES + 1024 {
            break;
        }
    }
    // nftables has no off switch: the firewall counts as on when any base chain filters.
    let filtering = view.default_in == Some(RuleAction::Block) || view.default_out == Some(RuleAction::Block) || !rules.is_empty();
    view.states.push(ProfileState {
        profile: FirewallProfile::All,
        enabled: filtering,
    });
    Ok(view.with_rules(rules))
}

// ---------------------------------------------------------------- Windows

/// The properties of an `INetFwRule` as COM gives them, for the pure mapping below.
#[derive(Debug, Clone, Default)]
pub struct RawWinRule {
    /// `Name`.
    pub name: String,
    /// `Direction`: 1 in, 2 out.
    pub direction: i32,
    /// `Action`: 0 block, 1 allow.
    pub action: i32,
    /// `Enabled`.
    pub enabled: bool,
    /// `Profiles`: a bit mask (1 domain, 2 private, 4 public, 0x7fffffff all).
    pub profiles: i32,
    /// `ApplicationName`.
    pub application: Option<String>,
    /// `LocalPorts`/`RemotePorts` joined by the reader as it likes; `*` means any.
    pub ports: Option<String>,
    /// `RemoteAddresses`; `*` means any.
    pub addresses: Option<String>,
    /// `Grouping`.
    pub group: Option<String>,
    /// `ServiceName`.
    pub service: Option<String>,
    /// `Protocol`: an IANA number, 256 for any.
    pub protocol: Option<i32>,
}

fn win_profile(mask: i32) -> FirewallProfile {
    match mask {
        1 => FirewallProfile::Domain,
        2 => FirewallProfile::Private,
        4 => FirewallProfile::Public,
        _ => FirewallProfile::All,
    }
}

/// The protocol name for an IANA number as `INetFwRule.Protocol` gives it; 256 (any) is none.
fn win_protocol(n: i32) -> Option<String> {
    match n {
        256 => None,
        6 => Some("tcp".into()),
        17 => Some("udp".into()),
        1 => Some("icmpv4".into()),
        58 => Some("icmpv6".into()),
        n if (0..256).contains(&n) => Some(n.to_string()),
        _ => None,
    }
}

/// Maps one Windows rule. Rules of several profiles (not all) are reported for the first profile in the mask.
pub fn map_windows_rule(r: &RawWinRule) -> FirewallRule {
    let any = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty() && *s != "*").map(clip);
    let first = [1, 2, 4]
        .into_iter()
        .find(|b| r.profiles != 0x7fff_ffff && r.profiles & b != 0)
        .unwrap_or(0x7fff_ffff);
    FirewallRule {
        name: clip(&r.name),
        direction: match r.direction {
            1 => RuleDirection::In,
            2 => RuleDirection::Out,
            _ => RuleDirection::Any,
        },
        action: if r.action == 0 { RuleAction::Block } else { RuleAction::Allow },
        enabled: r.enabled,
        program: any(&r.application),
        ports: any(&r.ports),
        addresses: any(&r.addresses),
        profile: win_profile(first),
        source: RuleSource::WindowsFirewall,
        group: any(&r.group),
        service: any(&r.service),
        protocol: r.protocol.and_then(win_protocol),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPS: &str = "Total number of apps = 3 \n\n1 :  /Applications/Example Sync.app\n\t ( Allow incoming connections ) \n\n\
2 :  /usr/libexec/exampled\n\t ( Block incoming connections ) \n\n3 :  /Applications/NoVerdict.app\n";

    #[test]
    fn application_firewall_outputs_become_a_view() {
        let v = alf_view(
            "Firewall is enabled. (State = 1)",
            "Block all DISABLED! ",
            "Stealth mode enabled",
            APPS,
        )
        .unwrap();
        assert_eq!(
            v.states,
            [ProfileState {
                profile: FirewallProfile::All,
                enabled: true
            }]
        );
        assert_eq!(v.switches, [(FirewallSwitch::BlockAll, false), (FirewallSwitch::Stealth, true)]);
        assert_eq!(v.rule_count, 2, "an app with no verdict line is not a rule");
        assert_eq!((v.rules[0].name.as_str(), v.rules[0].action), ("Example Sync", RuleAction::Allow));
        assert_eq!(
            (v.rules[1].program.as_deref(), v.rules[1].action),
            (Some("/usr/libexec/exampled"), RuleAction::Block)
        );
        assert_eq!(parse_alf_global_state("Firewall is disabled. (State = 0)"), Some(false));
        assert_eq!(
            parse_alf_global_state("Firewall is on for specific services (State = 2)"),
            Some(true)
        );
        assert!(alf_view("sfltool: nope", "", "", "").is_err());
    }

    const PF: &str = "No ALTQ support in kernel\nscrub-anchor \"com.apple/*\" all fragment reassemble\nblock drop in all\n\
pass out all flags S/SA keep state\npass in quick on en0 inet proto tcp from 192.0.2.0/24 to any port = ssh flags S/SA keep state\n\
block drop in quick inet6 proto udp from any to { 2001:db8::1 2001:db8::2 } port = 5353\nanchor \"com.apple/*\" all\n";

    #[test]
    fn pf_rules_keep_direction_action_ports_and_addresses() {
        let rules = parse_pf_rules(PF);
        assert_eq!(rules.len(), 4);
        assert_eq!(
            (rules[0].direction, rules[0].action, rules[0].addresses.clone()),
            (RuleDirection::In, RuleAction::Block, None)
        );
        assert_eq!(rules[1].direction, RuleDirection::Out);
        assert_eq!(rules[2].addresses.as_deref(), Some("from 192.0.2.0/24 to any"));
        assert_eq!(rules[2].ports.as_deref(), Some("= ssh"));
        assert_eq!(rules[2].protocol.as_deref(), Some("tcp"));
        assert_eq!(rules[3].addresses.as_deref(), Some("from any to { 2001:db8::1 2001:db8::2 }"));
        assert!(rules.iter().all(|r| r.source == RuleSource::Pf && r.name.starts_with("pf rule ")));
        let v = pf_view("Status: Enabled for 1 days 02:03:04   Debug: Urgent\n", PF).unwrap();
        assert!(v.states[0].enabled);
        assert!(!pf_view("Status: Disabled\n", "").unwrap().states[0].enabled);
        assert!(pf_view("pfctl: /dev/pf: Permission denied", "").is_err());
    }

    const NFT: &str = r#"{"nftables":[{"metainfo":{"version":"1.0.9"}},
{"table":{"family":"inet","name":"filter","handle":1}},
{"chain":{"family":"inet","table":"filter","name":"input","handle":1,"type":"filter","hook":"input","prio":0,"policy":"drop"}},
{"chain":{"family":"inet","table":"filter","name":"output","handle":2,"type":"filter","hook":"output","prio":0,"policy":"accept"}},
{"chain":{"family":"inet","table":"filter","name":"helper","handle":3}},
{"rule":{"family":"inet","table":"filter","chain":"input","handle":4,"expr":[{"match":{"op":"==","left":{"payload":{"protocol":"tcp","field":"dport"}},"right":22}},{"accept":null}]}},
{"rule":{"family":"inet","table":"filter","chain":"input","handle":5,"expr":[{"match":{"op":"==","left":{"payload":{"protocol":"ip","field":"saddr"}},"right":{"prefix":{"addr":"198.51.100.0","len":24}}}},{"drop":null}]}},
{"rule":{"family":"inet","table":"filter","chain":"helper","handle":6,"expr":[{"accept":null}]}},
{"rule":{"family":"inet","table":"filter","chain":"input","handle":7,"expr":[{"counter":{"packets":0,"bytes":0}}]}}]}"#;

    #[test]
    fn nft_hooked_chains_give_policies_and_rules() {
        let v = parse_nft(NFT).unwrap();
        assert_eq!((v.default_in, v.default_out), (Some(RuleAction::Block), Some(RuleAction::Allow)));
        assert!(v.states[0].enabled);
        assert_eq!(v.rule_count, 2, "unhooked chains and rules with no verdict are not listed");
        assert_eq!((v.rules[0].direction, v.rules[0].action), (RuleDirection::In, RuleAction::Allow));
        assert_eq!(v.rules[0].ports.as_deref(), Some("tcp dport 22"));
        assert_eq!(v.rules[0].protocol.as_deref(), Some("tcp"));
        assert_eq!(
            (v.rules[1].action, v.rules[1].addresses.as_deref()),
            (RuleAction::Block, Some("saddr 198.51.100.0/24"))
        );
    }

    #[test]
    fn nft_refuses_the_oversized_and_the_unrecognised_without_panicking() {
        assert_eq!(parse_nft(&" ".repeat(MAX_NFT_BYTES + 1)), Err(FirewallParseError::TooLarge));
        for junk in [
            "",
            "not json",
            "{}",
            r#"{"nftables":7}"#,
            r#"{"nftables":[{"rule":5},{"chain":[]},null]}"#,
        ] {
            let r = parse_nft(junk);
            if let Ok(v) = r {
                assert_eq!(v.rule_count, 0);
            }
        }
        assert!(parse_pf_rules("block").len() == 1 && parse_pf_rules("pass in port").len() == 1);
    }

    #[test]
    fn windows_rule_properties_map_to_the_one_model() {
        let raw = RawWinRule {
            name: "Example allow".into(),
            direction: 2,
            action: 1,
            enabled: true,
            profiles: 2 | 4,
            application: Some(r"C:\Program Files\Example\x.exe".into()),
            ports: Some("*".into()),
            addresses: Some("203.0.113.0/24".into()),
            group: Some("Example Group".into()),
            service: Some("exampled".into()),
            protocol: Some(6),
        };
        let r = map_windows_rule(&raw);
        assert_eq!(
            (r.direction, r.action, r.profile),
            (RuleDirection::Out, RuleAction::Allow, FirewallProfile::Private)
        );
        assert_eq!((r.ports, r.addresses.as_deref()), (None, Some("203.0.113.0/24")));
        assert_eq!(
            (r.group.as_deref(), r.service.as_deref(), r.protocol.as_deref()),
            (Some("Example Group"), Some("exampled"), Some("tcp"))
        );
        assert_eq!(
            map_windows_rule(&RawWinRule {
                protocol: Some(256),
                group: Some("*".into()),
                ..raw.clone()
            })
            .protocol,
            None
        );
        let all = map_windows_rule(&RawWinRule {
            profiles: 0x7fff_ffff,
            action: 0,
            direction: 1,
            ..raw
        });
        assert_eq!(
            (all.profile, all.action, all.direction),
            (FirewallProfile::All, RuleAction::Block, RuleDirection::In)
        );
    }
}
