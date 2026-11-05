//! Facet search is the engine's: it looks at every value of the field, not only the most frequent ones the popover
//! can show, so a rare value is found (B3). The store and the in-memory reference answer alike.

use crate::common;

use common::{pair_of, TempDir};
use debrief_core::api::{Facet, FacetRequest};
use debrief_core::ports::{Filter, Query as _};
use debrief_core::Profile;
use std::io::Write;

/// 1,999 values seen twice each and one value seen once, in a log of invented checkout lines.
fn many_values() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new("facet-search");
    let path = dir.0.join("apps-20261014.log");
    let mut f = std::fs::File::create(&path).unwrap();
    let mut line = |n: u32, app: &str| {
        writeln!(
            f,
            "2026-10-14 10:{:02}:{:02}.000 +00:00 [INF] Order {n} captured {{\"Application\": \"{app}\"}}",
            (n / 60) % 60,
            n % 60
        )
        .unwrap();
    };
    let mut n = 0;
    for k in 0..1999 {
        for _ in 0..2 {
            line(n, &format!("svc-{k:04}"));
            n += 1;
        }
    }
    line(n, "Zz-Rarest-Gadget");
    (dir, path)
}

fn facet(p: &common::Pair, search: &str, limit: usize) -> (Facet, Facet) {
    let req = FacetRequest {
        query: Filter::default(),
        field: "Application".into(),
        limit,
        search: search.into(),
    };
    (p.ws.facet(&req).unwrap(), p.duck.facet(&req).unwrap())
}

#[test]
fn a_search_finds_the_rarest_of_two_thousand_values() {
    let (_dir, path) = many_values();
    let p = pair_of(Profile::default_profile(), std::slice::from_ref(&path));
    // Without a search the rare value is beyond the 1,000 values the engine returns.
    let (w, d) = facet(&p, "", 1000);
    assert_eq!((d.distinct, d.total), (2000, 3999));
    assert!(
        d.values.iter().all(|v| v.value != "Zz-Rarest-Gadget"),
        "the rarest value is not among the top 1000"
    );
    for (who, f) in [("workspace", &w), ("store", &d)] {
        assert_eq!(f.values.len(), 1000, "{who}");
    }
    // Searching for it (any case) returns it, with its count, over all values.
    let (w, d) = facet(&p, "  RAREST-gad ", 1000);
    for (who, f) in [("workspace", &w), ("store", &d)] {
        let got: Vec<_> = f.values.iter().map(|v| (v.value.as_str(), v.count)).collect();
        assert_eq!(got, [("Zz-Rarest-Gadget", 1)], "{who}");
        assert_eq!(
            (f.distinct, f.total),
            (1, 3999),
            "{who}: distinct counts the matches, total all values"
        );
    }
}

#[test]
fn a_search_is_text_not_a_pattern() {
    let (_dir, path) = many_values();
    let p = pair_of(Profile::default_profile(), std::slice::from_ref(&path));
    // LIKE wildcards, quotes and backslashes match themselves only; nothing here is in a value.
    for needle in ["svc-%", "s_c-0001", "%", "_", "\\", "'; DROP TABLE events; --", "\""] {
        let (w, d) = facet(&p, needle, 50);
        assert!(w.values.is_empty() && d.values.is_empty(), "{needle:?}");
        assert_eq!((w.distinct, d.distinct), (0, 0), "{needle:?}");
    }
    // A plain part of many values returns the matches in order, within the limit.
    let (w, d) = facet(&p, "svc-000", 5);
    let names = |f: &Facet| f.values.iter().map(|v| v.value.clone()).collect::<Vec<_>>();
    assert_eq!(names(&d), ["svc-0000", "svc-0001", "svc-0002", "svc-0003", "svc-0004"]);
    assert_eq!(names(&w), names(&d));
    assert_eq!((d.distinct, w.distinct), (10, 10));
    // A search longer than the bound is cut, not refused.
    let (_, d) = facet(&p, &"x".repeat(5000), 5);
    assert!(d.values.is_empty());
}
