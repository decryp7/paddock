//! The temporary copies must not outlive the move to `paddock-rules` (note section 3.1).

#[test]
fn the_shared_copy_module_and_the_shared_crate_never_exist_together() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let shared_crate = manifest.lines().any(|l| l.trim_start().starts_with("paddock-rules"));
    let copy_module = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/shared_copy/mod.rs")
        .exists();
    assert!(
        !(shared_crate && copy_module),
        "pacenotes-rules depends on paddock-rules and still has src/shared_copy: delete the module in the same change (C3a)"
    );
    assert!(shared_crate || copy_module, "neither the shared crate nor the copy is there");
}
