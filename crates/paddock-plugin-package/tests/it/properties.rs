//! The parsers never panic, whatever the bytes: arbitrary manifest text and archive bytes, and valid packages with bytes flipped
//! or cut off. A changed package either fails to read or still verifies (a flipped byte in a compressed stream may decode to the
//! same content); it is never accepted with content other than what was signed.

use paddock_plugin_package::archive::{Build, build};
use paddock_plugin_package::{Manifest, Package, SecretKey};
use proptest::prelude::*;
use std::io::Cursor;

const MANIFEST: &str = r#"format = 1
[plugin]
id = "com.example.prop"
name = "Prop"
version = "1.0.0"
publisher = "Example"
protocol = "2.0"
binary = "prop"
targets = ["x86_64-pc-windows-msvc"]

[[contributions]]
kind = "tool"
id = "t"
actions = [{ id = "go", writes = true }]

[[permissions]]
permission = "memory"
mib = 256
reason = "buffers"
"#;

fn package(signed: bool) -> (Vec<u8>, String) {
    let t = tempfile::tempdir().unwrap();
    let prog = t.path().join("prop.exe");
    std::fs::write(&prog, vec![7u8; 3000]).unwrap();
    let out = t.path().join("p.paddockplugin");
    let key = SecretKey::generate().unwrap();
    let digest = build(
        Build {
            manifest: Manifest::parse(MANIFEST).unwrap(),
            programs: vec![("x86_64-pc-windows-msvc".into(), &prog)],
            sign: signed.then_some(&key),
        },
        &out,
    )
    .unwrap();
    (std::fs::read(out).unwrap(), digest)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = Package::read(Cursor::new(bytes));
    }

    #[test]
    fn arbitrary_manifest_text_never_panics_and_valid_text_roundtrips(text in "\\PC{0,600}") {
        if let Ok(m) = Manifest::parse(&text) {
            prop_assert_eq!(Manifest::parse(&m.to_toml()).unwrap(), m);
        }
    }

    #[test]
    fn mutated_manifests_never_panic_and_stay_valid_or_refused(at in any::<prop::sample::Index>(), byte in any::<u8>(), cut in any::<prop::sample::Index>()) {
        let mut bytes = MANIFEST.as_bytes().to_vec();
        let i = at.index(bytes.len());
        bytes[i] = byte;
        if let Ok(text) = std::str::from_utf8(&bytes)
            && let Ok(m) = Manifest::parse(text)
        {
            prop_assert!(m.check().is_ok());
            prop_assert_eq!(Manifest::parse(&m.to_toml()).unwrap(), m);
        }
        let _ = Manifest::parse(&MANIFEST[..cut.index(MANIFEST.len())]);
    }

    #[test]
    fn damaged_packages_never_panic_or_verify_other_content(signed in any::<bool>(), flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8), cut in any::<prop::sample::Index>()) {
        let (good, digest) = package(signed);
        let mut bad = good.clone();
        for (i, x) in &flips {
            let i = i.index(bad.len());
            bad[i] ^= x | 1;
        }
        if let Ok(p) = Package::read(Cursor::new(&bad)) {
            // Accepted only with exactly the content that was built.
            prop_assert_eq!(p.digest(), digest.clone());
        }
        let truncated = &good[..cut.index(good.len())];
        let _ = Package::read(Cursor::new(truncated));
    }
}
