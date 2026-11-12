//! `dev ci isolation` (the Rust port of the CI kit's job-isolation.sh, RT-1) as a real process, over the real slot holder: acquire,
//! a kept folder, release, and the sweep, plus the layouts of the ADR 0025 amendment of 2026-11-10 (`{src}` and retired layouts),
//! which the shell script does not know. The parity cases against the script live in conformance/test/job-isolation.test.mjs
//! (`ISOLATION_CMD=<this binary>`). Unix only, like isolation.rs: the Windows host jobs run the real thing on every run.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const AMENDED: &str =
    "target target-{name}-{src}-{slot} slot\nsrc src/{name}-{slot} slot\ntools tools shared\n# retired: target-{name}-{slot} 2026-11-10\n";

struct Host {
    ci: PathBuf,
    home: PathBuf,
    temp: PathBuf,
    env_file: PathBuf,
    tables: PathBuf,
}

impl Host {
    fn new(root: &Path, kept: &str) -> Host {
        let root = root.canonicalize().unwrap();
        let home = root.join("home");
        let temp = root.join("work/0123456789abcdef/tmp");
        let tables = root.join("tables");
        for d in [&home, &temp, &tables] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(tables.join("kept-folders.txt"), kept).unwrap();
        std::fs::write(tables.join("ports.txt"), "sccache 4227 1 1\n").unwrap();
        let env_file = root.join("work/0123456789abcdef/github-env");
        std::fs::write(&env_file, "").unwrap();
        Host {
            ci: home.join(".paddock-ci"),
            home,
            temp,
            env_file,
            tables,
        }
    }

    fn exported(&self) -> HashMap<String, String> {
        std::fs::read_to_string(&self.env_file)
            .unwrap()
            .lines()
            .filter_map(|l| l.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
            .collect()
    }

    // Starts the tool under test, git or a helper by program name (a throw-away fixture), never a process by pid (C53).
    #[allow(clippy::disallowed_methods)]
    fn run(&self, args: &[&str], envs: &[(&str, &str)], later: bool) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_paddock-dev"));
        cmd.args(["ci", "isolation"])
            .args(args)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &self.home)
            .env("RUNNER_OS", "macOS")
            .env("RUNNER_TEMP", &self.temp)
            .env("GITHUB_ENV", &self.env_file)
            .env("PADDOCK_CI_TABLES", &self.tables)
            .env("PADDOCK_CI_SLOTS", "3")
            .env("PADDOCK_CI_KEEP_SLOTS", "1")
            .env("PADDOCK_CI_MIN_FREE_GB", "0")
            // ER-14: `dev disk check` must not depend on the real disk of the machine running the tests.
            .env("PADDOCK_DISK_WARN_GIB", "0")
            .env("PADDOCK_DISK_REFUSE_GIB", "0")
            .env("PADDOCK_DISK_INDEX", self.home.join("disk-index.sqlite"));
        if later {
            cmd.envs(self.exported());
        }
        cmd.envs(envs.iter().copied());
        cmd.output().unwrap()
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn acquires_uses_releases_and_sweeps_with_the_real_binary() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::new(d.path(), "target target-{name}-{slot} slot\ntools tools shared\n");
    std::fs::create_dir_all(h.ci.join("target-old-s3")).unwrap();
    let acq = h.run(&["acquire"], &[], false);
    assert!(acq.status.success(), "{}", text(&acq.stderr));
    assert!(acq.stdout.is_empty(), "messages go to stderr");
    let env = h.exported();
    assert_eq!(env.get("PADDOCK_CI_SLOT").map(String::as_str), Some("s1"));
    assert!(env.get("PADDOCK_CI_SLOT_TOKEN").is_some_and(|t| !t.is_empty()));
    assert_eq!(
        env.get("PADDOCK_DEV").map(String::as_str),
        Some(env!("CARGO_BIN_EXE_paddock-dev")),
        "the job pins this very binary"
    );
    assert!(
        !h.ci.join("target-old-s3").exists(),
        "the sweep removed the idle slot's folder under its lock"
    );
    let st = h.run(&["status"], &[], true);
    assert!(text(&st.stdout).contains("s1 busy: job folder"), "{}", text(&st.stdout));

    let path = h.run(&["path", "target", "x-app"], &[], true);
    assert_eq!(text(&path.stdout).trim(), h.ci.join("target-x-app-s1").to_str().unwrap());
    std::fs::create_dir_all(h.ci.join("target-y-s2")).unwrap();
    let rel = h.run(&["release"], &[], true);
    assert!(rel.status.success(), "{}", text(&rel.stderr));
    assert!(!h.ci.join("target-y-s2").exists());
    assert!(h.ci.join("target-x-app-s1").exists(), "the kept slot's folder stays");
    assert!(text(&h.run(&["status"], &[], false).stdout).contains("s1 free"));
}

#[test]
fn the_amended_layouts_key_by_source_retire_old_folders_once_and_never_retire_a_current_one() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::new(d.path(), AMENDED);
    let (a, b) = (d.path().join("root-a"), d.path().join("root-b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let acq = h.run(&["acquire"], &[], false);
    assert!(acq.status.success(), "{}", text(&acq.stderr));

    let no_src = h.run(&["path", "target", "pitwall-app"], &[], true);
    assert_eq!(no_src.status.code(), Some(1));
    assert!(text(&no_src.stderr).contains("--source"), "{}", text(&no_src.stderr));
    let pa = text(
        &h.run(&["path", "target", "pitwall-app", "--source", a.to_str().unwrap()], &[], true)
            .stdout,
    );
    let pb = text(
        &h.run(&["path", "target", "pitwall-app", "--source", b.to_str().unwrap()], &[], true)
            .stdout,
    );
    assert_ne!(pa, pb, "two roots, two folders");
    let (pa, pb) = (pa.trim().to_string(), pb.trim().to_string());
    assert!(Path::new(&pa).is_dir() && Path::new(&pb).is_dir());

    // owns: yes for this slot's folder, no for another slot's, and for a folder with a source of the wrong shape (no `?*` is needed
    // to be strict here: the answer is by layout).
    assert!(h.run(&["owns", &pa, "target"], &[], true).status.success());
    let other = h.ci.join("target-pitwall-app-0123456789-s2");
    std::fs::create_dir_all(&other).unwrap();
    assert!(
        !h.run(&["owns", other.to_str().unwrap(), "target"], &[], true).status.success(),
        "another slot's folder"
    );

    // The sweep of idle slots: an old-layout folder of slot 2 is retired; a current folder (hyphenated name) of slot 2 is above the kept
    // slot, so it goes by the slot rule, and the same-looking folder of slot 1 (held, fresh) stays.
    let old_layout = h.ci.join("target-pitwall-app-s2");
    std::fs::create_dir_all(&old_layout).unwrap();
    let swept = h.run(
        &["sweep-slot"],
        &[("PADDOCK_CI_IDLE_SLOT", "s2"), ("PADDOCK_CI_KEEP_SLOTS", "2")],
        true,
    );
    assert!(swept.status.success(), "{}", text(&swept.stderr));
    let said = text(&swept.stderr);
    assert!(said.contains("removed target-pitwall-app-s2 (retired layout"), "{said}");
    assert!(
        !said.contains("0123456789-s2"),
        "a current folder inside the kept slots is never taken for a retired one: {said}"
    );
    assert!(!old_layout.exists() && other.exists() && Path::new(&pa).exists());
}

#[test]
fn the_engine_reads_every_name_from_the_data_files() {
    // A table that names nothing of Paddock: the commands work on it all the same.
    let d = tempfile::tempdir().unwrap();
    let h = Host::new(d.path(), "cache cachedir/{name}-{slot} slot\n");
    std::fs::write(h.tables.join("ports.txt"), "thing 9000 2 10\n").unwrap();
    let acq = h.run(&["acquire"], &[], false);
    assert!(acq.status.success(), "{}", text(&acq.stderr));
    let p = h.run(&["path", "cache", "n1"], &[], true);
    assert_eq!(text(&p.stdout).trim(), h.ci.join("cachedir/n1-s1").to_str().unwrap());
    assert_eq!(text(&h.run(&["port", "thing"], &[], true).stdout).trim(), "9000");
    assert_eq!(
        text(&h.run(&["port", "thing"], &[("PADDOCK_CI_SLOT", "s3")], true).stdout).trim(),
        "9020"
    );
    let pat = h.run(&["pattern", "[/\\\\]elsewhere[/\\\\]job-{slot}"], &[], true);
    assert_eq!(
        pat.status.code(),
        Some(1),
        "a stem the table does not declare does not anchor {{slot}}"
    );
    let ok = h.run(&["pattern", "[/\\\\]cachedir[/\\\\]n1-{slot}"], &[], true);
    assert!(ok.status.success(), "{}", text(&ok.stderr));
    let rel = h.run(&["release"], &[], true);
    assert!(rel.status.success(), "{}", text(&rel.stderr));
}

#[test]
fn the_source_key_is_one_command_and_matches_the_folder_path_makes() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::new(d.path(), AMENDED);
    let root = d.path().join("root-a");
    std::fs::create_dir_all(&root).unwrap();
    assert!(h.run(&["acquire"], &[], false).status.success());
    let key = text(&h.run(&["source-key", root.to_str().unwrap()], &[], true).stdout);
    let key = key.trim();
    assert_eq!(key.len(), 10);
    assert!(key.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{key}");
    let p = text(
        &h.run(&["path", "target", "x", "--source", root.to_str().unwrap()], &[], true)
            .stdout,
    );
    assert!(p.trim().ends_with(&format!("target-x-{key}-s1")), "{p}");
    assert_eq!(h.run(&["source-key", "/no/such/folder"], &[], true).status.code(), Some(1));
}

#[test]
fn a_table_with_an_unknown_placeholder_stops_acquire_before_a_slot_is_taken_and_names_the_line() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::new(d.path(), "src src/{name}-{slot} slot\ntarget target-{name}-{arch}-{slot} slot\n");
    let acq = h.run(&["acquire"], &[], false);
    assert_eq!(acq.status.code(), Some(1));
    let e = text(&acq.stderr);
    assert!(
        e.contains("::error::job-isolation: kept-folders.txt line 2") && e.contains("{arch}"),
        "{e}"
    );
    assert!(
        text(&h.run(&["status"], &[], false).stdout).contains("s1 free"),
        "no slot was taken"
    );
}

#[test]
fn release_gives_the_slot_back_even_when_the_tables_have_become_unreadable() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::new(d.path(), "target target-{name}-{slot} slot\n");
    assert!(h.run(&["acquire"], &[], false).status.success());
    std::fs::write(h.tables.join("kept-folders.txt"), "target target-{name}-{arch}-{slot} slot\n").unwrap();
    let rel = h.run(&["release"], &[], true);
    assert_eq!(rel.status.code(), Some(1), "the table error is reported");
    assert!(
        text(&h.run(&["status"], &[], false).stdout).contains("s1 free"),
        "but the slot is free"
    );
}

#[test]
fn a_disk_that_cannot_be_measured_is_said_so_and_the_floor_is_not_pretended_checked() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::new(d.path(), "target target-{name}-{slot} slot\n");
    let acq = h.run(
        &["acquire"],
        &[("PADDOCK_DF", "/usr/bin/false"), ("PADDOCK_CI_MIN_FREE_GB", "30")],
        false,
    );
    assert!(acq.status.success(), "{}", text(&acq.stderr));
    let e = text(&acq.stderr);
    assert!(
        e.contains("::warning::job-isolation: free disk on ") && e.contains("not measured (") && e.contains("the floor was not checked"),
        "{e}"
    );
}

#[test]
// Starts the tool under test, git or a helper by program name (a throw-away fixture), never a process by pid (C53).
#[allow(clippy::disallowed_methods)]
fn the_binary_answers_print_binary_with_its_own_path() {
    let out = Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .arg("--print-binary")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        text(&out.stdout).trim(),
        std::env::current_exe()
            .ok()
            .map(|_| env!("CARGO_BIN_EXE_paddock-dev").to_string())
            .unwrap()
    );
}

#[test]
fn old_layout_folders_with_and_without_a_hyphen_are_retired_and_current_ones_stay() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::new(d.path(), AMENDED);
    for n in ["target-x-s2", "target-pitwall-app-s2", "target-pitwall-app-0123456789-s2"] {
        std::fs::create_dir_all(h.ci.join(n)).unwrap();
    }
    let swept = h.run(
        &["sweep-slot"],
        &[("PADDOCK_CI_IDLE_SLOT", "s2"), ("PADDOCK_CI_KEEP_SLOTS", "2")],
        false,
    );
    assert!(swept.status.success(), "{}", text(&swept.stderr));
    assert!(!h.ci.join("target-x-s2").exists() && !h.ci.join("target-pitwall-app-s2").exists());
    assert!(h.ci.join("target-pitwall-app-0123456789-s2").exists());
}

/// Git-bash paths (`/c/...`) on any OS: `PADDOCK_CI_ASSUME_WINDOWS=1` reads `/c/x` as `C:/x`, which here is a folder named `C:` in the
/// current folder, so the whole chain (home, `--source`, `owns`, `source-key`) runs for real.
#[test]
// Starts the tool under test, git or a helper by program name (a throw-away fixture), never a process by pid (C53).
#[allow(clippy::disallowed_methods)]
fn git_bash_paths_drive_path_and_owns_and_source_key_end_to_end() {
    let d = tempfile::tempdir().unwrap();
    let h = Host::new(d.path(), AMENDED);
    let cwd = d.path().join("cwd");
    std::fs::create_dir_all(cwd.join("C:/ci")).unwrap();
    std::fs::create_dir_all(cwd.join("C:/w")).unwrap();
    let run = |args: &[&str], later: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_paddock-dev"));
        cmd.args(["ci", "isolation"])
            .args(args)
            .current_dir(&cwd)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &h.home)
            .env("RUNNER_OS", "Windows")
            .env("RUNNER_TEMP", &h.temp)
            .env("GITHUB_ENV", &h.env_file)
            .env("PADDOCK_CI_TABLES", &h.tables)
            .env("PADDOCK_CI_HOME", "/c/ci")
            .env("PADDOCK_CI_ASSUME_WINDOWS", "1")
            .env("PADDOCK_CI_SLOT", "s1")
            .env("PADDOCK_CI_SLOTS", "3");
        let _ = later;
        cmd.output().unwrap()
    };
    let key_bash = text(&run(&["source-key", "/c/w"], false).stdout);
    let key_drive = text(&run(&["source-key", "C:/w"], false).stdout);
    assert_eq!(key_bash.trim().len(), 10, "{key_bash}");
    assert_eq!(key_bash, key_drive, "/c/w and C:/w are one folder");
    let p = run(&["path", "target", "x", "--source", "/c/w"], false);
    assert!(p.status.success(), "{}", text(&p.stderr));
    let path = text(&p.stdout);
    assert!(path.trim().ends_with(&format!("target-x-{}-s1", key_bash.trim())), "{path}");
    assert!(Path::new(path.trim()).is_dir());
    assert!(
        run(&["owns", "/c/ci/target-x-0123456789-s1"], false).status.code() == Some(1),
        "no such folder"
    );
    let mine = cwd.join("C:/ci").join(Path::new(path.trim()).file_name().unwrap());
    assert!(mine.is_dir());
    let spelled = format!("/c/ci/{}", mine.file_name().unwrap().to_str().unwrap());
    assert!(run(&["owns", &spelled, "target"], false).status.success(), "owns {spelled}");
    std::fs::create_dir_all(cwd.join("C:/ci/target-x-0123456789-s2")).unwrap();
    assert_eq!(
        run(&["owns", "/c/ci/target-x-0123456789-s2", "target"], false).status.code(),
        Some(1),
        "another slot"
    );
}
