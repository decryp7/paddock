//! Names and holder lines come from other processes and from files: parsing them never panics.
use paddock_slots::{Announcement, SlotId};

#[test]
fn no_parser_panics_on_hostile_input() {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    paddock_testkit::assert_never_panics(&[b"s1 token-1\n", b"none\n"], |b| {
        Announcement::parse(&text(b))
    });
    paddock_testkit::assert_never_panics(&[b"s1", b"s0", b"s4294967295"], |b| {
        text(b).parse::<SlotId>()
    });
}
