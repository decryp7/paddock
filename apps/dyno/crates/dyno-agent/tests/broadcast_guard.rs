#![allow(clippy::unwrap_used, clippy::expect_used)]

//! `Shared::broadcast` is lossy: it drops a frame for an app whose queue is full. Whole states and events must go through the
//! `broadcast_*` functions (a latest-wins slot or a bounded queue per connection), or a window keeps showing an old state until it asks
//! again (Linux CI lost the Wake-on-LAN list this way). This test fails when a new `Message` goes through plain `broadcast`.

use std::path::{Path, PathBuf};

/// Frames that may use the lossy door because the next tick sends them again.
const RESENT: [&str; 1] = ["Live"];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// The `Message` variants a source text sends through `.broadcast(&Message::<Variant>`, whatever the spacing.
fn broadcast_variants(text: &str) -> Vec<String> {
    let squeezed: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("( ", "(")
        .replace("& ", "&");
    squeezed
        .match_indices(".broadcast(&Message::")
        .map(|(at, m)| {
            squeezed[at + m.len()..]
                .chars()
                .take_while(|c| c.is_alphanumeric())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn only_frames_that_are_resent_go_through_the_lossy_broadcast() {
    let mut files = Vec::new();
    rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    let mut offenders = Vec::new();
    for file in files {
        for variant in broadcast_variants(&std::fs::read_to_string(&file).unwrap()) {
            if !RESENT.contains(&variant.as_str()) {
                offenders.push(format!("{}: Message::{variant}", file.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these go through the lossy `broadcast`, so a full queue drops them and a window stays stale: {offenders:?}. Add a `broadcast_*` \
         function that puts the state into `Pending` (a `LatestSlot`, the alarm slot or the event queue) and call that"
    );
}

#[test]
fn the_guard_sees_a_state_event_sent_the_lossy_way() {
    // Positive control (ER-33): the scan must find these, however they are wrapped.
    let flagged = |text: &str| broadcast_variants(text).into_iter().any(|v| !RESENT.contains(&v.as_str()));
    assert!(flagged("shared.broadcast(&Message::PowerChanged(Box::new(v)));"));
    assert!(flagged("shared\n    .broadcast(\n        &Message::NewThing(1),\n    );"));
    assert!(!flagged("shared.broadcast(&Message::Live(x));"));
    assert!(!flagged("shared.broadcast_power(&v);"));
}
