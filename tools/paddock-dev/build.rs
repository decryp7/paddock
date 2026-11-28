//! Two jobs:
//!
//! 1. Embeds the input hash the wrapper builds the tool for (`PADDOCK_DEV_HASH`), so `./dev self hash` can show that the
//!    binary and the wrapper agree. Without the variable (a plain `cargo build`) nothing is embedded.
//! 2. Writes the two lists that name every check from the files that hold them (user decision 2026-11-28, ER-40): each check is its
//!    own `src/check/c<n>.rs` and declares itself once, in its `impl Check`, so nothing else lists it.
//!    - `check_registry.rs` (included by `src/check/mod.rs`): `pub mod c<n>;` and the registry, one entry per `src/check/c<n>.rs`
//!      in id order, each the unit struct `C<n>` of its module.
//!    - `check_test_mods.rs` (included by `tests/it/main.rs`): `mod check_c<n>;` for every `tests/it/check_c<n>.rs`.
//!
//!    The scan reads file names only (never file text), and the files go to `OUT_DIR`, so no committed list exists for two parallel
//!    branches to conflict on. Why a build script and not `inventory` or `linkme` (ER-37): those are two more dependencies that
//!    gather items by linker sections (a known gap on some targets and under LTO) and still need a `mod` line per file; a
//!    `build.rs` is the standard cargo mechanism, is already here, adds nothing to the lockfile and works the same on every OS
//!    and without a network. Its list of modules is cargo's own rerun-if-changed folder scan.
#[path = "src/check/scan.rs"]
#[allow(dead_code)]
mod scan;

fn main() {
    println!("cargo:rerun-if-env-changed=PADDOCK_DEV_HASH");
    if let Ok(hash) = std::env::var("PADDOCK_DEV_HASH") {
        println!("cargo:rustc-env=PADDOCK_DEV_EMBEDDED_HASH={hash}");
    }

    let manifest = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|e| panic!("build.rs: cargo sets CARGO_MANIFEST_DIR: {e}")),
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap_or_else(|e| panic!("build.rs: cargo sets OUT_DIR: {e}")));
    println!("cargo:rerun-if-changed=src/check");
    println!("cargo:rerun-if-changed=tests/it");
    let write = |name: &str, text: Result<String, String>| {
        let text = text.unwrap_or_else(|e| panic!("build.rs: {e}"));
        std::fs::write(out.join(name), text).unwrap_or_else(|e| panic!("build.rs: cannot write {name}: {e}"));
    };
    write("check_registry.rs", scan::registry_source(&manifest.join("src/check")));
    write("check_test_mods.rs", scan::test_mods_source(&manifest.join("tests/it")));
}
