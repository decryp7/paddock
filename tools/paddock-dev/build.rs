//! Embeds the input hash the wrapper builds the tool for (`PADDOCK_DEV_HASH`), so `./dev self hash` can show that the
//! binary and the wrapper agree. Without the variable (a plain `cargo build`) nothing is embedded.
fn main() {
    println!("cargo:rerun-if-env-changed=PADDOCK_DEV_HASH");
    if let Ok(hash) = std::env::var("PADDOCK_DEV_HASH") {
        println!("cargo:rustc-env=PADDOCK_DEV_EMBEDDED_HASH={hash}");
    }
}
