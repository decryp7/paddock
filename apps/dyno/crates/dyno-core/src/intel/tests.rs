use super::*;
use crate::intel_text::{Entry, IpRange, Target};
use proptest::prelude::*;

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

fn list(id: &str, text: &str) -> ListInput {
    ListInput {
        id: id.into(),
        name: format!("List {id}"),
        enabled: true,
        entries: raw(text),
    }
}

/// Entries as typed, one per line, with no deduplication (what a hand-edited list could hold).
fn raw(text: &str) -> Vec<Entry> {
    text.lines()
        .map(|l| {
            let (t, n) = l.split_once('#').map_or((l, None), |(t, n)| (t, Some(n.trim().to_string())));
            Entry {
                target: crate::intel_text::parse_target(t.trim()).unwrap(),
                note: n,
            }
        })
        .collect()
}

fn build(lists: &[ListInput], allow: &str, allow_hosts: &str) -> IntelIndex {
    IntelIndex::build(
        lists,
        &AllowInput {
            addresses: raw(allow),
            hosts: raw(allow_hosts),
        },
    )
    .unwrap()
}

fn flagged(v: &Verdict) -> Vec<(String, String)> {
    match v {
        Verdict::Flagged { matches } => matches.iter().map(|m| (m.list_id.clone(), m.entry.clone())).collect(),
        other => panic!("expected Flagged, got {other:?}"),
    }
}

#[test]
fn an_address_inside_a_range_is_flagged_with_the_note() {
    let ix = build(&[list("a", "8.8.8.0/24 # test range\n1.1.1.1")], "", "");
    let v = ix.check(&ip("8.8.8.8"), None);
    let Verdict::Flagged { matches } = &v else { panic!("{v:?}") };
    assert_eq!(matches[0].entry, "8.8.8.0/24");
    assert_eq!(matches[0].note.as_deref(), Some("test range"));
    assert_eq!(matches[0].by, MatchedBy::Address);
    assert_eq!(ix.check(&ip("8.8.9.1"), None), Verdict::Clean);
    assert_eq!(flagged(&ix.check(&ip("1.1.1.1"), None)).len(), 1);
}

#[test]
fn several_lists_each_report_their_most_specific_entry() {
    let ix = build(
        &[list("a", "8.0.0.0/8\n8.8.0.0/16"), list("b", "8.8.8.0/24"), list("c", "9.9.9.9")],
        "",
        "",
    );
    let got = flagged(&ix.check(&ip("8.8.8.8"), None));
    assert_eq!(got, vec![("a".into(), "8.8.0.0/16".into()), ("b".into(), "8.8.8.0/24".into())]);
    assert_eq!(flagged(&ix.check(&ip("8.1.1.1"), None)), vec![("a".into(), "8.0.0.0/8".into())]);
}

#[test]
fn a_disabled_list_does_not_match() {
    let mut l = list("a", "8.8.8.8");
    l.enabled = false;
    assert_eq!(build(&[l], "", "").check(&ip("8.8.8.8"), None), Verdict::Clean);
}

#[test]
fn private_and_special_addresses_are_never_matched_even_in_a_wide_range() {
    let ix = build(&[list("a", "0.0.0.0/0\n::/0")], "", "");
    for a in [
        "10.0.0.1",
        "192.168.1.1",
        "127.0.0.1",
        "169.254.1.1",
        "100.64.1.1",
        "224.0.0.1",
        "198.18.0.1",
        "192.0.0.1",
        "2002::1",
        "2001::1",
        "100::1",
        "fe80::1",
        "fd00::1",
        "::1",
        "2001:db8::1",
        "::ffff:10.0.0.1",
        "64:ff9b::a00:1",
    ] {
        assert_eq!(ix.check(&ip(a), None), Verdict::Clean, "{a}");
    }
    assert!(matches!(ix.check(&ip("8.8.8.8"), None), Verdict::Flagged { .. }));
    assert!(matches!(ix.check(&ip("2606:4700::1"), None), Verdict::Flagged { .. }));
}

#[test]
fn mapped_and_nat64_addresses_are_matched_as_the_address_inside() {
    let ix = build(&[list("a", "8.8.8.8")], "", "");
    assert!(matches!(ix.check(&ip("::ffff:8.8.8.8"), None), Verdict::Flagged { .. }));
    assert!(matches!(ix.check(&ip("64:ff9b::808:808"), None), Verdict::Flagged { .. }));
    assert_eq!(ix.check(&ip("64:ff9b::909:909"), None), Verdict::Clean);
}

#[test]
fn an_address_allow_overrides_every_list_and_the_narrowest_allow_is_reported() {
    let ix = build(
        &[list("a", "8.8.8.0/24"), list("b", "8.8.8.8")],
        "8.8.0.0/16 # office\n8.8.8.8 # mine",
        "",
    );
    match ix.check(&ip("8.8.8.8"), None) {
        Verdict::Allowed { entry, note } => {
            assert_eq!(entry, "8.8.8.8");
            assert_eq!(note.as_deref(), Some("mine"));
        }
        v => panic!("{v:?}"),
    }
    assert!(matches!(ix.check(&ip("8.8.8.9"), None), Verdict::Allowed { .. }));
}

#[test]
fn host_names_match_by_label_suffix_only() {
    let ix = build(&[list("a", "evil.example # c2")], "", "");
    for n in ["evil.example", "a.evil.example", "A.B.Evil.Example."] {
        assert!(matches!(ix.check(&ip("8.8.8.8"), Some(n)), Verdict::Flagged { .. }), "{n}");
    }
    for n in ["notevil.example", "example", "evil.example.org", ""] {
        assert_eq!(ix.check(&ip("8.8.8.8"), Some(n)), Verdict::Clean, "{n}");
    }
    assert_eq!(ix.check(&ip("8.8.8.8"), None), Verdict::Clean, "no name, no host match");
    let Verdict::Flagged { matches } = ix.check(&ip("8.8.8.8"), Some("a.evil.example")) else {
        panic!()
    };
    assert_eq!(
        matches[0].by,
        MatchedBy::ReverseName {
            name: "a.evil.example".into(),
            label: NAME_LABEL
        }
    );
    assert_eq!(matches[0].note.as_deref(), Some("c2"));
}

#[test]
fn a_host_name_allow_cancels_only_host_name_entries_never_an_address_match() {
    // Spoofing: whoever owns the address range can name it anything, including the name the user allowed.
    let ix = build(&[list("a", "8.8.8.0/24\nevil.example")], "", "update.example");
    let v = ix.check(&ip("8.8.8.8"), Some("update.example"));
    assert_eq!(flagged(&v), vec![("a".into(), "8.8.8.0/24".into())], "the address match stays");
    let v = ix.check(&ip("8.8.8.8"), Some("x.update.example"));
    assert!(matches!(v, Verdict::Flagged { .. }));
    // Cancelling the user's own host name entry:
    let ix = build(&[list("a", "evil.example")], "", "good.evil.example");
    assert_eq!(ix.check(&ip("9.9.9.9"), Some("good.evil.example")), Verdict::Clean);
    assert!(matches!(
        ix.check(&ip("9.9.9.9"), Some("bad.evil.example")),
        Verdict::Flagged { .. }
    ));
}

#[test]
fn a_host_name_in_the_address_allow_list_is_ignored() {
    let ix = build(&[list("a", "8.8.8.8")], "update.example", "");
    assert!(matches!(ix.check(&ip("8.8.8.8"), Some("update.example")), Verdict::Flagged { .. }));
}

#[test]
fn limits_give_clear_messages() {
    let many: Vec<ListInput> = (0..=MAX_LISTS).map(|i| list(&i.to_string(), "8.8.8.8")).collect();
    let e = IntelIndex::build(&many, &AllowInput::default()).unwrap_err();
    assert_eq!(e, IntelError::TooManyLists);
    assert!(e.to_string().contains("32"));
    let big = ListInput {
        id: "x".into(),
        name: "x".into(),
        enabled: true,
        entries: (0..=MAX_TOTAL_ENTRIES)
            .map(|i| Entry {
                target: Target::Host(format!("h{i}.example")),
                note: None,
            })
            .collect(),
    };
    assert_eq!(
        IntelIndex::build(&[big], &AllowInput::default()).unwrap_err(),
        IntelError::TooManyEntries
    );
    let allow = AllowInput {
        addresses: vec![
            Entry {
                target: Target::Range(IpRange::address(&ip("8.8.8.8"))),
                note: None
            };
            MAX_ALLOW + 1
        ],
        hosts: vec![],
    };
    assert_eq!(IntelIndex::build(&[], &allow).unwrap_err(), IntelError::TooManyAllows);
}

#[test]
fn the_whole_address_space_range_has_no_overflow() {
    let ix = build(&[list("a", "0.0.0.0/0\n::/0")], "", "");
    assert_eq!(ix.segment_count(), 3, "::/0 cut around the IPv4 block, plus 0.0.0.0/0");
    assert!(matches!(ix.check(&ip("255.255.255.254"), None), Verdict::Clean));
    assert!(
        matches!(ix.check(&ip("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"), None), Verdict::Clean),
        "multicast is never matched"
    );
}

// --- the index against a naive matcher -------------------------------------------------------------------------------------

fn arb_range() -> impl Strategy<Value = String> {
    prop_oneof![
        // IPv4 in a small public-ish block so ranges overlap and nest.
        (0u8..4, 0u8..4, 0u8..=32).prop_map(|(a, b, p)| format!("{}.{}.0.0/{}", 20 + a, b, p)),
        (0u8..4, 0u8..=255).prop_map(|(a, d)| format!("{}.9.9.{}", 20 + a, d)),
        (0u16..4, 0u8..=64).prop_map(|(a, p)| format!("2606:{:x}::/{}", a, p.max(1))),
        Just("0.0.0.0/0".to_string()),
        Just("::/0".to_string()),
        Just("::ffff:20.0.0.0/104".to_string()),
    ]
}

fn arb_addr() -> impl Strategy<Value = IpAddr> {
    prop_oneof![
        (20u8..24, 0u8..5, 0u8..=255, 0u8..=255).prop_map(|(a, b, c, d)| IpAddr::from([a, b, c, d])),
        (0u16..5, any::<u16>()).prop_map(|(a, b)| IpAddr::from([0x2606, a, 0, 0, 0, 0, 0, b])),
        (20u8..24, 0u8..=255).prop_map(|(a, d)| ip(&format!("::ffff:{a}.9.9.{d}"))),
        (any::<u32>()).prop_map(|n| IpAddr::from(n.to_be_bytes())),
    ]
}

#[test]
fn a_range_in_several_lists_is_stored_once_and_every_list_is_reported() {
    let ix = build(
        &[
            list("a", "8.8.8.0/24 # a-note"),
            list("b", "8.8.8.0/24 # b-note\n8.8.0.0/16"),
            list("c", "8.8.8.0/24"),
        ],
        "",
        "",
    );
    assert_eq!(ix.segment_count(), 3, "8.8.0.0-8.8.7.255, the /24, 8.8.9.0-8.8.255.255");
    let Verdict::Flagged { matches } = ix.check(&ip("8.8.8.8"), None) else {
        panic!()
    };
    let by: Vec<(&str, &str, Option<&str>)> = matches
        .iter()
        .map(|m| (m.list_id.as_str(), m.entry.as_str(), m.note.as_deref()))
        .collect();
    assert_eq!(
        by,
        [
            ("a", "8.8.8.0/24", Some("a-note")),
            ("b", "8.8.8.0/24", Some("b-note")),
            ("c", "8.8.8.0/24", None)
        ]
    );
    let Verdict::Flagged { matches } = ix.check(&ip("8.8.1.1"), None) else {
        panic!()
    };
    assert_eq!(matches.len(), 1, "only list b has the /16");
    assert_eq!(ix.stored_ranges(), 2);
}

proptest! {
    #[test]
    fn matching_with_dedup_equals_matching_without_it(
        a in proptest::collection::vec(arb_range(), 0..25),
        b in proptest::collection::vec(arb_range(), 0..25),
        addrs in proptest::collection::vec(arb_addr(), 1..40),
    ) {
        // Raw entries (parse_list already dedupes, so build them by hand).
        let raw = |id: &str, v: &Vec<String>| ListInput {
            id: id.into(), name: id.into(), enabled: true,
            entries: v.iter().filter_map(|t| crate::intel_text::parse_target(t).ok()).map(|target| Entry { target, note: None }).collect(),
        };
        let without = [raw("a", &a), raw("b", &b)];
        let with: Vec<ListInput> = without.iter().cloned().map(|mut l| { l.entries = crate::intel_text::dedupe(l.entries).entries; l }).collect();
        let x = IntelIndex::build(&without, &AllowInput::default()).unwrap();
        let y = IntelIndex::build(&with, &AllowInput::default()).unwrap();
        let ids = |v: Verdict| match v {
            Verdict::Flagged { matches } => matches.into_iter().map(|m| m.list_id).collect::<std::collections::BTreeSet<_>>(),
            _ => Default::default(),
        };
        for addr in addrs {
            prop_assert_eq!(ids(x.check(&addr, None)), ids(y.check(&addr, None)), "address {}", addr);
        }
    }

    #[test]
    fn the_index_agrees_with_a_linear_matcher(
        a in proptest::collection::vec(arb_range(), 0..25),
        b in proptest::collection::vec(arb_range(), 0..25),
        addrs in proptest::collection::vec(arb_addr(), 1..40),
    ) {
        let lists = [list("a", &a.join("\n")), list("b", &b.join("\n"))];
        let ix = IntelIndex::build(&lists, &AllowInput::default()).unwrap();
        for addr in addrs {
            let got: Vec<(String, String)> = match ix.check(&addr, None) {
                Verdict::Clean => vec![],
                Verdict::Flagged { matches } => matches.into_iter().map(|m| (m.list_id, m.entry)).collect(),
                Verdict::Allowed { .. } => unreachable!(),
            };
            let mut want: Vec<(String, String)> = Vec::new();
            if crate::ip_lookup::is_public(&addr) {
                let p = crate::intel_text::point(&addr);
                for l in &lists {
                    // the most specific containing range: the longest prefix in this family (ties: same text)
                    let best = l.entries.iter().filter_map(|e| match &e.target {
                        Target::Range(r) if r.contains(p) && r.v4 == crate::ip_lookup::unwrapped(&addr).is_ipv4() => Some(*r),
                        _ => None,
                    }).max_by_key(|r| (u128::MAX - (r.end - r.start), r.prefix));
                    if let Some(r) = best { want.push((l.id.clone(), r.to_string())); }
                }
            }
            want.sort();
            let mut got = got; got.sort();
            prop_assert_eq!(got, want, "address {}", addr);
        }
    }
}

/// Rule 48: `cargo test -p dyno-core --release --lib -- --ignored --nocapture bench_index`; the numbers go to docs/performance.md.
#[test]
#[ignore = "benchmark: run by hand in release mode"]
fn bench_index() {
    use std::time::Instant;
    for n in [100_000usize, 1_000_000] {
        // A deterministic spread of /24 .. /16 ranges and single addresses over public space, nested now and then.
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let entries: Vec<Entry> = (0..n)
            .filter_map(|_| {
                let v = next();
                let a = 11 + (v % 100) as u8; // 11..110, public
                let ipv = IpAddr::from([a, (v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8]);
                let prefix = [32u8, 32, 32, 24, 24, 20, 16][(v >> 40) as usize % 7];
                IpRange::cidr(&ipv, prefix).map(|r| Entry {
                    target: Target::Range(r),
                    note: None,
                })
            })
            .collect();
        let lists = [ListInput {
            id: "a".into(),
            name: "a".into(),
            enabled: true,
            entries,
        }];
        let t = Instant::now();
        let ix = IntelIndex::build(&lists, &AllowInput::default()).unwrap();
        let build = t.elapsed();
        let probes: Vec<IpAddr> = (0..200_000)
            .map(|_| {
                let v = next();
                IpAddr::from([11 + (v % 100) as u8, (v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8])
            })
            .collect();
        let mut lat: Vec<u128> = Vec::with_capacity(probes.len());
        let mut hits = 0usize;
        for p in &probes {
            let t = Instant::now();
            if !matches!(ix.check(p, None), Verdict::Clean) {
                hits += 1;
            }
            lat.push(t.elapsed().as_nanos());
        }
        lat.sort_unstable();
        println!(
            "{n} entries: build {build:?}, {} segments, ~{} MB, lookup p50 {} ns, p99 {} ns, {hits} hits of {}",
            ix.segment_count(),
            ix.approx_bytes() / 1_000_000,
            lat[lat.len() / 2],
            lat[lat.len() * 99 / 100],
            probes.len()
        );
    }
}
