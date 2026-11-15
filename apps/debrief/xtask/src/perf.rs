//! `perf-import` and `perf-post`: the macOS performance runs (ports of `scripts/perf/perf-import.sh` and `perf-post.sh`). The sampler
//! (`procsample.py`: physical footprint, RSS, CPU and wake-ups through `proc_pid_rusage`) and the summary (`perfsum.py`) stay Python
//! files in `scripts/perf/`; these tasks start them. Environment: `DEBRIEF_PERF_BIN` (the release target folder), `DEBRIEF_LIB_DIR`
//! (the libduckdb folder) and `DEBRIEF_PERF_OUT` (the results folder).

use crate::command;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Instant, SystemTime};

/// The three folders every perf run needs, from the environment (an error names the variable).
pub struct Folders {
    pub bin: PathBuf,
    pub lib: PathBuf,
    pub out: PathBuf,
}

/// Reads [`Folders`] through `get`.
pub fn folders(get: impl Fn(&str) -> Option<String>) -> Result<Folders, String> {
    let need = |name: &str, what: &str| {
        get(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| format!("set {name} to {what}"))
    };
    Ok(Folders {
        bin: need("DEBRIEF_PERF_BIN", "the release target folder")?,
        lib: need("DEBRIEF_LIB_DIR", "the libduckdb folder")?,
        out: need("DEBRIEF_PERF_OUT", "a results folder")?,
    })
}

/// A store folder that is safe to delete before an import: a folder path with a parent and at least two components, never a
/// root, the home folder or something with `..`.
pub fn store_ok(store: &Path, home: Option<&Path>) -> Result<(), String> {
    let comps = store.components().count();
    let bad = comps < 3
        || store.components().any(|c| matches!(c, std::path::Component::ParentDir))
        || home.is_some_and(|h| store == h)
        || store.parent().is_none();
    if bad {
        Err(format!(
            "the store folder {} is replaced on every run: name a folder of its own (a subfolder at least two levels deep)",
            store.display()
        ))
    } else {
        Ok(())
    }
}

/// The summary line appended to `summary.txt`.
pub fn summary_line(label: &str, rc: i32, wall_secs: f64, store_size: &str) -> String {
    format!("{label} rc={rc} wall={wall_secs:.1}s store={store_size}")
}

/// The newest file in `candidates` (path, modified time).
pub fn newest(candidates: Vec<(PathBuf, SystemTime)>) -> Option<PathBuf> {
    candidates.into_iter().max_by_key(|(_, t)| *t).map(|(p, _)| p)
}

/// True for a `perf_idle-<hash>` test executable: not a `.d`, `.o` or `.rlib` file.
pub fn is_perf_idle(name: &str) -> bool {
    name.starts_with("perf_idle-") && ![".d", ".o", ".rlib"].iter().any(|e| name.ends_with(e))
}

fn find_files(dir: &Path, out: &mut Vec<(PathBuf, SystemTime)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            find_files(&p, out);
        } else if is_perf_idle(&e.file_name().to_string_lossy()) && is_exec(&meta) {
            out.push((p, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
        }
    }
}

#[cfg(unix)]
fn is_exec(m: &std::fs::Metadata) -> bool {
    std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o100 != 0
}

#[cfg(not(unix))]
fn is_exec(_: &std::fs::Metadata) -> bool {
    true
}

fn uptime() -> String {
    command("uptime")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn append(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn python_helper(root: &Path, name: &str) -> PathBuf {
    root.join("scripts/perf").join(name)
}

fn sampler(root: &Path, pid: u32, csv: &Path) -> Result<std::process::Child, String> {
    command("python3")
        .args([
            "-I",
            &python_helper(root, "procsample.py").to_string_lossy(),
            &pid.to_string(),
            &csv.to_string_lossy(),
            "0.25",
        ])
        .spawn()
        .map_err(|e| format!("could not start python3 for procsample.py: {e}"))
}

fn summarise(root: &Path, args: &[&str]) {
    let mut a = vec!["-I".to_string(), python_helper(root, "perfsum.py").to_string_lossy().into_owned()];
    a.extend(args.iter().map(|s| (*s).to_string()));
    let _ = command("python3").args(a).status();
}

fn store_size(store: &Path) -> String {
    command("du")
        .args(["-sh", &store.to_string_lossy()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().next().map(str::to_string))
        .unwrap_or_else(|| "?".to_string())
}

/// `cargo xtask perf-import LABEL CORPUS_DIR STORE_DIR`: imports the corpus (synthetic data only, for example from `gen_large.py`)
/// with the release CLI and store worker, sampling the CLI and its children every 250 ms.
pub fn import(args: &[String], root: &Path) -> Result<(), String> {
    let [label, corpus, store] = args else {
        return Err("usage: cargo xtask perf-import LABEL CORPUS_DIR STORE_DIR".to_string());
    };
    let f = folders(|k| std::env::var(k).ok())?;
    let store = PathBuf::from(store);
    store_ok(&store, std::env::var_os("HOME").map(PathBuf::from).as_deref())?;
    std::fs::create_dir_all(&f.out).map_err(|e| format!("cannot create {}: {e}", f.out.display()))?;
    let _ = std::fs::remove_dir_all(&store);
    std::fs::write(f.out.join(format!("{label}.uptime")), uptime()).map_err(|e| format!("cannot write the uptime file: {e}"))?;
    let (json, err) = (
        std::fs::File::create(f.out.join(format!("{label}.json"))).map_err(|e| e.to_string())?,
        std::fs::File::create(f.out.join(format!("{label}.err"))).map_err(|e| e.to_string())?,
    );
    let t0 = Instant::now();
    // The release CLI starts only the store worker that sits in its own folder, so the folder is the one the CLI is run from.
    let mut cli = command(&f.bin.join("debrief").to_string_lossy())
        .args(["import", corpus, "--store", &store.to_string_lossy(), "--json"])
        .env("DYLD_LIBRARY_PATH", &f.lib)
        .stdout(Stdio::from(json))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| {
            format!(
                "could not start {}: {e}; build the release CLI first",
                f.bin.join("debrief").display()
            )
        })?;
    let mut samp = sampler(root, cli.id(), &f.out.join(format!("{label}.csv")))?;
    let rc = cli.wait().map_err(|e| e.to_string())?.code().unwrap_or(-1);
    let _ = samp.wait();
    let line = summary_line(label, rc, t0.elapsed().as_secs_f64(), &store_size(&store));
    println!("{line}");
    append(&f.out.join("summary.txt"), &format!("{line}\n"))?;
    append(&f.out.join(format!("{label}.uptime")), &uptime())?;
    summarise(root, &[&f.out.join(format!("{label}.csv")).to_string_lossy()]);
    Ok(())
}

/// `cargo xtask perf-post LABEL STORE_DIR [IDLE_S]`: reopens a store an import wrote with the `perf_idle` harness (ignored by
/// default) and samples it; prints the per-phase summary.
pub fn post(args: &[String], root: &Path) -> Result<(), String> {
    let (label, store, idle) = match args {
        [l, s] => (l, s, "60"),
        [l, s, i] => (l, s, i.as_str()),
        _ => return Err("usage: cargo xtask perf-post LABEL STORE_DIR [IDLE_S]".to_string()),
    };
    let f = folders(|k| std::env::var(k).ok())?;
    // Both cargo build-folder layouts: deps/ (before 1.100) and build/<package>/<hash>/out/ (1.100).
    let mut found = Vec::new();
    for sub in ["deps", "build"] {
        find_files(&f.bin.join(sub), &mut found);
    }
    let test = newest(found).ok_or_else(|| {
        format!(
            "::error::no perf_idle-* test binary under {} (deps/ or build/*/*/out/); next step: build it with cargo test --release -p debrief-store-duck --no-run",
            f.bin.display()
        )
    })?;
    std::fs::create_dir_all(&f.out).map_err(|e| format!("cannot create {}: {e}", f.out.display()))?;
    let crate_dir = root.join("crates/debrief-store-duck");
    std::fs::write(f.out.join(format!("{label}-post.uptime")), uptime()).map_err(|e| e.to_string())?;
    let log_path = f.out.join(format!("{label}-post.log"));
    let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;
    let mut child = command(&test.to_string_lossy())
        .args(["--ignored", "--nocapture", "--test-threads", "1"])
        .current_dir(&crate_dir)
        .env("DYLD_LIBRARY_PATH", &f.lib)
        .env("DEBRIEF_PERF_STORE", store)
        .env("DEBRIEF_PERF_IDLE_S", idle)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log2))
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", test.display()))?;
    let csv = f.out.join(format!("{label}-post.csv"));
    let mut samp = sampler(root, child.id(), &csv)?;
    let rc = child.wait().map_err(|e| e.to_string())?.code().unwrap_or(-1);
    println!("rc={rc}");
    let _ = samp.wait();
    append(&f.out.join(format!("{label}-post.uptime")), &uptime())?;
    if let Ok(text) = std::fs::read_to_string(&log_path) {
        for l in text
            .lines()
            .filter(|l| !(l.starts_with("PHASE") || l.starts_with("test ") || l.is_empty() || l.starts_with("running")))
        {
            println!("{l}");
        }
    }
    summarise(root, &[&csv.to_string_lossy(), &log_path.to_string_lossy()]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_missing_folder_variable_is_named() {
        let get = |k: &str| (k != "DEBRIEF_LIB_DIR").then(|| "/x".to_string());
        assert_eq!(folders(get).err().unwrap(), "set DEBRIEF_LIB_DIR to the libduckdb folder");
        let all = folders(|_| Some("/x".to_string())).unwrap();
        assert_eq!(
            (all.bin, all.lib, all.out),
            (PathBuf::from("/x"), PathBuf::from("/x"), PathBuf::from("/x"))
        );
    }

    #[test]
    fn the_store_that_is_replaced_every_run_must_be_a_folder_of_its_own() {
        for bad in ["/", "/Users", "/Users/me", "rel", "/a/../b/c/d"] {
            let home = Path::new("/Users/me");
            assert!(store_ok(Path::new(bad), Some(home)).is_err(), "{bad}");
        }
        assert!(store_ok(Path::new("/Volumes/d/stores/mid550"), Some(Path::new("/Users/me"))).is_ok());
    }

    #[test]
    fn the_summary_line_is_the_scripts() {
        assert_eq!(summary_line("mid550", 0, 12.34, "1.2G"), "mid550 rc=0 wall=12.3s store=1.2G");
    }

    #[test]
    fn the_test_binary_is_the_newest_perf_idle_executable_in_either_cargo_layout() {
        assert!(
            is_perf_idle("perf_idle-1a2b")
                && !is_perf_idle("perf_idle-1a2b.d")
                && !is_perf_idle("perf_idle-1a2b.rlib")
                && !is_perf_idle("other-1")
        );
        let t = |s| SystemTime::UNIX_EPOCH + Duration::from_secs(s);
        assert_eq!(
            newest(vec![(PathBuf::from("deps/a"), t(1)), (PathBuf::from("build/b"), t(9))]),
            Some(PathBuf::from("build/b"))
        );
        assert_eq!(newest(vec![]), None);
    }

    #[test]
    fn find_files_reads_both_layouts_and_skips_non_executables() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("build/pkg/h/out/perf_idle-aa");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "x").unwrap();
        std::fs::write(d.path().join("build/pkg/h/out/perf_idle-aa.d"), "x").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&exe, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let mut found = Vec::new();
        find_files(&d.path().join("build"), &mut found);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, exe);
    }
}
