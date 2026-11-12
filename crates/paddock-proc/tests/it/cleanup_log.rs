//! The warn forms of the clean-up helpers name the path, the OS error and the thread's panic message in the log (ER-27).

use paddock_proc::cleanup;
use std::sync::Mutex;

struct Capture;
static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, r: &log::Record) {
        if let Ok(mut l) = LINES.lock() {
            l.push(format!("{} {}", r.level(), r.args()));
        }
    }
    fn flush(&self) {}
}

#[test]
fn a_leftover_and_a_panicked_thread_are_logged_at_warn_with_their_cause() {
    static C: Capture = Capture;
    log::set_logger(&C).expect("this test binary has no other logger");
    log::set_max_level(log::LevelFilter::Debug);

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plain");
    std::fs::write(&file, b"x").unwrap();
    // a file is not a folder: the OS says why, and the path is named
    cleanup::remove_dir_all_warn(&file);
    // a folder is not a file
    cleanup::remove_file_warn(dir.path());
    // not there is the goal and says nothing
    cleanup::remove_file_warn(&dir.path().join("missing"));
    cleanup::join_warn(
        "the test thread",
        std::thread::spawn(|| panic!("boom from the test")),
    );

    let lines = LINES.lock().unwrap().clone();
    let warns: Vec<&String> = lines.iter().filter(|l| l.starts_with("WARN")).collect();
    assert_eq!(warns.len(), 3, "{lines:?}");
    let file_text = file.display().to_string();
    assert!(
        warns[0].contains(&file_text) && warns[0].contains("left"),
        "{}",
        warns[0]
    );
    assert!(
        warns[0].len() > file_text.len() + 30,
        "an OS error text follows the path: {}",
        warns[0]
    );
    assert!(
        warns[1].contains(&dir.path().display().to_string()),
        "{}",
        warns[1]
    );
    assert!(
        warns[2].contains("the test thread") && warns[2].contains("boom from the test"),
        "{}",
        warns[2]
    );
}
