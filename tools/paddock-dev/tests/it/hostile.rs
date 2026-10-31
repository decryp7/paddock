//! The tool reads untrusted text (inputs.txt, config files, compiler output, the holder's line): none of it may panic.
use paddock_dev::input_hash::{build_rustc, compiler_lines, parse_inputs};
use paddock_dev::notices::{
    licences::allowed_for,
    licences::expression_allowed,
    sources::{Kind, declared_license, lock_production, rust_crates, workspace_production},
    text::{normalise, spdx_ids},
};
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
    // notices: licence expressions, lock files and cargo-about documents of any shape.
    let allowed = allowed_for(Kind::Npm, "x");
    paddock_testkit::assert_never_panics(
        &[b"MIT OR (Apache-2.0 AND", b"((((", b"WITH WITH", b"MIT WITH (", b"\xff\xfe OR \x00"],
        |b| expression_allowed(&text(b), &allowed),
    );
    paddock_testkit::assert_never_panics(&[b"a\r\n b \r\r", "\u{FEFF}\u{1D49C} OR AND".as_bytes()], |b| {
        (normalise(&text(b)), spdx_ids(&text(b)))
    });
    let json = |b: &[u8]| serde_json::from_slice::<serde_json::Value>(b).unwrap_or(serde_json::Value::Null);
    paddock_testkit::assert_never_panics(
        &[
            br#"{"packages":{"ws":{"dependencies":{"a":1}},"node_modules/a":{"dependencies":[1],"link":"x"}}}"#,
            b"[]",
            b"null",
            br#"{"packages":5}"#,
        ],
        |b| {
            let lock = json(b);
            (
                lock_production(&lock),
                workspace_production(&lock, "ws").ok(),
                declared_license(&lock),
            )
        },
    );
    paddock_testkit::assert_never_panics(
        &[
            br#"{"crates":[{"package":{"name":1}}],"licenses":[{"used_by":3}]}"#,
            b"{}",
            b"\"x\"",
        ],
        |b| rust_crates(&[json(b)]),
    );
}
