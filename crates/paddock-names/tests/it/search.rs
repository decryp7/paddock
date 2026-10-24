//! Rule 20 end to end: Debrief's name cases and Dyno's counter-search cases, on the one engine.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_names::*;
use proptest::prelude::*;

fn cand(name: &str, n: u64) -> Candidate {
    Candidate {
        name: name.into(),
        count: n,
    }
}

/// What Debrief's spelled-out log lines yield: three programs, one written four ways.
fn programs() -> Catalogue {
    let names = [
        cand("CompanyA.Graphic.Builder.exe", 1),
        cand("graphicbuilder.exe", 2),
        cand("GraphicBuilder.exe", 1),
        cand("CompanyA.Tray.Agent.exe", 1),
        cand("CompanyA.Report.Builder.exe", 1),
    ];
    let vocab = vocabulary_from(&names);
    let entries = group(&names, &vocab)
        .iter()
        .map(|g| Entry::from(g).kind("process"))
        .collect();
    Catalogue::with_vocabulary(entries, Style::Path, vocab).unwrap()
}

fn top(c: &Catalogue, q: &str) -> Match {
    let s = c.search(q, 5);
    s.matches
        .first()
        .cloned()
        .unwrap_or_else(|| panic!("nothing for {q:?}"))
}

#[test]
fn one_program_found_however_it_is_written() {
    let c = programs();
    for q in [
        "graphic builder",
        "graphicbuilder",
        "Graphic-Builder.exe",
        "GRAPHIC.BUILDER",
        "graphic_builder_x64",
        "GraphicBuilder_v2.exe",
        "C:\\Program Files\\CompanyA\\GraphicBuilder.exe",
        "the graphic builder",
        "graphic builders",
    ] {
        let s = c.search(q, 5);
        assert_eq!(
            s.matches.first().map(|m| m.id.as_str()),
            Some("graphic builder"),
            "{q}: {s:#?}"
        );
        assert!(s.clarify.is_none(), "{q}");
        assert!(!s.matches[0].why.is_empty());
    }
    assert_eq!(top(&c, "graphic builder").how, How::Exact);
    assert_eq!(top(&c, "graphicbuilder").how, How::Exact);
}

#[test]
fn typos_find_the_name_and_say_so() {
    let c = programs();
    for q in [
        "grafic bilder",
        "graphik builder",
        "grahpic builder",
        "graphicbilder",
    ] {
        let m = top(&c, q);
        assert_eq!(m.id, "graphic builder", "{q}");
        assert_eq!(m.how, How::Typo, "{q}");
        assert!(m.why.contains("close spelling"), "{}", m.why);
    }
    // Two real words that are another thing are not a typo.
    assert!(c.search("great builder", 5).matches.is_empty());
}

#[test]
fn typos_are_used_only_when_nothing_matches_better() {
    let c = programs();
    let s = c.search("builder", 5);
    assert!(s.matches.iter().all(|m| m.how == How::Parts), "{s:#?}");
    let s = c.search("qxzvw", 5);
    assert!(s.matches.is_empty());
}

#[test]
fn a_true_tie_is_a_question_with_the_kind_of_thing() {
    let s = programs().search("builder", 5);
    let q = s.clarify.expect("two builders tie");
    assert!(
        q.question
            .contains("Graphic Builder (process) or Report Builder (process)"),
        "{}",
        q.question
    );
    assert_eq!(
        q.question,
        "By 'builder', do you mean Graphic Builder (process) or Report Builder (process)?"
    );
    assert_eq!(q.options.len(), 2);
    assert!(
        q.options.contains(&"graphic builder".to_string())
            && q.options.contains(&"report builder".to_string())
    );
}

#[test]
fn more_events_win_a_near_tie_but_a_real_gap_is_not_a_question() {
    let c = Catalogue::new(vec![
        Entry::new("a", "alpha.service.exe").weight(1_000_000),
        Entry::new("b", "alphabet.exe").weight(1),
    ])
    .unwrap();
    let s = c.search("alpha", 5);
    assert_eq!(s.matches[0].id, "a");
    assert_eq!(s.matches[0].how, How::Parts);
}

#[test]
fn prefixes_and_parts_rank_in_that_order() {
    let c = Catalogue::new(vec![
        Entry::new("p", "Quantum.Spooler.Service.exe"),
        Entry::new("q", "quantum.exe"),
        Entry::new("r", "spoolsv.exe"),
    ])
    .unwrap();
    let s = c.search("spooler", 5);
    assert_eq!(
        (s.matches[0].id.as_str(), s.matches[0].how),
        ("p", How::Parts)
    );
    let s = c.search("quant spool", 5);
    assert_eq!(
        (s.matches[0].id.as_str(), s.matches[0].how),
        ("p", How::Prefix)
    );
}

#[test]
fn the_users_own_name_outranks_everything_and_never_asks() {
    let c = Catalogue::new(vec![
        Entry::new("a", "alpha.exe"),
        Entry {
            user_aliases: vec!["the renderer".into()],
            ..Entry::new("b", "zorblax.exe")
        },
        Entry::new("c", "renderer.exe"),
    ])
    .unwrap();
    let s = c.search("The Renderer", 5);
    assert_eq!(
        (s.matches[0].id.as_str(), s.matches[0].how),
        ("b", How::UserAlias)
    );
    assert!(s.clarify.is_none());
}

#[test]
fn aliases_match_after_parts_and_before_prefixes() {
    let c = Catalogue::new(vec![Entry {
        aliases: vec!["image editor".into()],
        ..Entry::new("g", "graphicbuilder.exe")
    }])
    .unwrap();
    let m = top(&c, "image editor");
    assert_eq!((m.id.as_str(), m.how), ("g", How::Alias));
    assert!(
        c.search("image edit", 5).matches.is_empty(),
        "an alias is matched whole"
    );
}

#[test]
fn ties_never_cross_ways_and_a_single_match_asks_nothing() {
    let c = programs();
    assert!(c.search("tray agent", 5).clarify.is_none());
    assert!(c.search("tray", 5).clarify.is_none());
}

#[test]
fn an_empty_query_a_symbol_query_and_a_huge_query_are_safe() {
    let c = programs();
    for q in ["", "   ", "...", "///", "\u{0}"] {
        assert_eq!(c.search(q, 5), Search::default(), "{q:?}");
    }
    let huge = "graphic ".repeat(10_000);
    let s = c.search(&huge, 5);
    assert!(s.matches.len() <= 5);
    assert_eq!(
        c.search("graphic builder", 0).matches.len(),
        1,
        "a limit of 0 is raised to 1"
    );
    assert!(c.search("builder", 10_000).matches.len() <= MAX_MATCHES);
}

#[test]
fn non_english_names_are_found() {
    let c = Catalogue::new(vec![
        Entry::new("u", "Über-Dienst.exe"),
        Entry::new("j", "日本語ツール.exe"),
    ])
    .unwrap();
    assert_eq!(top(&c, "über dienst").id, "u");
    assert_eq!(top(&c, "ÜBERDIENST").id, "u");
    assert_eq!(top(&c, "日本語ツール").id, "j");
}

#[test]
fn too_many_entries_are_refused_with_a_message() {
    let entries = vec![Entry::default(); MAX_ENTRIES + 1];
    let Err(e) = Catalogue::new(entries) else {
        panic!("refused")
    };
    assert_eq!(e, NamesError::TooManyEntries(MAX_ENTRIES + 1));
    assert!(e.to_string().contains("narrow the scope"));
}

#[test]
fn oversized_entries_are_cut_not_refused() {
    let big = Entry {
        names: (0..100)
            .map(|i| format!("{}{i}", "n".repeat(1000)))
            .collect(),
        fields: vec!["f".into(); 100],
        aliases: vec!["a".into(); 100],
        user_aliases: vec!["u".into(); 100],
        ..Entry::new("big", "x")
    };
    let c = Catalogue::new(vec![big]).unwrap();
    assert_eq!(c.len(), 1);
    assert!(!c.is_empty());
    assert_eq!(
        top(&c, &"n".repeat(MAX_NAME_CHARS)).how,
        How::Exact,
        "names are cut at {MAX_NAME_CHARS} characters"
    );
    assert!(c.vocabulary().knows("graphic"));
}

#[test]
fn entries_round_trip_through_json_with_defaults() {
    let e: Entry = serde_json::from_str(r#"{"id":"x","names":["a.exe"]}"#).unwrap();
    assert_eq!(e, Entry::new("x", "a.exe"));
    let s = programs().search("graphic builder", 1);
    let again: Search = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(again, s);
    assert_eq!(How::Typo.id(), "typo");
    assert_eq!(
        [
            How::UserAlias,
            How::Exact,
            How::Joined,
            How::Parts,
            How::Alias,
            How::Prefix,
            How::Words,
            How::Typo
        ]
        .map(How::id),
        [
            "userAlias",
            "exact",
            "joined",
            "parts",
            "alias",
            "prefix",
            "words",
            "typo"
        ]
    );
}

// --- Dyno's counter search -------------------------------------------------------------------------------------------

/// One entry per counter and instance, as the Dyno catalogue would feed them.
fn counters() -> Catalogue {
    let mut v = Vec::new();
    for (object, counter, instances) in [
        ("Memory", "Pages/sec", vec![]),
        ("Memory", "Available MBytes", vec![]),
        (
            "Network Interface",
            "Bytes Received/sec",
            vec!["en0", "lo0"],
        ),
        ("Network Interface", "Bytes Sent/sec", vec!["en0", "lo0"]),
        ("Processor", "% Processor Time", vec!["0", "1", "_Total"]),
    ] {
        let instances: Vec<Option<&str>> = if instances.is_empty() {
            vec![None]
        } else {
            instances.into_iter().map(Some).collect()
        };
        for i in instances {
            let path = match i {
                Some(i) => format!("\\{object}({i})\\{counter}"),
                None => format!("\\{object}\\{counter}"),
            };
            let mut fields = vec![object];
            fields.extend(i);
            v.push(Entry::new(path, counter).fields(&fields));
        }
    }
    Catalogue::text(v).unwrap()
}

fn ids(c: &Catalogue, q: &str, limit: usize) -> Vec<String> {
    c.search(q, limit)
        .matches
        .into_iter()
        .map(|m| m.id)
        .collect()
}

#[test]
fn counters_every_word_in_any_order_ignoring_case() {
    let c = counters();
    assert_eq!(ids(&c, "MEMORY pages", 10), ["\\Memory\\Pages/sec"]);
    assert_eq!(ids(&c, "pages memory", 10), ["\\Memory\\Pages/sec"]);
    assert!(ids(&c, "pages nothing", 10).is_empty());
    assert!(ids(&c, "   ", 10).is_empty());
}

#[test]
fn counters_one_hit_per_instance_and_the_instance_is_searchable() {
    let c = counters();
    let mut got = ids(&c, "bytes received", 10);
    got.sort();
    assert_eq!(
        got,
        [
            "\\Network Interface(en0)\\Bytes Received/sec",
            "\\Network Interface(lo0)\\Bytes Received/sec"
        ]
    );
    assert_eq!(
        ids(&c, "received lo0", 10),
        ["\\Network Interface(lo0)\\Bytes Received/sec"]
    );
    assert_eq!(
        ids(&c, "network en0 sent", 10),
        ["\\Network Interface(en0)\\Bytes Sent/sec"]
    );
}

#[test]
fn counters_name_matches_rank_before_object_matches() {
    let c = counters();
    let s = c.search("processor", 10);
    assert_eq!(s.matches[0].id, "\\Processor(0)\\% Processor Time");
    let memory = ids(&c, "memory", 10);
    assert_eq!(memory.len(), 2);
    assert!(memory.iter().all(|p| p.starts_with("\\Memory")));
    // The counter-name hit beats an object-only hit.
    let s = c.search("pages", 10);
    assert_eq!(s.matches[0].id, "\\Memory\\Pages/sec");
}

#[test]
fn counters_are_found_through_typos_plurals_and_separators() {
    let c = counters();
    assert_eq!(
        top(&c, "procesor time").id,
        "\\Processor(0)\\% Processor Time"
    );
    assert_eq!(
        top(&c, "bytes recieved lo0").id,
        "\\Network Interface(lo0)\\Bytes Received/sec"
    );
    assert_eq!(top(&c, "pages sec").how, How::Exact);
    assert_eq!(top(&c, "pagessec").how, How::Exact);
    assert_eq!(
        ids(&c, "Bytes Received/sec lo0", 3),
        ["\\Network Interface(lo0)\\Bytes Received/sec"]
    );
}

#[test]
fn counters_results_are_capped() {
    let c = counters();
    assert_eq!(ids(&c, "e", 2).len(), 2);
    assert_eq!(ids(&c, "e", 0).len(), 1);
    let many: Vec<Entry> = (0..400)
        .map(|i| Entry::new(format!("id{i:03}"), "x").fields(&["x"]))
        .collect();
    let big = Catalogue::text(many).unwrap();
    assert_eq!(big.search("x", 10_000).matches.len(), MAX_MATCHES);
    let first: Vec<String> = ids(&big, "x", 10);
    assert_eq!(
        first,
        (0..10).map(|i| format!("id{i:03}")).collect::<Vec<_>>(),
        "ties break by id"
    );
}

#[test]
fn text_style_keeps_slashes_digits_and_extensions() {
    let n = normalise_as(
        "Pages/sec",
        Style::Text,
        &Vocabulary::builtin(),
        &Default::default(),
    );
    assert_eq!(n.key(), "pages sec");
    let n = normalise_as(
        "lo0",
        Style::Text,
        &Vocabulary::builtin(),
        &Default::default(),
    );
    assert_eq!(n.key(), "lo 0");
    let n = normalise_as(
        "config.json",
        Style::Text,
        &Vocabulary::builtin(),
        &Default::default(),
    );
    assert_eq!(n.key(), "config json");
}

// --- grouping (Debrief) ------------------------------------------------------------------------------------------------

#[test]
fn spellings_group_into_one_entity() {
    let c = [
        cand("CompanyA.Graphic.Builder.exe", 5),
        cand("graphicbuilder.exe", 12),
        cand("GraphicBuilder.exe", 3),
        cand("graphic-builder", 1),
        cand("CompanyA.Tray.Agent.exe", 7),
        cand("other.exe", 2),
    ];
    let g = group(&c, &Vocabulary::builtin());
    let gb = g.iter().find(|g| g.key == "graphic builder").unwrap();
    assert_eq!(gb.count, 21);
    assert_eq!(gb.canonical(), "graphicbuilder.exe");
    assert_eq!(gb.spellings.len(), 4);
    assert_eq!(gb.display, "Graphic Builder");
    assert!(g.iter().any(|g| g.key == "tray agent"));
    assert_eq!(g.len(), 3);
    assert!(
        g.windows(2).all(|w| w[0].count >= w[1].count),
        "most events first"
    );
}

#[test]
fn a_single_vendor_prefix_joins_the_group_without_it_but_a_known_first_word_does_not() {
    let c = [
        cand("CompanyA.Graphic.Builder.exe", 5),
        cand("graphicbuilder.exe", 12),
    ];
    let g = group(&c, &Vocabulary::builtin());
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].spellings.len(), 2);
    let c = [
        cand("Graphic.Builder.Core.exe", 5),
        cand("builder.core.exe", 12),
    ];
    assert_eq!(group(&c, &Vocabulary::builtin()).len(), 2);
}

#[test]
fn absorbing_a_group_adds_the_counts_of_a_shared_spelling() {
    let v = Vocabulary::builtin();
    let mut a = group(&[cand("graphicbuilder.exe", 4)], &v).remove(0);
    let b = group(
        &[
            cand("graphicbuilder.exe", 6),
            cand("Graphic.Builder.exe", 1),
        ],
        &v,
    )
    .remove(0);
    a.absorb(b);
    assert_eq!(a.spellings[0].count, 10);
    assert_eq!(a.spellings.len(), 2);
    assert_eq!(a.count, 11);
    let empty = Group {
        key: "k".into(),
        display: "K".into(),
        spellings: vec![],
        count: 0,
    };
    assert_eq!(empty.canonical(), "K");
}

#[test]
fn grouping_skips_names_without_letters_and_the_vocabulary_learns_separated_words_only() {
    let g = group(
        &[cand("....", 3), cand("graphicbuilder.exe", 1)],
        &Vocabulary::builtin(),
    );
    assert_eq!(g.len(), 1);
    let v = vocabulary_from(&[
        cand("Acme.Quasar.Hub.exe", 9),
        cand("zorblaxfoo.exe", 50),
        cand("a-b.exe", 1),
    ]);
    assert!(v.knows("quasar") && v.knows("acme") && !v.knows("zorblaxfoo"));
}

#[test]
fn many_spellings_are_capped() {
    let c: Vec<Candidate> = (0..100)
        .map(|i| cand(&format!("graphic.builder{}.exe", "x".repeat(i)), 1))
        .collect();
    let g = group(&c, &Vocabulary::builtin());
    assert!(g.iter().all(|g| g.spellings.len() <= MAX_SPELLINGS));
}

// --- properties ---------------------------------------------------------------------------------------------------------

proptest! {
    #[test]
    fn search_never_panics_is_bounded_and_deterministic(q in ".{0,400}", limit in 0usize..100) {
        let c = programs();
        let a = c.search(&q, limit);
        prop_assert!(a.matches.len() <= MAX_MATCHES && a.matches.len() <= limit.max(1));
        prop_assert_eq!(&a, &c.search(&q, limit));
        prop_assert!(a.matches.windows(2).all(|w| w[0].score >= w[1].score));
        if let Some(cl) = &a.clarify {
            prop_assert!(cl.options.len() >= 2 && cl.options.len() <= MAX_OPTIONS);
        }
    }

    #[test]
    fn building_never_panics_and_every_entry_finds_itself(names in proptest::collection::vec("[A-Za-z0-9 ._/\\\\-]{1,40}", 1..20)) {
        let entries: Vec<Entry> = names.iter().enumerate().map(|(i, n)| Entry::new(format!("{i:03}"), n.clone())).collect();
        let c = Catalogue::new(entries).unwrap();
        for n in &names {
            if squash(n).is_empty() { continue; }
            let s = c.search(n, MAX_MATCHES);
            prop_assert!(!s.matches.is_empty(), "{n:?} finds nothing");
            prop_assert!(s.matches.iter().any(|m| m.how <= How::Joined), "{n:?}: {:?}", s.matches);
        }
    }

    #[test]
    fn grouping_and_normalising_never_panic(names in proptest::collection::vec(".{0,60}", 0..30)) {
        let cands: Vec<Candidate> = names.iter().map(|n| cand(n, 1)).collect();
        let v = vocabulary_from(&cands);
        let g = group(&cands, &v);
        prop_assert!(g.windows(2).all(|w| w[0].count > w[1].count || (w[0].count == w[1].count && w[0].key <= w[1].key)));
        for n in &names {
            let _ = normalise_as(n, Style::Text, &v, &Default::default());
            prop_assert_eq!(edit_distance_capped(n, n, 0), Some(0));
        }
    }
}

#[test]
fn edit_distance_counts_swaps_and_respects_the_cap() {
    assert_eq!(edit_distance_capped("graphic", "grafic", 2), Some(2));
    assert_eq!(edit_distance_capped("builder", "bilder", 1), Some(1));
    assert_eq!(
        edit_distance_capped("abcd", "abdc", 1),
        Some(1),
        "a swap of neighbours is one"
    );
    assert_eq!(edit_distance_capped("abc", "xyz", 2), None);
    assert_eq!(edit_distance_capped("a", "abcdef", 2), None);
    assert_eq!(
        (allowed_typos(3), allowed_typos(8), allowed_typos(20)),
        (1, 2, 5)
    );
}

#[test]
fn joined_says_whether_the_name_was_run_together() {
    let c = Catalogue::new(vec![
        Entry::new("one", "gfxbuilder_v2.exe"),
        Entry::new("two", "graphic_builder_2.exe"),
    ])
    .unwrap();
    let m = top(&c, "gfx builder");
    assert_eq!((m.id.as_str(), m.how), ("one", How::Joined));
    assert!(m.why.contains("one word"), "{}", m.why);
    let m = top(&c, "graphic builder");
    assert_eq!((m.id.as_str(), m.how), ("two", How::Joined));
    assert!(m.why.contains("spelled differently"), "{}", m.why);
}

#[test]
fn plurals_inside_words_and_spelled_names_are_found() {
    let c = Catalogue::new(vec![
        Entry::new("v", "Graphic.Viewer.exe").spelled(&["graphicviewer.exe"]),
    ])
    .unwrap();
    assert_eq!(top(&c, "graphic viewers").id, "v");
    let t = Catalogue::text(vec![Entry::new("p", "Bytes Received/sec")]).unwrap();
    let m = top(&t, "ytes");
    assert_eq!(m.how, How::Words);
    assert!(
        t.search("by", 5)
            .matches
            .iter()
            .all(|m| m.how == How::Prefix),
        "two letters must start a word"
    );
    assert!(
        t.search("yt", 5).matches.is_empty(),
        "two letters are not found inside a word"
    );
}

// --- the index never changes a result -----------------------------------------------------------------------------------

#[test]
fn the_index_gives_the_same_answers_as_ranking_every_entry() {
    let queries = [
        "graphic builder",
        "graphicbuilder",
        "grafic bilder",
        "builder",
        "tray",
        "tray agent",
        "report",
        "xyz",
        "the graphic builders",
        "gra bui",
        "ilder",
        "e",
        "ab",
        "memory pages",
        "bytes recieved lo0",
        "lo0",
        "processor",
        "ytes",
        "procesor time",
        "Bytes Received/sec",
        "",
        "...",
        "great builder",
    ];
    for c in [programs(), counters()] {
        for q in queries {
            assert_eq!(c.search(q, 20), c.search_scan(q, 20), "{q:?}");
        }
    }
}

proptest! {
    #[test]
    fn indexed_and_scanned_searches_agree_on_random_catalogues(
        names in proptest::collection::vec("[a-z]{1,9}([ ._-][a-z]{1,9}){0,3}", 1..40),
        fields in proptest::collection::vec("[a-z0-9]{1,6}", 0..3),
        q in "[a-z]{1,9}([ ._-][a-z]{1,9}){0,2}",
        text in any::<bool>(),
    ) {
        let entries: Vec<Entry> = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let f: Vec<&str> = fields.iter().map(String::as_str).collect();
                let mut e = Entry::new(format!("{i:03}"), n.clone()).fields(&f).weight(i as u64);
                if i % 5 == 0 { e.aliases.push(n.replace(' ', "")); }
                if i % 7 == 0 { e.user_aliases.push(format!("my {n}")); }
                e
            })
            .collect();
        let c = if text { Catalogue::text(entries).unwrap() } else { Catalogue::new(entries).unwrap() };
        prop_assert_eq!(c.search(&q, 30), c.search_scan(&q, 30));
        let first = names[0].clone();
        prop_assert_eq!(c.search(&first, 30), c.search_scan(&first, 30));
        let mangled = format!("{}x", first.replace(' ', ""));
        prop_assert_eq!(c.search(&mangled, 30), c.search_scan(&mangled, 30));
    }
}

// --- the names_search command --------------------------------------------------------------------------------------------

#[test]
fn the_service_validates_swaps_and_answers() {
    let svc = NamesService::new();
    let req = |q: &str, limit| NamesSearchRequest {
        query: q.into(),
        limit,
    };
    let empty = svc.search(&req("graphic", None)).unwrap();
    assert_eq!(
        (empty.matches.len(), empty.searchable),
        (0, 0),
        "nothing to search before the first catalogue"
    );
    let entries: Vec<Entry> = programs_entries();
    assert_eq!(svc.replace(entries, Style::Path), Ok(3));
    let r = svc.search(&req("grafic bilder", Some(5))).unwrap();
    assert_eq!(
        (r.matches[0].id.as_str(), r.searchable),
        ("graphic builder", 3)
    );
    assert!(svc.search(&req("builder", None)).unwrap().clarify.is_some());
    assert_eq!(
        svc.search(&req(&"x".repeat(MAX_TEXT_CHARS + 1), None)),
        Err(RequestError::QueryTooLong)
    );
    for bad in [0, MAX_MATCHES + 1] {
        assert_eq!(
            svc.search(&req("x", Some(bad))),
            Err(RequestError::BadLimit)
        );
    }
    assert!(
        RequestError::QueryTooLong.to_string().contains("256")
            && RequestError::BadLimit.to_string().contains("50")
    );
    // A refused rebuild keeps the old catalogue.
    assert!(
        svc.replace(vec![Entry::default(); MAX_ENTRIES + 1], Style::Text)
            .is_err()
    );
    assert_eq!(svc.search(&req("tray", None)).unwrap().searchable, 3);
    assert_eq!(
        svc.replace(vec![Entry::new("a", "Bytes/sec")], Style::Text),
        Ok(1)
    );
    assert_eq!(svc.search(&req("bytes", None)).unwrap().matches[0].id, "a");
}

#[test]
fn the_command_shapes_are_camel_case_json_and_refuse_unknown_fields() {
    assert_eq!(NAMES_SEARCH, "names_search");
    let r: NamesSearchRequest = serde_json::from_str(r#"{"query":"x","limit":3}"#).unwrap();
    assert_eq!(r.limit, Some(3));
    assert_eq!(
        serde_json::from_str::<NamesSearchRequest>(r#"{"query":"x"}"#)
            .unwrap()
            .limit,
        None
    );
    assert!(serde_json::from_str::<NamesSearchRequest>(r#"{"query":"x","scope":"all"}"#).is_err());
    let svc = NamesService::new();
    svc.replace(programs_entries(), Style::Path).unwrap();
    let reply = svc
        .search(&NamesSearchRequest {
            query: "builder".into(),
            limit: None,
        })
        .unwrap();
    let json = serde_json::to_value(&reply).unwrap();
    assert!(
        json["searchable"].is_number()
            && json["matches"][0]["how"].is_string()
            && json["clarify"]["options"].is_array()
    );
}

fn programs_entries() -> Vec<Entry> {
    let names = [
        cand("CompanyA.Graphic.Builder.exe", 1),
        cand("graphicbuilder.exe", 2),
        cand("CompanyA.Tray.Agent.exe", 1),
        cand("CompanyA.Report.Builder.exe", 1),
    ];
    let vocab = vocabulary_from(&names);
    group(&names, &vocab)
        .iter()
        .map(|g| Entry::from(g).kind("process"))
        .collect()
}
