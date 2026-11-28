//! A text reader never panics on any bytes (conformance C36).
use paddock_testkit::{corpus, never_panics};

const GATES: &[u8] = b"version = 1\n[[report_only]]\nid = \"C1\"\napps = [\"a\"]\napproved = \"user\"\n[[ratchet]]\nid = \"C2\"\n[[register_growth]]\ncheck = \"C1\"\nby = \"user\"\ncount = 2\ndigest = \"d\"\n";
const OS_REMOVALS: &[u8] = b"version = 1\n[[approved]]\ncontract = \"c\"\nby = \"user\"\ndate = \"2026-11-27\"\nwhy = \"w\"\nentries = [\"skip a\"]\n";

#[test]
fn the_registers_never_panic_on_a_corpus_of_bad_text() {
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    assert!(
        never_panics(&corpus(GATES), |b| paddock_approvals::parse_gates(&text(b))
            .is_ok())
        .is_empty()
    );
    assert!(
        never_panics(&corpus(OS_REMOVALS), |b| {
            paddock_approvals::parse_os_removals(&text(b)).is_ok()
        })
        .is_empty()
    );
}

#[test]
fn the_trailer_reader_never_panics_on_a_corpus_of_bad_text() {
    const TRAILERS: &[u8] = b"Gates-date: C13 2026-12-06 -> 2026-12-20; approved: coordinator\nSigned-off-by: x\n: ;\nGates-date:\n";
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    assert!(
        never_panics(&corpus(TRAILERS), |b| {
            paddock_approvals::date_move_claim(
                &text(b),
                "Gates-date",
                "C13 2026-12-06 -> 2026-12-20",
            );
            true
        })
        .is_empty()
    );
}
