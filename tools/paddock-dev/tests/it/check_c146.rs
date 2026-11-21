//! C146: a notify watcher reads its results through paddock-watch's shared mapping. The positive control runs in the host; these hold
//! the site scanner to its edges.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check::c146::{readable, sites};

#[test]
fn a_watcher_without_the_shared_mapping_is_a_site_and_one_with_it_is_not() {
    let bare = "fn a() {\n    let w = notify::recommended_watcher(h)?;\n    let p = notify::PollWatcher::new(h, c)?;\n}\n";
    assert_eq!(sites("apps/x/src/live.rs", bare), [2, 3]);
    let shared = format!("{bare}fn m(r: &R) -> Change {{ NotifySource::change_from(r) }}\n");
    assert!(sites("apps/x/src/live.rs", &shared).is_empty());
    let handler = "struct H;\nimpl notify::EventHandler for H {\n    fn handle_event(&mut self, _: E) {}\n}\n";
    assert_eq!(sites("apps/x/src/h.rs", handler), [2]);
    let debouncer = "fn d() { let d = new_debouncer(Duration::from_secs(1), tx); }\n";
    assert_eq!(sites("apps/x/src/d.rs", debouncer), [1]);
    let backends = "fn b() {\n    INotifyWatcher::new(h, c)?;\n    notify::FsEventWatcher::new(h, c)?;\n    <W as Watcher>::new(h, c)?;\n    <_ as notify::Watcher>::new(h, c)?;\n}\n";
    assert_eq!(sites("apps/x/src/b.rs", backends), [2, 3, 4, 5]);
}

#[test]
fn only_paddock_watchs_mapping_counts_not_any_function_called_change_from() {
    let other = "fn a() {\n    let w = notify::recommended_watcher(move |r| tx.send(mine::change_from(&r)))?;\n}\n";
    assert_eq!(sites("apps/x/src/live.rs", other), [2]);
    let shared = "fn a() {\n    let w = notify::recommended_watcher(move |r| tx.send(paddock_watch::NotifySource::change_from(&r)))?;\n}\n";
    assert!(sites("apps/x/src/live.rs", shared).is_empty());
}

#[test]
fn comments_strings_paddock_watch_and_build_folders_are_not_read() {
    assert!(
        sites(
            "apps/x/src/c.rs",
            "// notify::recommended_watcher(h)\nconst S: &str = \"PollWatcher::new(\";\n"
        )
        .is_empty()
    );
    assert!(!readable("crates/paddock-watch/src/native.rs"));
    assert!(!readable("apps/x/target/debug/build/out.rs"));
    assert!(!readable("apps/x/src/live.ts"));
    assert!(readable("apps/debrief/app/src-tauri/src/live.rs"));
}
