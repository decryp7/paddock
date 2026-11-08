//! Embeds an Info.plist into the macOS binary, so its code-signing identifier is `net.decryptology.dyno.helper` and survives
//! Tauri's ad-hoc re-signing (`codesign --force -s -` takes `CFBundleIdentifier` from the embedded plist), as the agent's does
//! (Dyno ADR 0026; the identifier is a release gate of ADR 0027).

fn main() {
    println!("cargo:rerun-if-changed=macos/Info.plist");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let plist = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("macos/Info.plist");
        println!("cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}", plist.display());
    }
}
