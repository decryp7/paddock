//! Whether the Linux container sees the files the host has (user decision 2026-11-25): before a container row does its work, the
//! content hash of the change's files inside the container is compared with the host's. OrbStack 2.2.5's bind mount served stale
//! files for over an hour after a host change (the old size and mtime, the new text cut at the old length), so a container row could
//! lint or test an old version and pass. Any difference makes the row "not measured" (exit 6), never a pass (ER-33).
//!
//! Both sides run the same probe ([`SCRIPT`], `sh` and `git hash-object --no-filters` in one bulk call, ER-20), read by one parser
//! ([`parse_view`]); the comparison ([`compare`]) is pure. The executor runs the probe on the host and through the container helper
//! (`tools/linux-container.sh`, the one entry of every container row), so the probe reads the same mount the row will.

/// The most files compared per row: the row's own workspaces' files come first, the rest of the change after them. Measured cost
/// (one `git hash-object --stdin-paths` call per side, plus one container start): the probe is one process on each side whatever
/// the count; the cap only bounds the argument list of the container start (ARG_MAX is far above 256 repository paths).
pub const CAP: usize = 256;

/// The probe, run with the repository root as its working folder and the files as its arguments: one line per file (`present` or
/// `absent`), then the blob hash of each present file in the same order. `--no-filters` hashes the bytes on disk, so the host's and
/// the container's git configuration cannot make two equal files differ.
pub const SCRIPT: &str = r#"for f in "$@"; do if [ -f "$f" ]; then echo present; else echo absent; fi; done
for f in "$@"; do if [ -f "$f" ]; then printf '%s\n' "$f"; fi; done | git hash-object --no-filters --stdin-paths"#;

/// What the probe's next step says when the container's view differs from the host's.
pub const STALE_NEXT: &str = "restart OrbStack (or Docker), or the Mac, so the container's bind mount serves the host's files again, then re-run `./dev check --pre-push`; an agent reports this to the coordinator and never restarts it (ER-14). This is not a finding in your change";

/// One side's view of the files: the blob hash of each, `None` when it is not a file there.
pub type View = Vec<Option<String>>;

/// The verdict of the probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountProbe {
    /// Every compared file is the same on both sides.
    Fresh {
        /// The files compared.
        checked: usize,
        /// The files of the change (more than `checked` when the cap cut the list).
        of: usize,
    },
    /// The container sees other content (or another presence) for these files.
    Stale {
        /// The files that differ, with what each side has.
        differing: Vec<String>,
    },
    /// The probe could not look (a side did not run, or printed something it could not read): nothing is proven.
    Broken(String),
}

/// The files a container row's probe compares, set by the planner on every row that runs in the container.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Compared {
    /// The files, at most [`CAP`].
    pub files: Vec<String>,
    /// How many files the change has (more than `files` when the cap cut the list).
    pub of: usize,
}

/// The files the probe compares for a row: the change's files under the row's `workspaces` first, then the others, each once, at
/// most [`CAP`]. Paths holding a newline are left out: the probe reads one path per line.
pub fn files_for(change_files: &[String], workspaces: &[String]) -> Compared {
    let own = |f: &str| workspaces.iter().any(|w| f.starts_with(&format!("{}/", w.trim_end_matches('/'))));
    let mut files: Vec<String> = change_files
        .iter()
        .filter(|f| !f.contains('\n') && !f.is_empty())
        .cloned()
        .collect();
    files.sort();
    files.dedup();
    let (mut first, rest): (Vec<String>, Vec<String>) = files.into_iter().partition(|f| own(f));
    first.extend(rest);
    let of = first.len();
    first.truncate(CAP);
    Compared { files: first, of }
}

/// One side's view from the probe's output for `count` files.
pub fn parse_view(stdout: &str, count: usize) -> Result<View, String> {
    let mut lines = stdout.lines();
    let mut present = Vec::with_capacity(count);
    for i in 0..count {
        match lines.next() {
            Some("present") => present.push(true),
            Some("absent") => present.push(false),
            other => return Err(format!("line {} of the probe's output is {other:?}, not present or absent", i + 1)),
        }
    }
    let mut view = Vec::with_capacity(count);
    for p in present {
        if !p {
            view.push(None);
            continue;
        }
        match lines.next() {
            Some(h) if h.len() >= 40 && h.chars().all(|c| c.is_ascii_hexdigit()) => view.push(Some(h.to_string())),
            other => return Err(format!("the probe printed {other:?} where a blob hash was expected")),
        }
    }
    if let Some(extra) = lines.next() {
        return Err(format!("the probe printed more than it was asked for: {extra:?}"));
    }
    Ok(view)
}

/// The verdict for `files` from the host's and the container's views. Pure.
pub fn compare(files: &[String], host: &[Option<String>], container: &[Option<String>]) -> MountProbe {
    if host.len() != files.len() || container.len() != files.len() {
        return MountProbe::Broken(format!(
            "{} files asked, the host answered {} and the container {}",
            files.len(),
            host.len(),
            container.len()
        ));
    }
    let side = |h: &Option<String>| h.as_deref().map_or("absent".to_string(), |h| h[..h.len().min(12)].to_string());
    let differing: Vec<String> = files
        .iter()
        .zip(host.iter().zip(container))
        .filter(|(_, (h, c))| h != c)
        .map(|(f, (h, c))| format!("{f} (host {}, container {})", side(h), side(c)))
        .collect();
    if differing.is_empty() {
        MountProbe::Fresh {
            checked: files.len(),
            of: files.len(),
        }
    } else {
        MountProbe::Stale { differing }
    }
}

/// The probe over `files`, with `host` and `container` running [`SCRIPT`] on each side (their stdout, or why they could not run).
/// No file: nothing to compare, so the probe cannot have seen the mount (ER-33): broken, never fresh.
pub fn check(
    files: &[String],
    of: usize,
    host: impl FnOnce(&[String]) -> Result<String, String>,
    container: impl FnOnce(&[String]) -> Result<String, String>,
) -> MountProbe {
    if files.is_empty() {
        return MountProbe::Broken("the change names no file to compare".into());
    }
    let host = match host(files).and_then(|out| parse_view(&out, files.len())) {
        Ok(v) => v,
        Err(e) => return MountProbe::Broken(format!("on the host: {e}")),
    };
    let container = match container(files).and_then(|out| parse_view(&out, files.len())) {
        Ok(v) => v,
        Err(e) => return MountProbe::Broken(format!("in the container: {e}")),
    };
    match compare(files, &host, &container) {
        MountProbe::Fresh { checked, .. } => MountProbe::Fresh { checked, of },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_tool::ToolCall;
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::Duration;

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Runs the probe with plain `sh` in `dir`: what the container helper runs inside, here on a folder of the test's own.
    fn probe_in(dir: &Path) -> impl FnOnce(&[String]) -> Result<String, String> + '_ {
        move |files| {
            let env: HashMap<String, String> = paddock_env::vars().collect();
            let ran = ToolCall::new("sh")
                .args(["-c", SCRIPT, "sh"])
                .args(files.iter().cloned())
                .cwd(dir)
                .timeout(Duration::from_secs(60))
                .run(&env, &mut std::io::sink())?;
            if ran.success {
                Ok(ran.stdout)
            } else {
                Err(format!("{}: {}", ran.how, ran.stderr))
            }
        }
    }

    #[test]
    fn the_probe_gives_gits_blob_hashes_and_names_absent_files() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "hello\n").unwrap();
        let files = strs(&["a.txt", "gone.rs"]);
        let out = probe_in(d.path())(&files).unwrap();
        // `printf 'hello\n' | git hash-object --stdin`
        assert_eq!(
            parse_view(&out, 2).unwrap(),
            vec![Some("ce013625030ba8dba906f756967f9e9ca394464a".to_string()), None]
        );
    }

    #[test]
    fn a_container_view_cut_at_the_old_length_is_stale_never_fresh() {
        // Positive control: the OrbStack symptom (the new text cut at the old length) on a second folder standing in for the mount.
        let host = tempfile::tempdir().unwrap();
        let mount = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(host.path().join("crates/x")).unwrap();
        std::fs::create_dir_all(mount.path().join("crates/x")).unwrap();
        std::fs::write(host.path().join("crates/x/lib.rs"), "pub fn new_text() {}\n").unwrap();
        std::fs::write(mount.path().join("crates/x/lib.rs"), "pub fn new_t").unwrap();
        std::fs::write(host.path().join("same.txt"), "same\n").unwrap();
        std::fs::write(mount.path().join("same.txt"), "same\n").unwrap();
        let files = strs(&["crates/x/lib.rs", "same.txt"]);
        match check(&files, 2, probe_in(host.path()), probe_in(mount.path())) {
            MountProbe::Stale { differing } => {
                assert_eq!(differing.len(), 1, "{differing:?}");
                assert!(differing[0].starts_with("crates/x/lib.rs (host "), "{differing:?}");
            }
            other => panic!("a stale view must not pass: {other:?}"),
        }
        // And the same folder on both sides is fresh: the probe can tell the two apart.
        assert_eq!(
            check(&files, 5, probe_in(host.path()), probe_in(host.path())),
            MountProbe::Fresh { checked: 2, of: 5 }
        );
    }

    #[test]
    fn a_file_only_one_side_has_is_stale() {
        let files = strs(&["new.rs"]);
        assert!(matches!(
            compare(&files, &[Some("a".repeat(40))], &[None]),
            MountProbe::Stale { .. }
        ));
        assert!(matches!(
            compare(&files, &[None], &[Some("a".repeat(40))]),
            MountProbe::Stale { .. }
        ));
    }

    #[test]
    fn a_probe_that_cannot_look_is_broken_never_fresh() {
        let files = strs(&["a.rs"]);
        let ok = |_: &[String]| Ok("absent\n".to_string());
        assert!(matches!(
            check(&files, 1, ok, |_: &[String]| Err("exit 125".to_string())),
            MountProbe::Broken(e) if e.contains("in the container: exit 125")
        ));
        assert!(matches!(
            check(&files, 1, ok, |_: &[String]| Ok(String::new())),
            MountProbe::Broken(_)
        ));
        assert!(matches!(check(&[], 0, ok, ok), MountProbe::Broken(_)));
        assert!(parse_view("present\nnot-a-hash\n", 1).is_err());
        assert!(parse_view("absent\nextra\n", 1).is_err());
    }

    #[test]
    fn the_rows_own_workspaces_come_first_and_the_list_is_capped() {
        let files = strs(&["z/top.rs", "apps/dyno/src/a.rs", "z/top.rs", "docs/x.md"]);
        assert_eq!(
            files_for(&files, &strs(&["apps/dyno"])),
            Compared {
                files: strs(&["apps/dyno/src/a.rs", "docs/x.md", "z/top.rs"]),
                of: 3
            }
        );
        let many: Vec<String> = (0..CAP + 10).map(|i| format!("f{i:04}")).collect();
        let capped = files_for(&many, &[]);
        assert_eq!((capped.files.len(), capped.of), (CAP, CAP + 10));
    }
}
