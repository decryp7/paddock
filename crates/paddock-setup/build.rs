//! Gives `paddock-setup.exe` its version resource without a resource compiler (`src/versionres.rs` writes the `.res`).
//! The per-app build sets `PADDOCK_SETUP_PRODUCT` (the app's name) and `PADDOCK_SETUP_VERSION`; the description is
//! "<App> update". Other builds (tests, other hosts) get no resource.

#[path = "src/versionres.rs"]
#[allow(dead_code)]
mod versionres;

fn main() {
    println!("cargo:rerun-if-env-changed=PADDOCK_SETUP_PRODUCT");
    println!("cargo:rerun-if-env-changed=PADDOCK_SETUP_VERSION");
    println!("cargo:rerun-if-changed=src/versionres.rs");
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
    let Ok(product) = std::env::var("PADDOCK_SETUP_PRODUCT") else {
        return;
    };
    let version = std::env::var("PADDOCK_SETUP_VERSION")
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    let info = versionres::VersionInfo {
        description: format!("{product} update"),
        product,
        version,
    };
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default())
        .join("paddock-setup.res");
    if std::fs::write(&out, versionres::to_res(&info)).is_ok() {
        println!("cargo:rustc-link-arg-bins={}", out.display());
    } else {
        println!("cargo:warning=paddock-setup: the version resource could not be written");
    }
}
