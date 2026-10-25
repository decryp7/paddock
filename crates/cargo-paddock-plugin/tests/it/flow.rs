//! `new`, real `cargo build`, `pack`, `keygen`, `sign` and `verify` on a project made in a temp folder.

use crate::support::{cli, tamper};
use paddock_cli::Exit;
use paddock_plugin_package::manifest::host_target;

/// A project with a program that builds in a second and needs no network: the real cargo path (`build`) is the point here,
/// not the template's dependencies (those need the registry or a checkout, and are covered by the template's own tests).
fn tiny_project(dir: &std::path::Path) -> std::path::PathBuf {
    let root = dir.join("tiny");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"tiny\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    let manifest = paddock_plugin_test::fixture::manifest_toml("2.0", "tiny", "");
    std::fs::write(root.join("manifest.toml"), manifest).unwrap();
    root
}

#[test]
fn new_makes_a_project_whose_manifest_and_dependencies_are_right() {
    let dir = tempfile::tempdir().unwrap();
    let r = cli(
        dir.path(),
        &["new", "hello-plugin", "--id", "com.example.hello", "--json"],
    );
    assert_eq!(r.exit, Exit::Ok, "{}", r.err);
    let v = r.json();
    assert_eq!(v["id"], "com.example.hello");
    let root = dir.path().join("hello-plugin");
    let cargo: toml::Table = std::fs::read_to_string(root.join("Cargo.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let deps: Vec<_> = cargo["dependencies"]
        .as_table()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(deps, ["paddock-plugin-api", "serde_json"]);
    // The next command finds the project by its manifest, which names the program after the crate.
    let m = std::fs::read_to_string(root.join("manifest.toml")).unwrap();
    assert!(m.contains("binary = \"hello-plugin\""));
    let main = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
    assert!(main.contains("paddock_plugin_api::serve("));
}

#[test]
fn build_pack_sign_and_verify_with_the_real_cargo() {
    let dir = tempfile::tempdir().unwrap();
    let root = tiny_project(dir.path());
    let project = root.to_str().unwrap();

    // Not built yet: pack says how to build.
    let r = cli(dir.path(), &["pack", "--dir", project, "--json"]);
    assert_eq!(r.exit, Exit::Failure);
    assert!(r.json()["hint"].as_str().unwrap().contains("build"));

    let r = cli(dir.path(), &["build", "--dir", project, "--json", "-q"]);
    assert_eq!(r.exit, Exit::Ok, "{}", r.err);
    let program = std::path::PathBuf::from(r.json()["programs"][0]["path"].as_str().unwrap());
    assert!(program.is_file() && program.starts_with(root.join("target")));

    // Deterministic and unsigned.
    let out_a = dir.path().join("a.paddockplugin");
    let out_b = dir.path().join("b.paddockplugin");
    let a = cli(
        dir.path(),
        &[
            "pack",
            "--dir",
            project,
            "--out",
            out_a.to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(a.exit, Exit::Ok, "{}", a.err);
    cli(
        dir.path(),
        &[
            "pack",
            "--dir",
            project,
            "--out",
            out_b.to_str().unwrap(),
            "-q",
        ],
    );
    assert_eq!(
        std::fs::read(&out_a).unwrap(),
        std::fs::read(&out_b).unwrap()
    );
    assert_eq!(a.json()["signed"], false);

    // Keys: private file, never overwritten.
    let key_file = dir.path().join("me.key");
    let k = cli(
        dir.path(),
        &["keygen", "--out", key_file.to_str().unwrap(), "--json"],
    );
    assert_eq!(k.exit, Exit::Ok, "{}", k.err);
    let public = k.json()["publicKey"].as_str().unwrap().to_string();
    let again = cli(dir.path(), &["keygen", "--out", key_file.to_str().unwrap()]);
    assert_eq!(again.exit, Exit::Failure);
    assert!(again.err.contains("already exists"));
    let stranger_file = dir.path().join("stranger.key");
    let stranger = cli(
        dir.path(),
        &["keygen", "--out", stranger_file.to_str().unwrap(), "--json"],
    );
    let stranger_public = stranger.json()["publicKey"].as_str().unwrap().to_string();

    // Signed at pack time, and by `sign` afterwards: the same package either way.
    let signed = dir.path().join("signed.paddockplugin");
    let p = cli(
        dir.path(),
        &[
            "pack",
            "--dir",
            project,
            "--key",
            key_file.to_str().unwrap(),
            "--out",
            signed.to_str().unwrap(),
            "-q",
        ],
    );
    assert_eq!(p.exit, Exit::Ok, "{}", p.err);
    let later = dir.path().join("later.paddockplugin");
    let s = cli(
        dir.path(),
        &[
            "sign",
            out_a.to_str().unwrap(),
            "--key",
            key_file.to_str().unwrap(),
            "--out",
            later.to_str().unwrap(),
            "-q",
        ],
    );
    assert_eq!(s.exit, Exit::Ok, "{}", s.err);
    assert_eq!(
        std::fs::read(&signed).unwrap(),
        std::fs::read(&later).unwrap()
    );

    // Verify: good, wrong key, unsigned when a key is required, tampered.
    let good = cli(
        dir.path(),
        &[
            "verify",
            signed.to_str().unwrap(),
            "--trust",
            &public,
            "--json",
        ],
    );
    assert_eq!(good.exit, Exit::Ok, "{}", good.err);
    let v = good.json();
    assert_eq!(
        (v["signature"].as_str(), v["trusted"].as_bool()),
        (Some("signed"), Some(true))
    );
    assert_eq!(v["targets"][0], host_target());
    let wrong = cli(
        dir.path(),
        &[
            "verify",
            signed.to_str().unwrap(),
            "--trust",
            &stranger_public,
            "--json",
        ],
    );
    assert_eq!(wrong.exit, Exit::Refused);
    assert_eq!(wrong.json()["ok"], false);
    let unsigned = cli(
        dir.path(),
        &["verify", out_a.to_str().unwrap(), "--trust", &public],
    );
    assert_eq!(unsigned.exit, Exit::Refused);
    assert!(unsigned.err.contains("not signed"), "{}", unsigned.err);
    let plain = cli(dir.path(), &["verify", out_a.to_str().unwrap()]);
    assert_eq!(plain.exit, Exit::Ok);
    assert!(plain.out.contains("not signed"));

    let bad = dir.path().join("tampered.paddockplugin");
    tamper(&signed, &bad);
    let t = cli(
        dir.path(),
        &[
            "verify",
            bad.to_str().unwrap(),
            "--trust",
            &public,
            "--json",
        ],
    );
    assert_eq!(t.exit, Exit::Refused, "{}{}", t.out, t.err);
    assert!(
        t.json()["error"]
            .as_str()
            .unwrap()
            .contains("changed after it was signed")
    );
}

#[test]
fn a_program_that_does_not_compile_fails_the_build_with_cargos_messages_and_a_next_step() {
    let dir = tempfile::tempdir().unwrap();
    let root = tiny_project(dir.path());
    std::fs::write(
        root.join("src/main.rs"),
        "fn main() { this does not compile }\n",
    )
    .unwrap();
    let r = cli(
        dir.path(),
        &["build", "--dir", root.to_str().unwrap(), "--json"],
    );
    assert_eq!(r.exit, Exit::Failure);
    let v = r.json();
    assert!(
        v["error"].as_str().unwrap().contains("cargo build failed"),
        "{v}"
    );
    assert!(v["hint"].as_str().unwrap().contains("Fix the errors"));
}
