//! The heartbeat file is read back from disk: no content makes the reader panic (conformance check C36).
use paddock_testkit::{Scratch, assert_never_panics};
use shell_demo_agent::read_heartbeat;

#[test]
fn the_heartbeat_survives_hostile_bytes() {
    let scratch = Scratch::new();
    let data = scratch.work();
    let path = shell_demo_agent::heartbeat_path(&data);
    std::fs::create_dir_all(path.parent().expect("heartbeat folder")).expect("make the folder");
    assert_never_panics(&[br#"{"at":1761350000,"pid":42,"version":"0.1.0"}"#], |b| {
        std::fs::write(&path, b).expect("write the heartbeat");
        let _ = read_heartbeat(&data);
    });
    scratch.finish().expect("nothing left in tmp");
}
