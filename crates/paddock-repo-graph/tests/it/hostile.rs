//! The parsers take untrusted text (git output, `cargo metadata` JSON, lockfiles, manifests): none may panic on any input.
use paddock_repo_graph::{
    Graph, MemoryFiles, lock_diff, npm_graph, parse_log, parse_metadata, relative,
};

#[test]
fn no_parser_panics_on_hostile_input() {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    paddock_testkit::assert_never_panics(&[b"abc\x1ffeat: one\ndef\x1ffix: two\n"], |b| {
        parse_log(&text(b))
    });
    paddock_testkit::assert_never_panics(
        &[br#"{"packages":[{"manifest_path":"/r/Cargo.toml","dependencies":[]}]}"#],
        |b| parse_metadata(&mut Graph::new(), &text(b), "/r", ""),
    );
    paddock_testkit::assert_never_panics(
        &[br#"{"packages":{"":{"name":"x"},"a":{"version":"1"}}}"#],
        |b| lock_diff(Some(&text(b)), Some("{}")),
    );
    paddock_testkit::assert_never_panics(&[br#"{"name":"x","workspaces":["a"]}"#], |b| {
        let files = MemoryFiles::new().with("package.json", &text(b));
        npm_graph(&["package.json".to_string()], &files)
    });
    paddock_testkit::assert_never_panics(&[b"C:\\r\\a\\b", b"/r/a/b"], |b| {
        relative("/r", &text(b))
    });
}
