use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

const WAIT: Duration = Duration::from_secs(60);
const MARK: Markers<'static> = &["node_modules/.package-lock.json"];

fn cache(dir: &Path) -> Cache {
    Cache::with_cap_bytes(dir.join("cache"), u64::MAX, WAIT)
}

/// A tree with what an install leaves: a marker, a package, a relative link as in `.bin` and a second workspace folder.
fn fake_install(tree: &Path) -> Result<Vec<String>, String> {
    let nm = tree.join("node_modules");
    std::fs::create_dir_all(nm.join(".bin")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(nm.join("pkg")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(tree.join("ui/node_modules/own")).map_err(|e| e.to_string())?;
    std::fs::write(nm.join(".package-lock.json"), "{}").map_err(|e| e.to_string())?;
    std::fs::write(nm.join("pkg/index.js"), "module.exports = 1;").map_err(|e| e.to_string())?;
    std::fs::write(tree.join("ui/node_modules/own/x.js"), "x").map_err(|e| e.to_string())?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("../pkg/index.js", nm.join(".bin/tool")).map_err(|e| e.to_string())?;
    Ok(vec!["node_modules".into(), "ui/node_modules".into()])
}

fn never() -> Result<Vec<String>, String> {
    panic!("a hit must not build")
}

fn trees() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("a");
    let b = d.path().join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    (d, a, b)
}

#[test]
fn the_key_follows_the_lockfile_the_node_version_and_the_platform() {
    let k = node_modules_key(b"lock", "v24.1.0", "macos", "aarch64");
    assert_eq!(
        k,
        node_modules_key(b"lock", "v24.1.0", "macos", "aarch64"),
        "the same inputs name the same entry"
    );
    for other in [
        node_modules_key(b"lock2", "v24.1.0", "macos", "aarch64"),
        node_modules_key(b"lock", "v24.2.0", "macos", "aarch64"),
        node_modules_key(b"lock", "v24.1.0", "linux", "aarch64"),
        node_modules_key(b"lock", "v24.1.0", "macos", "x86_64"),
    ] {
        assert_ne!(k, other);
    }
    let u = ui_key("d", "c", &k, "o");
    for other in [
        ui_key("d2", "c", &k, "o"),
        ui_key("d", "c2", &k, "o"),
        ui_key("d", "c", "other-deps", "o"),
        ui_key("d", "c", &k, "o2"),
    ] {
        assert_ne!(u, other);
    }
}

#[test]
fn the_second_tree_gets_a_clone_and_no_install_runs() {
    let (d, a, b) = trees();
    let c = cache(d.path());
    assert_eq!(c.fetch("node-modules-k", MARK, &a, || fake_install(&a)).unwrap(), Fetched::Built);
    assert_eq!(c.fetch("node-modules-k", MARK, &b, never).unwrap(), Fetched::Hit);
    assert!(b.join("node_modules/pkg/index.js").is_file() && b.join("ui/node_modules/own/x.js").is_file());
    let meta = std::fs::symlink_metadata(b.join("node_modules")).unwrap();
    assert!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "a clone is a real folder, never a link into the cache"
    );
    #[cfg(unix)]
    {
        let link = std::fs::read_link(b.join("node_modules/.bin/tool")).unwrap();
        assert_eq!(
            link,
            Path::new("../pkg/index.js"),
            "relative links stay relative, so they resolve inside the clone"
        );
        assert_eq!(
            std::fs::read_to_string(b.join("node_modules/.bin/tool")).unwrap(),
            "module.exports = 1;"
        );
    }
}

#[test]
fn a_clone_is_independent_of_the_entry_and_of_other_clones() {
    let (d, a, b) = trees();
    let c = cache(d.path());
    c.fetch("node-modules-k", MARK, &a, || fake_install(&a)).unwrap();
    c.fetch("node-modules-k", MARK, &b, never).unwrap();
    std::fs::write(b.join("node_modules/pkg/index.js"), "changed").unwrap();
    assert_eq!(
        std::fs::read_to_string(a.join("node_modules/pkg/index.js")).unwrap(),
        "module.exports = 1;"
    );
    let third = d.path().join("c");
    std::fs::create_dir_all(&third).unwrap();
    c.fetch("node-modules-k", MARK, &third, never).unwrap();
    assert_eq!(
        std::fs::read_to_string(third.join("node_modules/pkg/index.js")).unwrap(),
        "module.exports = 1;",
        "the entry is untouched"
    );
}

#[test]
fn a_hit_replaces_what_the_tree_already_has() {
    let (d, a, b) = trees();
    let c = cache(d.path());
    c.fetch("node-modules-k", MARK, &a, || fake_install(&a)).unwrap();
    std::fs::create_dir_all(b.join("node_modules/old")).unwrap();
    std::fs::write(b.join("node_modules/old/partial.js"), "partial").unwrap();
    c.fetch("node-modules-k", MARK, &b, never).unwrap();
    assert!(!b.join("node_modules/old").exists() && b.join("node_modules/pkg/index.js").is_file());
}

#[test]
fn a_changed_lockfile_is_a_miss() {
    let (d, a, b) = trees();
    let c = cache(d.path());
    let built = AtomicUsize::new(0);
    for (name, tree) in [("node-modules-one", &a), ("node-modules-two", &b)] {
        let r = c.fetch(name, MARK, tree, || {
            built.fetch_add(1, Ordering::SeqCst);
            fake_install(tree)
        });
        assert_eq!(r.unwrap(), Fetched::Built, "{name}");
    }
    assert_eq!(built.load(Ordering::SeqCst), 2);
}

#[test]
fn concurrent_builders_of_one_key_build_once() {
    let d = tempfile::tempdir().unwrap();
    let c = Arc::new(cache(d.path()));
    let built = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(6));
    let handles: Vec<_> = (0..6)
        .map(|i| {
            let (c, built, start) = (c.clone(), built.clone(), start.clone());
            let tree = d.path().join(format!("t{i}"));
            std::fs::create_dir_all(&tree).unwrap();
            std::thread::spawn(move || {
                start.wait();
                let r = c
                    .fetch("node-modules-k", MARK, &tree, || {
                        built.fetch_add(1, Ordering::SeqCst);
                        fake_install(&tree)
                    })
                    .unwrap();
                assert!(tree.join("node_modules/pkg/index.js").is_file());
                r
            })
        })
        .collect();
    let results: Vec<Fetched> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(built.load(Ordering::SeqCst), 1, "{results:?}");
    assert_eq!(results.iter().filter(|r| **r == Fetched::Hit).count(), 5);
}

#[test]
fn a_damaged_or_partial_entry_is_rebuilt() {
    for damage in [
        "a file is gone",
        "entry.json is gone",
        "the marker is gone",
        "entry.json is garbage",
        "a root is a path out of the entry",
    ] {
        let (d, a, b) = trees();
        let c = cache(d.path());
        c.fetch("node-modules-k", MARK, &a, || fake_install(&a)).unwrap();
        let entry = d.path().join("cache/node-modules-k");
        match damage {
            "a file is gone" => std::fs::remove_file(entry.join("payload/node_modules/pkg/index.js")).unwrap(),
            "entry.json is gone" => std::fs::remove_file(entry.join("entry.json")).unwrap(),
            "the marker is gone" => std::fs::remove_file(entry.join("payload/node_modules/.package-lock.json")).unwrap(),
            "entry.json is garbage" => std::fs::write(entry.join("entry.json"), "{ not json").unwrap(),
            _ => {
                let t = std::fs::read_to_string(entry.join("entry.json"))
                    .unwrap()
                    .replace("\"node_modules\"", "\"../../x\"");
                std::fs::write(entry.join("entry.json"), t).unwrap();
            }
        }
        let built = AtomicUsize::new(0);
        let r = c.fetch("node-modules-k", MARK, &b, || {
            built.fetch_add(1, Ordering::SeqCst);
            fake_install(&b)
        });
        assert_eq!(r.unwrap(), Fetched::Built, "{damage}");
        assert_eq!(built.load(Ordering::SeqCst), 1, "{damage}");
        // The rebuilt entry is sound: the next tree hits.
        let third = d.path().join("c");
        std::fs::create_dir_all(&third).unwrap();
        assert_eq!(c.fetch("node-modules-k", MARK, &third, never).unwrap(), Fetched::Hit, "{damage}");
    }
}

#[test]
fn a_build_that_leaves_no_marker_is_not_stored_and_leaves_no_staging() {
    let (d, a, _) = trees();
    let c = cache(d.path());
    let r = c.fetch("node-modules-k", MARK, &a, || {
        std::fs::create_dir_all(a.join("node_modules")).unwrap();
        std::fs::write(a.join("node_modules/x"), "x").unwrap();
        Ok(vec!["node_modules".into()])
    });
    assert!(r.unwrap_err().contains("incomplete"));
    assert!(!d.path().join("cache/node-modules-k").exists());
    let left: Vec<_> = std::fs::read_dir(d.path().join("cache"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(left.iter().all(|n| !n.starts_with(".staging-")), "{left:?}");
}

#[test]
fn a_failed_build_stores_nothing() {
    let (d, a, _) = trees();
    let c = cache(d.path());
    assert!(c.fetch("node-modules-k", MARK, &a, || Err("npm ci failed".into())).is_err());
    assert!(!d.path().join("cache/node-modules-k").exists());
}

fn age(dir: &Path, secs_ago: u64) {
    File::open(dir)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(secs_ago))
        .unwrap();
}

/// An entry of about `kib` KiB.
fn big_entry(c: &Cache, d: &Path, name: &str, kib: usize) {
    let tree = d.join(format!("t-{name}"));
    std::fs::create_dir_all(&tree).unwrap();
    c.fetch(name, MARK, &tree, || {
        fake_install(&tree)?;
        std::fs::write(tree.join("node_modules/big.bin"), vec![7u8; kib * 1024]).map_err(|e| e.to_string())?;
        Ok(vec!["node_modules".into()])
    })
    .unwrap();
}

#[test]
fn eviction_removes_the_least_recently_used_until_the_cap_holds() {
    let d = tempfile::tempdir().unwrap();
    let c = Cache::with_cap_bytes(d.path().join("cache"), 250 * 1024, WAIT);
    big_entry(&c, d.path(), "node-modules-old", 100);
    big_entry(&c, d.path(), "node-modules-mid", 100);
    age(&d.path().join("cache/node-modules-old"), 3000);
    age(&d.path().join("cache/node-modules-mid"), 2000);
    big_entry(&c, d.path(), "node-modules-new", 100); // 300 KiB in all: over the cap by one entry
    let there = |n: &str| d.path().join("cache").join(n).is_dir();
    assert!(!there("node-modules-old"), "the least recently used goes");
    assert!(there("node-modules-mid") && there("node-modules-new"));
    assert!(!d.path().join("cache/node-modules-old.lock").exists());
}

#[test]
fn eviction_never_removes_the_entry_just_used_or_one_that_is_locked() {
    let d = tempfile::tempdir().unwrap();
    let c = Cache::with_cap_bytes(d.path().join("cache"), 1, WAIT); // every entry is over the cap
    big_entry(&c, d.path(), "node-modules-held", 10);
    age(&d.path().join("cache/node-modules-held"), 5000);
    let holder = OpenOptions::new()
        .read(true)
        .write(true)
        .open(d.path().join("cache/node-modules-held.lock"))
        .unwrap();
    holder.try_lock_shared().unwrap(); // another run is cloning it out
    big_entry(&c, d.path(), "node-modules-fresh", 10);
    assert!(
        d.path().join("cache/node-modules-fresh").is_dir(),
        "the entry just built stays though it is over the cap"
    );
    assert!(d.path().join("cache/node-modules-held").is_dir(), "a locked entry stays");
    drop(holder);
    c.evict("node-modules-fresh").unwrap();
    assert!(!d.path().join("cache/node-modules-held").exists(), "once free it goes");
    assert!(d.path().join("cache/node-modules-fresh").is_dir());
}

#[test]
fn try_hit_never_waits_and_never_builds() {
    let (d, a, b) = trees();
    let c = cache(d.path());
    assert!(!c.try_hit("node-modules-k", MARK, &b).unwrap(), "no entry: no hit");
    c.fetch("node-modules-k", MARK, &a, || fake_install(&a)).unwrap();
    let builder = OpenOptions::new()
        .read(true)
        .write(true)
        .open(d.path().join("cache/node-modules-k.lock"))
        .unwrap();
    builder.try_lock().unwrap(); // someone holds it exclusively
    assert!(!c.try_hit("node-modules-k", MARK, &b).unwrap());
    assert!(!b.join("node_modules").exists());
    drop(builder);
    assert!(c.try_hit("node-modules-k", MARK, &b).unwrap());
    assert!(b.join("node_modules/pkg/index.js").is_file());
}

#[test]
fn a_waiter_gives_up_with_the_cause_and_the_next_step() {
    let (d, a, b) = trees();
    let c = Cache::with_cap_bytes(d.path().join("cache"), u64::MAX, Duration::from_millis(300));
    c.fetch("node-modules-k", MARK, &a, || fake_install(&a)).unwrap();
    let builder = OpenOptions::new()
        .read(true)
        .write(true)
        .open(d.path().join("cache/node-modules-k.lock"))
        .unwrap();
    builder.try_lock().unwrap();
    let e = c.fetch("node-modules-k", MARK, &b, never).unwrap_err();
    assert!(e.contains("did not finish") && e.contains("Next:"), "{e}");
}

#[test]
fn the_cache_declares_its_owner_cap_and_eviction_rule() {
    let (d, a, _) = trees();
    let c = Cache::new(d.path().join("cache"), 7, WAIT);
    c.fetch("node-modules-k", MARK, &a, || fake_install(&a)).unwrap();
    let owner: Value = serde_json::from_str(&std::fs::read_to_string(d.path().join("cache/owner.json")).unwrap()).unwrap();
    assert_eq!(owner["cap_bytes"], 7u64 << 30);
    assert!(owner["owner"].as_str().unwrap().contains("./dev deps") && owner["eviction"].as_str().unwrap().contains("least recently used"));
}

#[test]
fn workspace_folders_come_from_package_json() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    std::fs::write(r.join("package.json"), r#"{"workspaces": ["ui", "apps/*/app", "gone"]}"#).unwrap();
    for dir in [
        "node_modules",
        "ui/node_modules",
        "apps/one/app/node_modules",
        "apps/two/app",
        "apps/three/app/node_modules",
    ] {
        std::fs::create_dir_all(r.join(dir)).unwrap();
    }
    assert_eq!(
        node_roots(r),
        [
            "apps/one/app/node_modules",
            "apps/three/app/node_modules",
            "node_modules",
            "ui/node_modules"
        ]
    );
}

#[test]
fn clone_tree_keeps_links_and_refuses_nothing_it_can_copy() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("src");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("sub/f"), "f").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("sub/f", src.join("l")).unwrap();
    let dest = d.path().join("dest");
    clone_tree(d.path(), &src, &dest).unwrap();
    assert_eq!(std::fs::read_to_string(dest.join("sub/f")).unwrap(), "f");
    #[cfg(unix)]
    assert_eq!(std::fs::read_link(dest.join("l")).unwrap(), Path::new("sub/f"));
    assert!(clone_tree(d.path(), &d.path().join("missing"), &d.path().join("dest2")).is_err());
    assert!(!d.path().join("dest2").exists());
}
