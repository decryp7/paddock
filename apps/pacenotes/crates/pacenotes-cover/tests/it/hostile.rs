//! A malformed report is an error, never a panic (C36).

use pacenotes_cover::{Format, ImportContext, import};
use paddock_testkit::{corpus, never_panics};

use crate::support::{COBERTURA, ISTANBUL, JACOCO, LCOV, LLVM, ci_mapper};

#[test]
fn no_reader_panics_on_any_truncation_bit_flip_or_garbage_of_its_own_reports() {
    let mapper = ci_mapper();
    for (seed, format) in [
        (LCOV, Format::Lcov),
        (COBERTURA, Format::Cobertura),
        (JACOCO, Format::JaCoCo),
        (LLVM, Format::LlvmCov),
        (ISTANBUL, Format::Istanbul),
    ] {
        let panicked = never_panics(&corpus(seed.as_bytes()), |bytes| {
            let text = String::from_utf8_lossy(bytes);
            let _ = import(&text, format, &ImportContext::new(&mapper, None));
            let _ = Format::detect(&text);
        });
        assert!(
            panicked.is_empty(),
            "{format:?} panicked on {:?}",
            String::from_utf8_lossy(&panicked[0])
        );
    }
}

#[test]
fn absurd_numbers_and_deep_json_do_not_panic() {
    let mapper = ci_mapper();
    let cx = ImportContext::new(&mapper, None);
    let big = u64::MAX;
    let lcov = format!("SF:/ci/work/repo/a.c\nDA:{big},{big}\nDA:4294967295,18446744073709551615\nDA:7,{big}\nDA:7,{big}\nend_of_record\n");
    let r = import(&lcov, Format::Lcov, &cx).unwrap();
    assert_eq!(r.files.values().next().unwrap().lines[&7], u64::MAX, "hits saturate");
    let deep = format!("{}1{}", "[".repeat(5000), "]".repeat(5000));
    assert!(import(&deep, Format::Istanbul, &cx).is_err() && import(&deep, Format::LlvmCov, &cx).is_err());
    let nested_xml = format!("{}{}", "<a>".repeat(20_000), "</a>".repeat(20_000));
    for format in [Format::Cobertura, Format::JaCoCo] {
        assert!(matches!(
            import(&nested_xml, format, &cx),
            Err(pacenotes_cover::ImportError::Limit {
                what: "XML nesting depth",
                ..
            })
        ));
    }
    // 200 levels are fine, and comments, CDATA and self-closing tags do not count
    let ok = format!(
        "<coverage>{}<!-- <a><a><a> --><![CDATA[<b><b>]]><x/>{}</coverage>",
        "<a>".repeat(200),
        "</a>".repeat(200)
    );
    assert!(import(&ok, Format::Cobertura, &cx).is_ok());
}

/// Every number in `seed` replaced, one at a time, by each value that has broken a reader: the largest of each integer width, one past
/// it, a negative, a float and an absurd exponent. Bit flips never reach these, so this is how the corpus gets there.
fn extreme_numbers(seed: &str) -> Vec<String> {
    let bytes = seed.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            spans.push((start, i));
        } else {
            i += 1;
        }
    }
    let values = [
        "18446744073709551615",
        "18446744073709551616",
        "4294967295",
        "4294967296",
        "-1",
        "0",
        "1.5",
        "1e999",
        "9223372036854775807",
    ];
    let mut out = Vec::new();
    for (from, to) in spans.into_iter().take(60) {
        for v in values {
            out.push(format!("{}{}{}", &seed[..from], v, &seed[to..]));
        }
    }
    out
}

#[test]
fn readers_survive_every_number_of_their_own_reports_replaced_by_an_extreme_value() {
    let mapper = ci_mapper();
    for (seed, format) in [
        (LCOV, Format::Lcov),
        (COBERTURA, Format::Cobertura),
        (JACOCO, Format::JaCoCo),
        (LLVM, Format::LlvmCov),
        (ISTANBUL, Format::Istanbul),
    ] {
        let inputs = extreme_numbers(seed);
        assert!(inputs.len() >= 20, "{format:?}: only {} variants", inputs.len());
        let panicked = never_panics(&inputs, |bytes| {
            let mut cx = ImportContext::new(&mapper, None);
            cx.limits.max_lines_per_file = 20_000;
            cx.limits.max_total_lines = 50_000;
            let _ = import(&String::from_utf8_lossy(bytes), format, &cx);
        });
        assert!(
            panicked.is_empty(),
            "{format:?} panicked on {:?}",
            String::from_utf8_lossy(&panicked[0])
        );
    }
}

#[test]
fn readers_survive_hostile_shapes_deep_nesting_quotes_and_amplifiers() {
    let mapper = ci_mapper();
    let cx = ImportContext::new(&mapper, None);
    let shapes: Vec<(String, Format)> = vec![
        (
            format!("<coverage>{}{}</coverage>", "<a x='>'>".repeat(4000), "</a>".repeat(4000)),
            Format::Cobertura,
        ),
        (format!("<report>{}</report>", "<package name=\"".repeat(2000)), Format::JaCoCo),
        (
            format!(
                "<coverage>{}</coverage>",
                "<class filename=\"a\"><lines><line number=\"1\" hits=\"1\"/></lines></class>".repeat(5000)
            ),
            Format::Cobertura,
        ),
        (
            format!(
                "{{\"data\":[{{\"files\":[{}]}}]}}",
                "{\"filename\":\"/ci/work/repo/a\",\"segments\":[[1,1,1,true,true,false],[900000,1,0,false,false,false]]},".repeat(50)
                    + "{}"
            ),
            Format::LlvmCov,
        ),
        (format!("{}1{}", "[".repeat(3000), "]".repeat(3000)), Format::LlvmCov),
        (format!("{{\"a\":{}}}", "{\"b\":".repeat(3000)), Format::Istanbul),
        (
            "SF:/ci/work/repo/a\n".to_owned() + &"DA:1,1\n".repeat(200_000) + "end_of_record\n",
            Format::Lcov,
        ),
    ];
    for (text, format) in shapes {
        let started = std::thread::spawn({
            let text = text.clone();
            move || {
                let mapper = ci_mapper();
                let mut cx = ImportContext::new(&mapper, None);
                cx.limits.max_total_lines = 100_000;
                let _ = import(&text, format, &cx);
            }
        });
        started.join().unwrap_or_else(|_| panic!("{format:?} panicked on a hostile shape"));
    }
    let _ = cx;
}
