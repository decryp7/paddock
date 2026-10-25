//! The scanner: finds what a category could remove and proposes it as [`Candidate`]s; only what
//! [`dyno_core::cleanup::admit`] accepts reaches the preview. A scan deletes nothing and never follows a link.

use super::roots::{folders, Env};
use super::walk::{measure, modified, owner, Budget};
use dyno_core::cleanup::{
    admit, dev_artifact, is_installer, result, stale_toolchains, Candidate, CategoryResult, CategorySpec, CleanupItem, Selector,
    PERSONAL_DIRS,
};
use dyno_core::time::{Timestamp, DAY};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The most directory entries read in one folder.
pub const MAX_ENTRIES_PER_DIR: usize = 20_000;
/// The most entries measured for one item.
pub const ITEM_BUDGET: usize = 200_000;
/// The most entries visited looking for idle projects.
pub const PROJECT_WALK_BUDGET: usize = 60_000;
/// How deep below the home folder projects are looked for.
pub const PROJECT_DEPTH: usize = 4;

/// What a scan needs besides the category.
pub struct ScanCtx<'a> {
    /// The folders of this OS.
    pub env: &'a Env,
    /// Now.
    pub now: Timestamp,
    /// Projects idle this many days offer their build output.
    pub dev_age_days: u32,
    /// The scan stops here and says the list is partial.
    pub deadline: Instant,
    /// Only files owned by this user are offered (Unix shared folders such as /tmp).
    pub owner: Option<u32>,
}

fn candidate(path: &Path, meta: &std::fs::Metadata, skip_owner: Option<u32>, ctx: &ScanCtx) -> Option<Candidate> {
    let link = meta.file_type().is_symlink();
    if !link && skip_owner.is_some() && owner(meta) != skip_owner {
        return None;
    }
    let mut budget = Budget {
        entries: ITEM_BUDGET,
        deadline: ctx.deadline,
    };
    let m = if link {
        super::walk::Measure {
            bytes: 0,
            newest: modified(meta),
            complete: true,
        }
    } else {
        measure(path, meta, None, &mut budget)
    };
    Some(Candidate {
        path: path.to_string_lossy().into_owned(),
        bytes: m.bytes,
        modified: m.newest,
        folder: meta.is_dir(),
        link,
        reason: None,
    })
}

fn age_reason(c: &Candidate, now: Timestamp) -> String {
    let days = now.since(c.modified).max(0) / DAY;
    match days {
        0 => "Changed today".into(),
        1 => "Not changed for a day".into(),
        n => format!("Not changed for {n} days"),
    }
}

fn children(root: &Path, installers: bool, ctx: &ScanCtx, out: &mut Vec<Candidate>, partial: &mut bool) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for (n, e) in rd.flatten().enumerate() {
        if n >= MAX_ENTRIES_PER_DIR || Instant::now() >= ctx.deadline {
            *partial = true;
            return;
        }
        let path = e.path();
        let Ok(meta) = path.symlink_metadata() else { continue };
        if installers && (!meta.is_file() || !is_installer(&e.file_name().to_string_lossy())) {
            continue;
        }
        out.extend(candidate(&path, &meta, ctx.owner, ctx));
    }
}

fn is_personal_or_hidden(home: &Path, dir: &Path) -> bool {
    let Ok(rel) = dir.strip_prefix(home) else { return false };
    let first = rel
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default();
    first.starts_with('.')
        || ["Library", "AppData", "Applications", "node_modules", "target"]
            .iter()
            .any(|d| first.eq_ignore_ascii_case(d))
        || PERSONAL_DIRS
            .iter()
            .any(|d| d.split('/').next().is_some_and(|f| first.eq_ignore_ascii_case(f)))
}

fn projects(home: &Path, ctx: &ScanCtx, out: &mut Vec<Candidate>, partial: &mut bool) {
    let mut left = PROJECT_WALK_BUDGET;
    let mut stack: Vec<(PathBuf, usize)> = vec![(home.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let entries: Vec<_> = rd.flatten().take(MAX_ENTRIES_PER_DIR).collect();
        let markers: Vec<String> = entries.iter().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        for e in &entries {
            if left == 0 || Instant::now() >= ctx.deadline {
                *partial = true;
                return;
            }
            left -= 1;
            let path = e.path();
            let Ok(meta) = path.symlink_metadata() else { continue };
            if !meta.is_dir() || meta.file_type().is_symlink() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if name == "target" || name == "node_modules" {
                let mut budget = Budget {
                    entries: 20_000,
                    deadline: ctx.deadline,
                };
                let parent = measure(
                    &dir,
                    &dir.symlink_metadata().unwrap_or(meta.clone()),
                    Some(e.file_name().as_os_str()),
                    &mut budget,
                );
                let reason = dev_artifact(&name, &markers, parent.newest, ctx.now, ctx.dev_age_days);
                if let Some(reason) = reason {
                    if let Some(mut c) = candidate(&path, &meta, ctx.owner, ctx) {
                        c.reason = Some(reason);
                        out.push(c);
                    }
                }
            } else if depth < PROJECT_DEPTH && !name.starts_with('.') && !(depth == 0 && is_personal_or_hidden(home, &path)) {
                stack.push((path, depth + 1));
            }
        }
    }
}

fn toolchains(root: &Path, env: &Env, ctx: &ScanCtx, out: &mut Vec<Candidate>, partial: &mut bool) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let dirs: Vec<(String, PathBuf)> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    let settings = std::fs::read_to_string(env.rustup().join("settings.toml")).unwrap_or_default();
    let doc: toml::Table = toml::from_str(&settings).unwrap_or_default();
    let default = doc.get("default_toolchain").and_then(|v| v.as_str()).map(String::from);
    let overrides: Vec<String> = doc
        .get("overrides")
        .and_then(|v| v.as_table())
        .map(|t| t.values().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let installed: Vec<String> = dirs.iter().map(|(n, _)| n.clone()).collect();
    for name in stale_toolchains(&installed, default.as_deref(), &overrides) {
        if let Some((_, path)) = dirs.iter().find(|(n, _)| *n == name) {
            if Instant::now() >= ctx.deadline {
                *partial = true;
                return;
            }
            let Ok(meta) = path.symlink_metadata() else { continue };
            if let Some(mut c) = candidate(path, &meta, ctx.owner, ctx) {
                c.reason = Some("Not the default toolchain and no override uses it".into());
                out.push(c);
            }
        }
    }
}

/// Scans one category. `id_prefix` makes the item ids unique within the scan.
pub fn scan_category(spec: &CategorySpec, ctx: &ScanCtx, id_prefix: &str) -> CategoryResult {
    let roots = folders(spec, ctx.env);
    if spec.selector == Selector::DockerImages {
        return result(
            &spec.id,
            vec![],
            Some("Docker images are removed through Docker, which Dyno does not call yet. Run `docker image prune` yourself.".into()),
        );
    }
    if roots.is_empty() {
        return result(
            &spec.id,
            vec![],
            Some("Nothing to scan: this folder does not exist on this PC.".into()),
        );
    }
    let mut found = Vec::new();
    let mut partial = false;
    for root in &roots {
        match spec.selector {
            Selector::Children => children(root, false, ctx, &mut found, &mut partial),
            Selector::Installers => children(root, true, ctx, &mut found, &mut partial),
            Selector::DevArtifacts => projects(root, ctx, &mut found, &mut partial),
            Selector::StaleToolchains => toolchains(root, ctx.env, ctx, &mut found, &mut partial),
            Selector::DockerImages => {}
        }
    }
    let root_strings: Vec<String> = roots.iter().map(|r| r.to_string_lossy().into_owned()).collect();
    let home = ctx.env.home.to_string_lossy().into_owned();
    let mut items: Vec<CleanupItem> = Vec::new();
    for (n, mut c) in found.into_iter().enumerate() {
        if c.reason.is_none() {
            c.reason = Some(age_reason(&c, ctx.now));
        }
        let default = spec.name.clone();
        if let Ok(item) = admit(
            format!("{id_prefix}:{}:{n}", spec.id),
            c,
            &root_strings,
            &home,
            spec.risk,
            spec.min_age_days,
            ctx.now,
            &default,
        ) {
            items.push(item);
        }
    }
    let note = partial.then(|| "The scan stopped early (time or size limit), so the list is partial.".to_string());
    result(&spec.id, items, note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::cleanup::pack::{parse, Trust};
    use dyno_core::cleanup::Os;
    use std::time::Duration;

    fn env(home: &Path) -> Env {
        Env {
            os: Some(Os::Linux),
            home: home.to_path_buf(),
            temp: home.join("tmp"),
            ..Default::default()
        }
    }

    fn ctx<'a>(env: &'a Env, now: Timestamp) -> ScanCtx<'a> {
        ScanCtx {
            env,
            now,
            dev_age_days: 30,
            deadline: Instant::now() + Duration::from_secs(30),
            owner: None,
        }
    }

    fn pack(paths: &str, selector: &str, age: u32) -> CategorySpec {
        let text = format!(
            "format = 1\n[[category]]\nid = \"t\"\nname = \"T\"\ndescription = \"d\"\ngroup = \"developer\"\nrisk = \"review\"\npaths = [{paths}]\nselector = \"{selector}\"\nmin_age_days = {age}\n"
        );
        parse(&text, Trust::User).unwrap().remove(0)
    }

    fn old(path: &Path, days: u64) {
        super::super::testing::backdate(path, days);
    }

    fn now() -> Timestamp {
        super::super::testing::now()
    }

    #[test]
    fn children_are_offered_with_sizes_and_the_age_filter() {
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join("cachey");
        std::fs::create_dir_all(cache.join("old-dir")).unwrap();
        std::fs::write(cache.join("old-dir/f"), vec![0u8; 500]).unwrap();
        std::fs::write(cache.join("big.bin"), vec![0u8; 900]).unwrap();
        std::fs::write(cache.join("fresh.bin"), b"x").unwrap();
        old(&cache.join("big.bin"), 10);
        // A folder's age is its newest entry's.
        old(&cache.join("old-dir/f"), 10);
        old(&cache.join("old-dir"), 10);
        let e = env(home.path());
        let r = scan_category(&pack("\"{home}/cachey\"", "children", 5), &ctx(&e, now()), "s1");
        let names: Vec<_> = r
            .items
            .iter()
            .map(|i| Path::new(&i.path).file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["big.bin", "old-dir"], "biggest first, the fresh file is left alone");
        assert_eq!(r.bytes, 1400);
        assert!(r.items.iter().all(|i| !i.selected), "review categories are never pre-selected");
        assert!(r.items[0].reason.starts_with("Not changed for"));
        assert!(r.items[1].folder);
        assert_eq!(r.note, None);
    }

    #[cfg(unix)]
    #[test]
    fn links_are_never_offered() {
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join("cachey");
        std::fs::create_dir_all(&cache).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("precious"), b"keep").unwrap();
        std::os::unix::fs::symlink(outside.path(), cache.join("shortcut")).unwrap();
        std::fs::write(cache.join("real"), b"x").unwrap();
        old(&cache.join("real"), 10);
        let e = env(home.path());
        let r = scan_category(&pack("\"{home}/cachey\"", "children", 0), &ctx(&e, now()), "s");
        assert_eq!(r.item_count, 1);
        assert!(r.items[0].path.ends_with("real"));
    }

    #[test]
    fn installers_in_downloads_are_personal_and_never_selected() {
        let home = tempfile::tempdir().unwrap();
        let dl = home.path().join("Downloads");
        std::fs::create_dir_all(&dl).unwrap();
        for f in ["setup.exe", "tool.dmg", "photo.jpg", "notes.txt"] {
            std::fs::write(dl.join(f), vec![0u8; 10]).unwrap();
            old(&dl.join(f), 60);
        }
        std::fs::create_dir_all(dl.join("folder.pkg")).unwrap();
        let spec = dyno_core::cleanup::catalog()
            .into_iter()
            .find(|c| c.id == "downloadInstallers")
            .unwrap();
        let e = env(home.path());
        let r = scan_category(&spec, &ctx(&e, now()), "s");
        let mut names: Vec<_> = r
            .items
            .iter()
            .map(|i| Path::new(&i.path).file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["setup.exe", "tool.dmg"]);
        assert!(r.items.iter().all(|i| !i.selected));
    }

    #[cfg(unix)]
    #[test]
    fn idle_projects_offer_their_build_output() {
        let home = tempfile::tempdir().unwrap();
        let p = home.path().join("code/old-app");
        std::fs::create_dir_all(p.join("target/debug")).unwrap();
        std::fs::write(p.join("target/debug/x"), vec![0u8; 300]).unwrap();
        std::fs::write(p.join("Cargo.toml"), b"[package]").unwrap();
        old(&p.join("Cargo.toml"), 90);
        std::fs::create_dir_all(p.join("src")).unwrap();
        old(&p.join("src"), 90);
        old(&p.join("target/debug/x"), 1);
        // Active project: untouched target stays.
        let active = home.path().join("code/active");
        std::fs::create_dir_all(active.join("target")).unwrap();
        std::fs::write(active.join("target/y"), b"y").unwrap();
        std::fs::write(active.join("Cargo.toml"), b"[package]").unwrap();
        // Not a project: a `target` folder without Cargo.toml.
        std::fs::create_dir_all(home.path().join("code/plain/target")).unwrap();
        // Personal folders are not searched.
        std::fs::create_dir_all(home.path().join("Documents/proj/node_modules")).unwrap();
        std::fs::write(home.path().join("Documents/proj/package.json"), b"{}").unwrap();
        old(&home.path().join("Documents/proj/package.json"), 400);
        let spec = dyno_core::cleanup::catalog().into_iter().find(|c| c.id == "devArtifacts").unwrap();
        // The project directory's own mtime changed when files were created above, so backdate the folder too.
        old(&p, 90);
        let e = env(home.path());
        let r = scan_category(&spec, &ctx(&e, now()), "s");
        assert_eq!(r.item_count, 1, "{:?}", r.items);
        assert!(r.items[0].path.ends_with("old-app/target"));
        assert!(r.items[0].reason.starts_with("Rust build output; project untouched for"));
        assert_eq!(r.items[0].bytes, 300);
    }

    #[test]
    fn toolchains_not_used_are_offered() {
        let home = tempfile::tempdir().unwrap();
        let rustup = home.path().join(".rustup");
        for t in ["stable-x", "1.70.0-x", "nightly-x"] {
            std::fs::create_dir_all(rustup.join("toolchains").join(t)).unwrap();
            std::fs::write(rustup.join("toolchains").join(t).join("bin"), vec![0u8; 5]).unwrap();
        }
        std::fs::write(
            rustup.join("settings.toml"),
            "default_toolchain = \"stable-x\"\n[overrides]\n\"/some/project\" = \"nightly-x\"\n",
        )
        .unwrap();
        let spec = dyno_core::cleanup::catalog()
            .into_iter()
            .find(|c| c.id == "rustToolchains")
            .unwrap();
        let e = env(home.path());
        let r = scan_category(&spec, &ctx(&e, now()), "s");
        assert_eq!(r.item_count, 1);
        assert!(r.items[0].path.ends_with("1.70.0-x"));
    }

    #[test]
    fn docker_and_missing_folders_explain_themselves() {
        let home = tempfile::tempdir().unwrap();
        let e = env(home.path());
        let c = ctx(&e, now());
        let docker = dyno_core::cleanup::catalog().into_iter().find(|c| c.id == "dockerImages").unwrap();
        assert!(scan_category(&docker, &c, "s").note.unwrap().contains("docker image prune"));
        let r = scan_category(&pack("\"{home}/nope\"", "children", 0), &c, "s");
        assert!(r.items.is_empty() && r.note.unwrap().contains("does not exist"));
    }

    #[test]
    fn a_passed_deadline_gives_a_partial_list() {
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join("cachey");
        std::fs::create_dir_all(&cache).unwrap();
        for i in 0..3 {
            std::fs::write(cache.join(format!("f{i}")), b"x").unwrap();
        }
        let e = env(home.path());
        let mut c = ctx(&e, now());
        c.deadline = Instant::now();
        let r = scan_category(&pack("\"{home}/cachey\"", "children", 0), &c, "s");
        assert!(r.note.unwrap().contains("partial"));
    }

    #[cfg(unix)]
    #[test]
    fn other_users_files_are_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let cache = home.path().join("cachey");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("mine"), b"x").unwrap();
        let e = env(home.path());
        let mut c = ctx(&e, now());
        c.owner = Some(u32::MAX - 7);
        assert_eq!(scan_category(&pack("\"{home}/cachey\"", "children", 0), &c, "s").item_count, 0);
        c.owner = owner(&cache.join("mine").symlink_metadata().unwrap());
        assert_eq!(scan_category(&pack("\"{home}/cachey\"", "children", 0), &c, "s").item_count, 1);
    }
}
