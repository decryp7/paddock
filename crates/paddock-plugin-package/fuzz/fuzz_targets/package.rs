//! The archive reader on arbitrary bytes: reading never panics, and whatever is accepted lists only files the manifest names,
//! within the limits. Small limits keep each run fast and exercise the bounds.
#![no_main]

use libfuzzer_sys::fuzz_target;
use paddock_plugin_package::archive::{Limits, MANIFEST, Package, SIGNATURE, SUMS};
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    let limits = Limits {
        package: 1 << 20,
        binary: 1 << 18,
        total: 1 << 20,
        entries: 16,
    };
    if let Ok(pkg) = Package::read_with(Cursor::new(data), &limits) {
        for (path, entry) in &pkg.entries {
            assert!(path == MANIFEST || path.starts_with("bin/"), "{path}");
            assert!(path != SUMS && path != SIGNATURE);
            assert!(!path.contains(".."));
            assert!(entry.size <= limits.binary.max(64 * 1024));
        }
    }
});
