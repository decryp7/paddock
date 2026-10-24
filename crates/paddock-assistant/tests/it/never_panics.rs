//! A model's output is untrusted text: no input makes the tool-call parser panic (conformance check C36).
use paddock_assistant::protocol::parse_output;
use paddock_testkit::assert_never_panics;

#[test]
fn model_output_survives_hostile_text() {
    let seed = b"I will look.\n<tool_call>{\"name\":\"read\",\"arguments\":{\"path\":\"a.log\"}}</tool_call>";
    assert_never_panics(&[seed, b"```json\n{\"tool\":\"x\"}\n```"], |b| {
        parse_output(&String::from_utf8_lossy(b))
    });
}
