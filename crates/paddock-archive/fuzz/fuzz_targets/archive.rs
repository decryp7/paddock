//! Arbitrary bytes as an archive: unpacking never panics, never writes outside the destination, and always leaves either the
//! complete destination or none (no staging folder). The first byte picks the link policy so every path is reached.
#![no_main]

use libfuzzer_sys::fuzz_target;
use paddock_archive::{Layout, Limits, Links, NoControl, Options, extract};

fuzz_target!(|data: &[u8]| {
    let Some((&pick, body)) = data.split_first() else { return };
    let dir = tempfile::tempdir().expect("temp folder");
    let sandbox = dir.path().join("sandbox");
    std::fs::create_dir(&sandbox).expect("sandbox");
    let archive = dir.path().join("a.bin");
    std::fs::write(&archive, body).expect("archive");
    let opts = Options {
        limits: Limits { max_entries: 64, max_file_bytes: 1 << 20, max_total_bytes: 4 << 20, ratio_floor_bytes: 1 << 20, ..Limits::default() },
        links: [Links::Refuse, Links::Skip, Links::Copy, Links::Keep][usize::from(pick) % 4],
        layout: [Layout::Keep, Layout::Strip(1), Layout::Flat { max_depth: 3 }][usize::from(pick >> 2) % 3],
        ..Options::default()
    };
    let dest = sandbox.join("out");
    let ok = extract(&archive, &dest, &opts, &NoControl).is_ok();
    assert_eq!(dest.exists(), ok, "all or nothing");
    let left: Vec<_> = std::fs::read_dir(&sandbox).expect("sandbox").map(|e| e.expect("entry").file_name()).collect();
    assert_eq!(left.len(), usize::from(ok), "no staging left: {left:?}");
    let outside: Vec<_> = std::fs::read_dir(dir.path()).expect("tmp").map(|e| e.expect("entry").file_name()).collect();
    assert_eq!(outside.len(), 2, "nothing escaped the sandbox: {outside:?}");
});
