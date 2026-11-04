//! `dev vm` end to end with the real binary and a fake `tart` (a shell script that keeps one file per VM): lease, a refused second
//! lease, clone, headless start, stop, revert, delete, release and verify; a guest that runs without a lease is refused, and
//! `--adopt` is the person's explicit override. No VM is ever started.
#![cfg(unix)]
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

const FAKE_TART: &str = r#"#!/bin/sh
S="$FAKE_TART_STATE/vms"; mkdir -p "$S"
case "$1" in
  --version) echo "tart 2.0";;
  list) printf '['; first=1
    for f in "$S"/*; do [ -f "$f" ] || continue; n=$(basename "$f"); st=$(cat "$f"); r=false; [ "$st" = running ] && r=true
      [ $first = 1 ] || printf ','; first=0; printf '{"Name":"%s","Running":%s,"State":"%s"}' "$n" "$r" "$st"; done; printf ']\n';;
  clone) echo stopped > "$S/$3"; echo "clone $2 $3" >> "$FAKE_TART_STATE/calls.log";;
  run) for a; do n=$a; done; echo running > "$S/$n"; echo "run $*" >> "$FAKE_TART_STATE/calls.log";;
  stop) echo stopped > "$S/$2"; echo "stop $*" >> "$FAKE_TART_STATE/calls.log";;
  delete) rm -f "$S/$2"; echo "delete $2" >> "$FAKE_TART_STATE/calls.log";;
  *) exit 2;;
esac
"#;

struct Rig {
    _dir: tempfile::TempDir,
    state: PathBuf,
    fake: PathBuf,
    home: PathBuf,
    tart: PathBuf,
}

fn rig() -> Rig {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let fake = dir.path().join("fake");
    let home = dir.path().join("tart-home");
    std::fs::create_dir_all(&fake).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let tart = dir.path().join("tart");
    std::fs::write(&tart, FAKE_TART).unwrap();
    std::fs::set_permissions(&tart, std::fs::Permissions::from_mode(0o755)).unwrap();
    Rig {
        _dir: dir,
        state,
        fake,
        home,
        tart,
    }
}

impl Rig {
    fn dev(&self, args: &[&str]) -> (i32, Value, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
            .arg("--json")
            .args(args)
            .env("PADDOCK_VM_STATE", &self.state)
            .env("TART_HOME", &self.home)
            .env("TART", &self.tart)
            .env("FAKE_TART_STATE", &self.fake)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        (
            out.status.code().unwrap(),
            serde_json::from_str(text.trim()).unwrap_or(Value::Null),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn lease(&self, vm: &str, purpose: &str, extra: &[&str]) -> (i32, Value, String) {
        let pid = std::process::id().to_string();
        let mut a = vec!["vm", "lease", vm, "--for", purpose, "--holder-pid", &pid];
        a.extend_from_slice(extra);
        self.dev(&a)
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.fake.join("calls.log")).unwrap_or_default()
    }

    fn vm_file(&self, name: &str) -> PathBuf {
        self.fake.join("vms").join(name)
    }
}

fn id(v: &Value) -> String {
    v["lease"].as_str().unwrap().to_string()
}

#[test]
fn a_whole_tart_run_goes_through_one_lease_and_a_second_lease_is_refused() {
    let r = rig();
    std::fs::create_dir_all(r.fake.join("vms")).unwrap();
    std::fs::write(r.vm_file("linux-gnome-base"), "stopped").unwrap();

    let (code, first, err) = r.lease("dyno-vm-it", "dyno lifecycle", &[]);
    assert_eq!(code, 0, "{err}");
    let lease = id(&first);

    let (code, second, _) = r.lease("dyno-vm-it", "someone else", &[]);
    assert_eq!(code, 4);
    assert_eq!(second["kind"], "refused");
    let msg = second["error"].as_str().unwrap();
    assert!(msg.contains("dyno lifecycle") && msg.contains(&lease), "{msg}");

    let (code, v, err) = r.dev(&["vm", "clone", &lease, "--base", "linux-gnome-base"]);
    assert_eq!(code, 0, "{v} {err}");
    let (code, v, err) = r.dev(&[
        "vm",
        "start",
        &lease,
        "--max-load",
        "100000",
        "--arg",
        "--vnc-experimental",
        "--arg",
        "--rosetta=rosetta",
    ]);
    assert_eq!(code, 0, "{v} {err}");
    let calls = r.calls();
    assert!(
        calls.contains("run --no-graphics --vnc-experimental --rosetta=rosetta dyno-vm-it"),
        "{calls}"
    );
    assert!(!calls.contains("--graphics"), "{calls}");

    let (_, status, _) = r.dev(&["vm", "status", "dyno-vm-it"]);
    assert_eq!(status["running"], true);
    assert_eq!(status["lease"]["id"], lease.as_str());

    let (code, v, err) = r.dev(&["vm", "stop", &lease, "--grace", "5"]);
    assert_eq!(code, 0, "{v} {err}");
    assert_eq!(std::fs::read_to_string(r.vm_file("dyno-vm-it")).unwrap().trim(), "stopped");

    let (code, v, err) = r.dev(&["vm", "revert", &lease, "--to", "linux-gnome-base"]);
    assert_eq!(code, 0, "{v} {err}");
    assert!(
        r.calls().contains("delete dyno-vm-it") && r.calls().matches("clone linux-gnome-base dyno-vm-it").count() == 2,
        "{}",
        r.calls()
    );

    let (code, v, err) = r.dev(&["vm", "delete", &lease]);
    assert_eq!(code, 0, "{v} {err}");
    assert!(!r.vm_file("dyno-vm-it").exists());
    assert!(r.vm_file("linux-gnome-base").exists(), "a base image is never touched");

    let (code, _, _) = r.dev(&["vm", "release", &lease]);
    assert_eq!(code, 0);
    assert_eq!(
        r.dev(&["vm", "verify", &lease]).0,
        0,
        "a released lease is still known to have been issued"
    );
    assert_eq!(r.dev(&["vm", "verify", "dyno-vm-it.1.2"]).0, 4);
    assert_eq!(
        r.dev(&["vm", "start", &lease, "--max-load", "100000"]).0,
        4,
        "a released lease starts nothing"
    );

    let journal = std::fs::read_to_string(r.state.join("log/dyno-vm-it.log")).unwrap();
    assert!(
        journal.contains(&lease) && journal.contains("started headless") && journal.contains("lease released"),
        "{journal}"
    );
}

#[test]
fn a_guest_that_runs_without_a_lease_is_refused_and_adopt_is_the_explicit_override() {
    let r = rig();
    std::fs::create_dir_all(r.fake.join("vms")).unwrap();
    std::fs::write(r.vm_file("busy-vm"), "running").unwrap();

    let (code, v, _) = r.lease("busy-vm", "me", &[]);
    assert_eq!(code, 4);
    assert!(v["error"].as_str().unwrap().contains("running without a lease"), "{v}");
    assert_eq!(
        r.dev(&["vm", "list"]).1["leases"].as_array().unwrap().len(),
        0,
        "a refused lease is not kept"
    );

    let (code, v, err) = r.lease("busy-vm", "stop it", &["--adopt"]);
    assert_eq!(code, 0, "{v} {err}");
    let lease = id(&v);
    assert_eq!(r.dev(&["vm", "stop", &lease, "--grace", "5"]).0, 0);
    assert!(
        std::fs::read_to_string(r.state.join("log/busy-vm.log"))
            .unwrap()
            .contains("adopted a running guest")
    );
}

#[test]
fn a_base_image_is_never_leased_started_or_deleted_and_commands_need_a_lease() {
    let r = rig();
    let (code, v, _) = r.lease("linux-gnome-base", "x", &[]);
    assert_eq!(code, 4);
    assert!(v["error"].as_str().unwrap().contains("base image"));
    for args in [
        &["vm", "start", "nope.1.2"][..],
        &["vm", "stop", "nope.1.2"],
        &["vm", "revert", "nope.1.2", "--to", "s"],
        &["vm", "delete", "nope.1.2"],
    ] {
        assert_eq!(r.dev(args).0, 4, "{args:?}");
    }
    assert!(r.calls().is_empty(), "nothing reached the hypervisor tool");
}

#[test]
fn a_lease_ends_with_its_holder_process() {
    let r = rig();
    let mut child = Command::new("sleep").arg("30").spawn().unwrap();
    let pid = child.id().to_string();
    let (code, _, err) = r.dev(&["vm", "lease", "dyno-vm-holder", "--for", "short", "--holder-pid", &pid]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(r.lease("dyno-vm-holder", "other", &[]).0, 4, "held while the holder lives");
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(r.lease("dyno-vm-holder", "other", &[]).0, 0, "free once the holder is gone");
}

#[test]
fn help_lists_the_vm_commands() {
    let out = Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(["vm", "--help"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    for w in ["lease", "preflight", "start", "stop", "revert", "snapshot", "vnc", "verify"] {
        assert!(text.contains(w), "{w}");
    }
}
