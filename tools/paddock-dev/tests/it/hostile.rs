//! The tool reads untrusted text (inputs.txt, config files, compiler output, the holder's line): none of it may panic.
use paddock_dev::input_hash::{build_rustc, compiler_lines, parse_inputs};
use paddock_dev::slots::parse_announcement;

#[test]
fn no_parser_panics_on_hostile_input() {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    paddock_testkit::assert_never_panics(&[b"# c\ncrates/a\n tools/x \n", b"/abs\n..\n"], |b| parse_inputs(&text(b)));
    paddock_testkit::assert_never_panics(&[b"[build]\nrustc = \"/x/rustc\"  # c\n", b"build.rustc = \"a\\\"b\"\n"], |b| {
        build_rustc(&text(b))
    });
    paddock_testkit::assert_never_panics(&[b"commit-hash: abc\r\nrelease: 1.99.0\r\n"], |b| compiler_lines(&text(b)));
    paddock_testkit::assert_never_panics(&[b"s1 tok\n", b"none\n"], |b| parse_announcement(&text(b)));
}
