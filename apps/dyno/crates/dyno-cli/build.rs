//! Embeds an Info.plist into the macOS binary, so its code-signing identifier is a dotted one that survives re-signing
//! (`codesign --force -s -` takes `CFBundleIdentifier` from the embedded plist). macOS 27.2 shows a process the whole
//! kernel socket list only when that identifier contains a dot (ADR 0026); a plain cargo binary is signed `name-<hash>`.

fn main() {
    println!("cargo:rerun-if-changed=macos/Info.plist");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let plist = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("macos/Info.plist");
        println!("cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}", plist.display());
    }
}
