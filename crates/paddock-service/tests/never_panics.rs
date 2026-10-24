//! The OS tools' output is parsed, never trusted: no input makes the parsers panic (conformance check C34).
use paddock_testkit::{corpus, never_panics};

const LAUNCHCTL: &str = "gui/501/net.example.agent = {\n\tstate = running\n\tpid = 4242\n\tlast exit status = 0\n\tprogram = /Applications/Example.app/Contents/MacOS/agent\n}\n";

#[test]
fn the_launchctl_report_parser_survives_hostile_input() {
    let inputs = corpus(LAUNCHCTL.as_bytes());
    let panicked = never_panics(&inputs, |bytes| {
        paddock_service::parse::parse_job(&String::from_utf8_lossy(bytes))
    });
    assert!(
        panicked.is_empty(),
        "panicked on {} input(s)",
        panicked.len()
    );
}
