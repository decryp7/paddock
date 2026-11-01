//! `dev host prune` against the recorded behaviour of the four scripts (tests/golden-prune) and its own dry-run, guard and
//! symlink rules. Scratch folders only (tempfile); nothing reads a clock and nothing sleeps.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn dev(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_paddock-dev")).args(args).output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden-prune")
}

fn read_json(name: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(golden_dir().join(name)).unwrap()).unwrap()
}

fn canonical_tmp() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let p = fs::canonicalize(t.path()).unwrap();
    (t, p)
}

fn link(target: &str, at: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, at).unwrap();
    #[cfg(not(unix))]
    let _ = (target, at);
}

/// The fixture of `fixture.json` in `folder` (symlinks only where the platform makes them without a privilege).
fn build(folder: &Path) {
    build_with(folder, true);
}

/// The same, without the Cargo marker when `marker` is false.
fn build_with(folder: &Path, marker: bool) {
    let fixture = read_json("fixture.json");
    for f in fixture["files"].as_array().unwrap() {
        let path = f["path"].as_str().unwrap();
        if f["marker"] == true && !marker {
            continue;
        }
        if path.ends_with('/') {
            fs::create_dir_all(folder.join(path)).unwrap();
        } else {
            let p = folder.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, vec![7u8; f["size"].as_u64().unwrap() as usize]).unwrap();
        }
    }
    for l in fixture["links"].as_array().unwrap() {
        link(l["target"].as_str().unwrap(), &folder.join(l["path"].as_str().unwrap()));
    }
}

fn tree(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        let p = dir.join(&name);
        let m = fs::symlink_metadata(&p).unwrap();
        let rel = format!("{prefix}{name}");
        if m.file_type().is_symlink() {
            out.push(format!("{rel} -> link"));
        } else if m.is_dir() {
            out.push(format!("{rel}/"));
            tree(&p, &format!("{rel}/"), out);
        } else {
            out.push(format!("{rel} {}", m.len()));
        }
    }
}

fn mask(text: &str, root: &Path) -> String {
    let t = text.replace(root.to_str().unwrap(), "<ROOT>").replace('\\', "/");
    let t = regex::Regex::new(r" is \d+ \(cap")
        .unwrap()
        .replace_all(&t, " is <N> (cap")
        .into_owned();
    regex::Regex::new(r"still \d+;").unwrap().replace_all(&t, "still <N>;").into_owned()
}

#[test]
fn the_recorded_cases_of_the_shell_scripts_are_reproduced() {
    let expected = read_json("expected.json");
    let cases = read_json("cases.json");
    for c in cases.as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        if !cfg!(unix) && (name.contains("symlink") || name.contains("root")) {
            continue;
        }
        let (_t, root) = canonical_tmp();
        for d in ["home/target-app-s1", "home/target-app-s2", "home/notes", "other/build"] {
            build_with(&root.join(d), c["noMarker"] != true);
        }
        link("target-app-s1", &root.join("home/linkroot"));
        fs::create_dir(root.join("outside")).unwrap();
        fs::write(root.join("outside/canary"), "x").unwrap();
        let folder = match c["folder"].as_str().unwrap() {
            "" => String::new(),
            "//" => "/".to_string(),
            f if f.starts_with('/') => format!("{}{f}", root.display()),
            f => root.join(f).to_str().unwrap().to_string(),
        };
        let userhome = root.join("userhome");
        fs::create_dir(&userhome).unwrap();
        let mut args: Vec<String> = [
            "host",
            "prune",
            "cap",
            "--unit",
            "kb",
            "--root",
            &folder,
            "--max-gb",
            c["cap"].as_str().unwrap(),
            "--home",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        args.push(userhome.to_str().unwrap().into());
        if c["mode"] == "ci" {
            args.extend([
                "--ci-home".into(),
                root.join("home").to_str().unwrap().into(),
                "--slot".into(),
                c["slot"].as_str().unwrap().into(),
            ]);
            if c["dropBundles"] == true {
                args.push("--drop-bundles".into());
            }
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (code, out, _) = dev(&refs);
        let want = &expected[name];
        assert_eq!(i64::from(code), want["exit"].as_i64().unwrap(), "{name}: exit");
        assert_eq!(mask(&out, &root), want["stdout"].as_str().unwrap(), "{name}: stdout");
        let mut got = Vec::new();
        tree(&root, "", &mut got);
        got.retain(|l| !l.starts_with("userhome"));
        let want_tree: Vec<String> = want["tree"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .filter(|l| cfg!(unix) || !l.ends_with("-> link"))
            .collect();
        assert_eq!(got, want_tree, "{name}: the tree left behind");
    }
}

/// An existing, empty home folder in the scratch root (the guard fails closed without one).
fn scratch_home(root: &Path) -> String {
    let h = root.join("scratch-home");
    fs::create_dir_all(&h).unwrap();
    h.to_str().unwrap().to_string()
}

fn json(args: &[&str]) -> Value {
    let mut all = vec!["--json"];
    all.extend_from_slice(args);
    let (code, out, err) = dev(&all);
    assert_eq!(code, 0, "{args:?}: {err}{out}");
    serde_json::from_str(out.trim()).unwrap()
}

#[test]
fn a_dry_run_reports_the_real_run_and_removes_nothing() {
    for (cap, steps) in [("300", "incremental"), ("100", "folder")] {
        let (_t, root) = canonical_tmp();
        let folder = root.join("home/target-app-s1");
        build(&folder);
        scratch_home(&root);
        let mut before = Vec::new();
        tree(&root, "", &mut before);
        let f = folder.to_str().unwrap();
        let dry = json(&[
            "host",
            "prune",
            "cap",
            "--unit",
            "kb",
            "--root",
            f,
            "--max-gb",
            cap,
            "--dry-run",
            "--home",
            &scratch_home(&root),
        ]);
        let mut after = Vec::new();
        tree(&root, "", &mut after);
        assert_eq!(before, after, "a dry run changes nothing");
        assert_eq!(dry["action"], steps);
        assert_eq!(dry["dryRun"], true);
        assert_eq!(
            dry["removed"].as_array().unwrap().len(),
            3,
            "debug/incremental, release/deep/incremental and release/incremental"
        );
        let real = json(&[
            "host",
            "prune",
            "cap",
            "--unit",
            "kb",
            "--root",
            f,
            "--max-gb",
            cap,
            "--home",
            &scratch_home(&root),
        ]);
        assert_eq!(real["action"], dry["action"]);
        assert_eq!(real["removed"], dry["removed"], "the dry run names what the real run removed");
        assert_eq!((&real["sizeBefore"], &real["sizeAfter"]), (&dry["sizeBefore"], &dry["sizeAfter"]));
        assert_eq!(real["folderRemoved"], dry["folderRemoved"]);
        assert_eq!(real["folderRemoved"], steps == "folder");
    }
}

#[test]
fn under_the_cap_the_json_says_nothing_to_do() {
    let (_t, root) = canonical_tmp();
    let folder = root.join("b/build");
    build(&folder);
    let v = json(&[
        "host",
        "prune",
        "cap",
        "--unit",
        "kb",
        "--root",
        folder.to_str().unwrap(),
        "--max-gb",
        "100000",
        "--home",
        &scratch_home(&root),
    ]);
    assert_eq!(
        (v["ok"].clone(), v["action"].clone(), v["removed"].clone()),
        (true.into(), "none".into(), Value::Array(vec![]))
    );
    assert_eq!(v["unit"], "kb");
}

#[test]
fn roots_in_the_users_home_documents_are_refused_and_nothing_is_touched() {
    let (_t, root) = canonical_tmp();
    let home = root.join("me");
    for refused in ["Documents/proj", "Desktop", "downloads/x"] {
        let folder = home.join(refused);
        build(&folder);
        let (code, _, err) = dev(&[
            "host",
            "prune",
            "cap",
            "--unit",
            "kb",
            "--root",
            folder.to_str().unwrap(),
            "--max-gb",
            "1",
            "--home",
            home.to_str().unwrap(),
        ]);
        assert_eq!(code, 4, "{refused}: {err}");
        assert!(err.contains("Documents, Desktop, Downloads"), "{err}");
        assert!(folder.join("keep.txt").exists(), "{refused} is untouched");
    }
    for (root_arg, why) in [
        (home.to_str().unwrap(), "the home itself"),
        (root.to_str().unwrap(), "a folder holding the home"),
    ] {
        let (code, _, _) = dev(&[
            "host",
            "prune",
            "cap",
            "--root",
            root_arg,
            "--max-gb",
            "1",
            "--home",
            home.to_str().unwrap(),
        ]);
        assert_eq!(code, 4, "{why}");
    }
    let (code, _, err) = dev(&[
        "host",
        "prune",
        "cap",
        "--root",
        "relative/dir",
        "--max-gb",
        "1",
        "--dry-run",
        "--home",
        home.to_str().unwrap(),
    ]);
    assert_eq!(code, 2, "{err}");
    let (code, out, _) = dev(&[
        "--json",
        "host",
        "prune",
        "cap",
        "--root",
        "/",
        "--max-gb",
        "1",
        "--dry-run",
        "--home",
        home.to_str().unwrap(),
    ]);
    assert_eq!(code, 4);
    assert_eq!(serde_json::from_str::<Value>(out.trim()).unwrap()["kind"], "refused");
}

#[test]
fn the_ci_form_never_fails_and_a_ci_home_inside_the_home_is_fine() {
    let (_t, root) = canonical_tmp();
    let home = root.join("me");
    let ci_home = home.join(".paddock-ci");
    let folder = ci_home.join("target-app-s1");
    build(&folder);
    let v = json(&[
        "host",
        "prune",
        "cap",
        "--unit",
        "kb",
        "--root",
        folder.to_str().unwrap(),
        "--max-gb",
        "300",
        "--ci-home",
        ci_home.to_str().unwrap(),
        "--slot",
        "s1",
        "--home",
        home.to_str().unwrap(),
        "--drop-bundles",
    ]);
    assert_eq!(v["mode"], "ci");
    assert!(!folder.join("dist/bundle").exists());
    assert!(folder.join("keep.txt").exists());
}

#[cfg(unix)]
#[test]
fn a_symlinked_dist_is_never_followed_by_drop_bundles() {
    let (_t, root) = canonical_tmp();
    let ci_home = root.join("ci");
    let folder = ci_home.join("target-app-s1");
    fs::create_dir_all(&folder).unwrap();
    fs::create_dir_all(root.join("elsewhere/bundle")).unwrap();
    fs::write(root.join("elsewhere/bundle/canary"), "x").unwrap();
    link("../../elsewhere", &folder.join("dist"));
    json(&[
        "host",
        "prune",
        "cap",
        "--unit",
        "kb",
        "--root",
        folder.to_str().unwrap(),
        "--max-gb",
        "1000",
        "--ci-home",
        ci_home.to_str().unwrap(),
        "--slot",
        "s1",
        "--home",
        &scratch_home(&root),
        "--drop-bundles",
    ]);
    assert!(root.join("elsewhere/bundle/canary").exists());
}

#[cfg(unix)]
#[test]
fn a_symlinked_incremental_is_not_followed_out_of_the_root() {
    let (_t, root) = canonical_tmp();
    let folder = root.join("w/build");
    fs::create_dir_all(folder.join("debug")).unwrap();
    fs::write(folder.join("CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55").unwrap();
    fs::create_dir_all(root.join("outside")).unwrap();
    fs::write(root.join("outside/canary"), vec![1u8; 10_000]).unwrap();
    link("../../../outside", &folder.join("debug/incremental"));
    let v = json(&[
        "host",
        "prune",
        "cap",
        "--unit",
        "kb",
        "--root",
        folder.to_str().unwrap(),
        "--max-gb",
        "0",
        "--home",
        &scratch_home(&root),
    ]);
    assert_eq!(v["folderRemoved"], true, "a zero cap removes the folder");
    assert!(!folder.exists());
    assert!(root.join("outside/canary").exists(), "the link's target survives");
}

#[test]
fn worktrees_list_keep_and_remove_the_right_folders() {
    let (_t, root) = canonical_tmp();
    let home = root.join("me");
    let repo = root.join("repo");
    for d in [
        "me/.app-target-6c1/x",
        "me/.app-target-old/x",
        "me/.other-target-1/x",
        "repo/target/debug/x",
        "repo/target/agent-6c/x",
        "repo/target/agent-9/x",
        "repo/target/agent-nomarker/x",
        "repo/target/flycheck0/x",
        "repo/target/doc/x",
    ] {
        let p = root.join(d);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "data").unwrap();
    }
    for marked in [
        "me/.app-target-6c1",
        "me/.app-target-old",
        "repo/target/agent-6c",
        "repo/target/agent-9",
    ] {
        fs::write(root.join(marked).join(".rustc_info.json"), "{}").unwrap();
    }
    let h = home.to_str().unwrap();
    let r = repo.to_str().unwrap();
    let listed = json(&["host", "prune", "worktrees", "--app", "app", "--home", h, "--repo", r]);
    let names: Vec<String> = listed["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().rsplit(['/', '\\']).next().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        [".app-target-6c1", ".app-target-old", "agent-6c", "agent-9", "agent-nomarker"]
    );
    assert!(listed["folders"].as_array().unwrap().iter().all(|f| f["action"] == "found"));
    let dry = json(&[
        "host",
        "prune",
        "worktrees",
        "--app",
        "app",
        "--home",
        h,
        "--repo",
        r,
        "--remove",
        "--keep",
        "6c",
        "--dry-run",
    ]);
    let actions: Vec<&str> = dry["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions, ["kept", "would-remove", "kept", "would-remove", "not-cargo"]);
    assert!(root.join("me/.app-target-old").exists());
    let done = json(&[
        "host",
        "prune",
        "worktrees",
        "--app",
        "app",
        "--home",
        h,
        "--repo",
        r,
        "--remove",
        "--keep",
        "6c",
    ]);
    assert_eq!(done["errors"].as_array().unwrap().len(), 0);
    assert!(!root.join("me/.app-target-old").exists() && !root.join("repo/target/agent-9").exists());
    for kept in [
        "me/.app-target-6c1",
        "me/.other-target-1",
        "repo/target/agent-6c",
        "repo/target/agent-nomarker",
        "repo/target/debug",
        "repo/target/flycheck0",
        "repo/target/doc",
    ] {
        assert!(root.join(kept).exists(), "{kept} stays");
    }
    let (_, text, _) = dev(&["host", "prune", "worktrees", "--app", "app", "--home", h, "--repo", r]);
    assert!(text.lines().next().unwrap().starts_with("     0.0 GB  found     "), "{text}");
}

#[test]
fn worktrees_refuse_bad_input() {
    let (_t, root) = canonical_tmp();
    let (h, r) = (scratch_home(&root), root.to_str().unwrap().to_string());
    for args in [
        &["--app", "Bad"][..],
        &["--app", "app", "--keep", "a*"],
        &["--app", "app", "--keep", ""],
    ] {
        let mut all = vec!["host", "prune", "worktrees", "--home", &h, "--repo", &r];
        all.extend_from_slice(args);
        assert_eq!(dev(&all).0, 2, "{args:?}");
    }
}

#[test]
fn a_bad_cap_is_a_usage_error() {
    let (_t, root) = canonical_tmp();
    let (code, out, err) = dev(&[
        "host",
        "prune",
        "cap",
        "--root",
        root.to_str().unwrap(),
        "--max-gb",
        "many",
        "--home",
        &scratch_home(&root),
    ]);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.contains("whole number"), "{err}");
}

fn dev_without_home(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(args)
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn without_a_usable_home_the_guard_fails_closed() {
    let (_t, root) = canonical_tmp();
    let folder = root.join("w/build");
    build(&folder);
    let f = folder.to_str().unwrap();
    assert_eq!(dev_without_home(&["host", "prune", "cap", "--root", f, "--max-gb", "0"]).0, 4);
    let missing = root.join("no-such-home");
    let (code, _, err) = dev(&[
        "host",
        "prune",
        "cap",
        "--root",
        f,
        "--max-gb",
        "0",
        "--home",
        missing.to_str().unwrap(),
    ]);
    assert_eq!(code, 4, "{err}");
    assert_eq!(dev_without_home(&["host", "prune", "worktrees", "--app", "app"]).0, 4);
    assert!(folder.join("keep.txt").exists(), "nothing was removed");
    // The CI form never fails: the same situation is a notice and exit 0.
    let ci = root.join("ci");
    let owned = ci.join("target-app-s1");
    build(&owned);
    let (code, out) = dev_without_home(&[
        "host",
        "prune",
        "cap",
        "--root",
        owned.to_str().unwrap(),
        "--max-gb",
        "0",
        "--ci-home",
        ci.to_str().unwrap(),
        "--slot",
        "s1",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("leaving it alone"), "{out}");
    assert!(owned.join("keep.txt").exists());
}

#[cfg(unix)]
#[test]
fn the_home_is_compared_by_identity_not_by_spelling() {
    let (_t, root) = canonical_tmp();
    let real_home = root.join("realhome");
    fs::create_dir_all(real_home.join("Documents/proj")).unwrap();
    fs::write(real_home.join("Documents/proj/keep.txt"), "x").unwrap();
    // HOME is given through a symlink, the root through the real path.
    link("realhome", &root.join("h"));
    let (code, _, err) = dev(&[
        "host",
        "prune",
        "cap",
        "--root",
        real_home.join("Documents/proj").to_str().unwrap(),
        "--max-gb",
        "0",
        "--home",
        root.join("h").to_str().unwrap(),
    ]);
    assert_eq!(code, 4, "{err}");
    // The root is given through the symlink, HOME through the real path.
    let (code, _, _) = dev(&[
        "host",
        "prune",
        "cap",
        "--root",
        root.join("h/Documents/proj").to_str().unwrap(),
        "--max-gb",
        "0",
        "--home",
        real_home.to_str().unwrap(),
    ]);
    assert_eq!(code, 4);
    // The home itself, through the symlink.
    let (code, _, _) = dev(&[
        "host",
        "prune",
        "cap",
        "--root",
        root.join("h").to_str().unwrap(),
        "--max-gb",
        "0",
        "--home",
        real_home.to_str().unwrap(),
    ]);
    assert_eq!(code, 4);
    // A case variant of Documents is the same folder on a case-insensitive volume.
    fs::write(root.join("Probe"), "x").unwrap();
    if root.join("pROBE").exists() {
        let (code, _, _) = dev(&[
            "host",
            "prune",
            "cap",
            "--root",
            real_home.join("DOCUMENTS/proj").to_str().unwrap(),
            "--max-gb",
            "0",
            "--home",
            real_home.to_str().unwrap(),
        ]);
        assert_eq!(code, 4, "case variant of Documents");
    }
    assert!(real_home.join("Documents/proj/keep.txt").exists());
}

#[test]
fn the_ci_form_turns_every_problem_into_a_notice_with_exit_zero() {
    let (_t, root) = canonical_tmp();
    let home = scratch_home(&root);
    let ci = root.join("ci");
    fs::create_dir_all(&ci).unwrap();
    for (root_arg, cap) in [
        ("", "1"),
        ("/", "1"),
        ("relative", "1"),
        (ci.to_str().unwrap(), ""),
        (ci.to_str().unwrap(), "x"),
        (home.as_str(), "1"),
    ] {
        let (code, out, err) = dev(&[
            "host",
            "prune",
            "cap",
            "--root",
            root_arg,
            "--max-gb",
            cap,
            "--ci-home",
            ci.to_str().unwrap(),
            "--slot",
            "s1",
            "--home",
            &home,
        ]);
        assert_eq!(code, 0, "{root_arg:?} {cap:?}: {err}");
        assert!(out.contains("prune:"), "{out}");
    }
}

#[cfg(unix)]
#[test]
fn a_root_given_with_a_trailing_slash_is_not_followed_through_a_symlink() {
    let (_t, root) = canonical_tmp();
    let real = root.join("real/build");
    build(&real);
    link("real/build", &root.join("l"));
    let arg = format!("{}/", root.join("l").display());
    let v = json(&[
        "host",
        "prune",
        "cap",
        "--unit",
        "kb",
        "--root",
        &arg,
        "--max-gb",
        "0",
        "--home",
        &scratch_home(&root),
    ]);
    assert_eq!(v["action"], "absent");
    assert!(real.join("keep.txt").exists(), "the link's target is untouched");
}

#[test]
fn in_the_ci_form_a_bad_cap_is_noticed_before_the_bundles_go() {
    let (_t, root) = canonical_tmp();
    let ci = root.join("ci");
    let folder = ci.join("target-app-s1");
    build(&folder);
    let (code, out, err) = dev(&[
        "host",
        "prune",
        "cap",
        "--unit",
        "kb",
        "--root",
        folder.to_str().unwrap(),
        "--max-gb",
        "x",
        "--drop-bundles",
        "--ci-home",
        ci.to_str().unwrap(),
        "--slot",
        "s1",
        "--home",
        &scratch_home(&root),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("the cap must be a whole number"), "{out}");
    assert!(folder.join("dist/bundle/i").exists(), "nothing was removed");
}

#[test]
fn the_ci_form_without_a_slot_says_so_and_removes_nothing() {
    let (_t, root) = canonical_tmp();
    let ci = root.join("ci");
    let folder = ci.join("target-app-s1");
    build(&folder);
    let (code, out, _) = dev(&[
        "host",
        "prune",
        "cap",
        "--root",
        folder.to_str().unwrap(),
        "--max-gb",
        "0",
        "--ci-home",
        ci.to_str().unwrap(),
        "--home",
        &scratch_home(&root),
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("no slot given"), "{out}");
    assert!(folder.join("keep.txt").exists());
}
