//! Gives `paddock-setup.exe` its version resource without a resource compiler (`src/versionres.rs` writes the `.res`).
//! The per-app build sets `PADDOCK_SETUP_PRODUCT` (the app's name); the description is "<App> update". The version is this
//! crate's own (`CARGO_PKG_VERSION`), never the app's, so the program's bytes stay the same across app releases (Dyno 1.1.1 WP-7:
//! a Defender verdict is per hash). Only the VM tests set `PADDOCK_SETUP_TEST_VERSION` (a build that must look like a newer
//! installed app). Other builds (tests, other hosts) get no resource.
//!
//! The link is deterministic (`/Brepro`: lld-link then derives the header time stamp from the contents, not the clock; `/DEBUG:NONE`: no PDB path or GUID)
//! and loads its DLLs from the system folder only (`/DEPENDENTLOADFLAG:0x800`, Dyno 1.1.1 WP-6); the arguments are in `src/linkargs.rs`.

#[path = "src/linkargs.rs"]
#[allow(dead_code)]
mod linkargs;

#[path = "src/versionres.rs"]
#[allow(dead_code)]
mod versionres;

fn main() {
    println!("cargo:rerun-if-env-changed=PADDOCK_SETUP_PRODUCT");
    println!("cargo:rerun-if-env-changed=PADDOCK_SETUP_TEST_VERSION");
    println!("cargo:rerun-if-changed=src/versionres.rs");
    println!("cargo:rerun-if-changed=src/linkargs.rs");
    // `stop-all` calls `paddock_proc::installed` (plan S1). It is compiled in when that module is in the tree and left out
    // otherwise, so the crate builds with `--workspace --all-features` before and after S1 merges (no feature to forget).
    println!("cargo:rustc-check-cfg=cfg(has_proc_installed)");
    let installed = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
        .join("../paddock-proc/src/installed.rs");
    println!("cargo:rerun-if-changed=../paddock-proc/src/installed.rs");
    if installed.exists() {
        println!("cargo:rustc-cfg=has_proc_installed");
    }
    for name in ["IDENTIFIER", "PRODUCT", "MAIN", "UPDATE_KEY"] {
        println!("cargo:rerun-if-env-changed=PADDOCK_SETUP_{name}");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // `/Brepro`, `/DEBUG:NONE` (no PDB and no CodeView record: lld derives the `/Brepro` hash from the PDB too, and a PDB holds
        // the build folder's absolute paths) and `/DEPENDENTLOADFLAG:0x800` (system folder only), see `src/linkargs.rs`.
        for arg in linkargs::msvc_link_args() {
            println!("cargo:rustc-link-arg-bins={arg}");
        }
    }
    let Ok(product) = std::env::var("PADDOCK_SETUP_PRODUCT") else {
        return;
    };
    let version = std::env::var("PADDOCK_SETUP_TEST_VERSION")
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    let info = versionres::VersionInfo {
        description: format!("{product} update"),
        product,
        version,
        company: "decryptology".into(),
        internal_name: "paddock-setup".into(),
        original_filename: "paddock-setup.exe".into(),
        copyright: "Copyright (C) decryptology".into(),
    };
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default())
        .join("paddock-setup.res");
    if std::fs::write(&out, versionres::to_res(&info)).is_ok() {
        println!("cargo:rustc-link-arg-bins={}", out.display());
    } else {
        println!("cargo:warning=paddock-setup: the version resource could not be written");
    }
}
