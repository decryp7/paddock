//! The configuration probes: compile cache, cargo configuration, git, npm and the Angular cache.

use super::ledger::Kind;
use super::verify::{Ctx, Env, Probe};
use paddock_env::winpath;
use paddock_proc::{Limits, Outcome, command, run_bounded};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

const GIT_TIMEOUT: Duration = Duration::from_secs(60);

/// No compile cache: no wrapper, no remote backend, and 0 sccache requests.
pub(crate) fn compile_cache(ctx: &mut Ctx) {
    for var in ["RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"] {
        if let Some(v) = ctx.env.get(var) {
            ctx.add(
                Probe::CompileCache,
                format!("{var} is `{v}`: a compiler wrapper (a compile cache) is on."),
                "Release mode leaves it unset (ADR 0085 §4 item 2): remove the step that sets it.",
            );
        }
    }
    for var in [
        "SCCACHE_REDIS",
        "SCCACHE_ENDPOINT",
        "SCCACHE_WEBDAV_ENDPOINT",
        "SCCACHE_MEMCACHED",
        "SCCACHE_BUCKET",
    ] {
        if ctx.env.get(var).is_some() {
            ctx.add(
                Probe::CompileCache,
                format!("{var} names a remote compile cache."),
                "No remote cache in a release job (ADR 0085 D9).",
            );
        }
    }
    let wanted = ctx.opts.expect.contains(&Kind::Sccache) || ctx.ledger.get(Kind::Sccache).is_some();
    match (&ctx.opts.sccache_stats, wanted) {
        (None, true) => ctx.add(
            Probe::CompileCache,
            "check broken: sccache is expected `off` but no statistics were given, so \"0 requests\" was not measured.",
            "After the build run `sccache --show-stats --stats-format=json > <file>` and pass `--sccache-stats <file>`.",
        ),
        (Some(file), _) => match std::fs::read_to_string(file)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string()))
        {
            Err(e) => ctx.add(
                Probe::CompileCache,
                format!("check broken: the sccache statistics {} cannot be read: {e}.", file.display()),
                "Re-run the stats command.",
            ),
            Ok(v) => match v
                .pointer("/stats/compile_requests")
                .or_else(|| v.get("compile_requests"))
                .and_then(serde_json::Value::as_u64)
            {
                None => ctx.add(
                    Probe::CompileCache,
                    format!("check broken: {} has no compile_requests number.", file.display()),
                    "Use `--stats-format=json`.",
                ),
                Some(0) => {}
                Some(n) => ctx.add(
                    Probe::CompileCache,
                    format!("sccache served {n} compile request(s): a compile cache was on."),
                    "Release mode starts no sccache server and sets no RUSTC_WRAPPER.",
                ),
            },
        },
        (None, false) => {}
    }
}

/// `git ls-files` of the clone: the tagged tree's paths, `/`-separated.
fn tracked(env: &Env, clone: &Path) -> Result<BTreeSet<String>, String> {
    let out = git(env, clone, &["ls-files", "-z"])?;
    if out.0 != Some(0) {
        return Err(format!("`git ls-files` exited with {:?}: {}", out.0, out.2));
    }
    Ok(out.1.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect())
}

/// How much of git's standard error a finding quotes: its end, where the cause is.
const STDERR_TAIL: usize = 2048;

/// The last `max` bytes of `text`, cut at a character boundary.
fn tail_text(text: &str, max: usize) -> String {
    let t = text.trim();
    if t.len() <= max {
        return t.to_string();
    }
    let mut from = t.len() - max;
    while !t.is_char_boundary(from) {
        from += 1;
    }
    t[from..].to_string()
}

/// Runs git in `dir` with the job's environment (and no repository override), returning the exit code, the standard output and the
/// end of the standard error. Callers read the output; the standard error is only for a `check broken` message, because git may
/// print warnings on a run whose answer is fine.
fn git(env: &Env, dir: &Path, args: &[&str]) -> Result<(Option<i32>, String, String), String> {
    let mut cmd = command("git");
    cmd.args(args).current_dir(dir).env_clear();
    for (k, v) in &env.0 {
        if !matches!(
            k.as_str(),
            "GIT_DIR" | "GIT_WORK_TREE" | "GIT_INDEX_FILE" | "GIT_OBJECT_DIRECTORY" | "GIT_COMMON_DIR"
        ) {
            cmd.env(k, v);
        }
    }
    let out = run_bounded(cmd, Limits::new(GIT_TIMEOUT)).map_err(|e| format!("git cannot be run: {e}"))?;
    match out.outcome {
        Outcome::Exited(s) => Ok((s.code(), out.stdout_text(), tail_text(&out.stderr_text(), STDERR_TAIL))),
        _ => Err("git did not finish in time".to_string()),
    }
}

fn inside_tagged_tree(canonical: &Path, clone: &Path, files: &BTreeSet<String>) -> bool {
    canonical
        .strip_prefix(clone)
        .map(|rel| files.contains(&rel.to_string_lossy().replace('\\', "/")))
        .unwrap_or(false)
}

/// Every cargo configuration file cargo would read is a file of the tagged tree, or absent.
pub(crate) fn cargo_config(ctx: &mut Ctx) {
    let Some(clone) = ctx.folder(Kind::Workspace) else { return };
    let files = match tracked(ctx.env, &clone) {
        Ok(f) => f,
        Err(e) => {
            ctx.add(
                Probe::CargoConfig,
                format!("check broken: {e}."),
                "The clone must be a git checkout of the tag.",
            );
            return;
        }
    };
    // The clone must be the tagged commit: a tracked file means "of the tag" only then.
    match ctx.env.get("GITHUB_SHA") {
        None => ctx.add(
            Probe::CargoConfig,
            "check broken: GITHUB_SHA is not set, so the clone cannot be compared with the tagged commit.",
            "Run verify inside the CI job.",
        ),
        Some(sha) => match git(ctx.env, &clone, &["rev-parse", "HEAD"]) {
            Ok((Some(0), out, _)) if out.trim() == sha => {}
            other => ctx.add(
                Probe::CargoConfig,
                format!("the clone's HEAD ({other:?}) is not GITHUB_SHA {sha}."),
                "The clone must be a checkout of the tagged commit.",
            ),
        },
    }
    let stop = ctx.opts.walk_stop.as_ref().and_then(|p| winpath::real(p).ok());
    let mut starts: Vec<PathBuf> = Vec::new();
    for w in &ctx.opts.cargo_workspaces {
        match winpath::real(&clone.join(w)) {
            Ok(p) => starts.push(p),
            Err(_) => ctx.add(
                Probe::CargoConfig,
                format!("check broken: the Cargo workspace {w} is not in the clone, so its parent folders cannot be walked."),
                "Fix --cargo-workspace.",
            ),
        }
    }
    if starts.is_empty() && ctx.opts.cargo_workspaces.is_empty() {
        starts.push(clone.clone());
    }
    let mut checked: BTreeSet<PathBuf> = BTreeSet::new();
    let mut candidates: Vec<PathBuf> = Vec::new();
    for start in &starts {
        for dir in start.ancestors() {
            for name in ["config.toml", "config"] {
                candidates.push(dir.join(".cargo").join(name));
            }
            if stop.as_deref() == Some(dir) {
                break;
            }
        }
    }
    if let Some(home) = ctx.folder(Kind::CargoHome) {
        for name in ["config.toml", "config"] {
            candidates.push(home.join(name));
        }
    }
    for c in candidates {
        if !c.is_file() || !checked.insert(c.clone()) {
            continue;
        }
        let canonical = winpath::real(&c).unwrap_or_else(|_| c.clone());
        if !inside_tagged_tree(&canonical, &clone, &files) {
            ctx.add(
                Probe::CargoConfig,
                format!("{} is a cargo configuration that is not a file of the tagged tree.", canonical.display()),
                "Cargo reads it for every build (rustc-wrapper, [source], linker, rustflags, [env]): move it away from the runner or remove it.",
            );
        } else {
            let rel = canonical
                .strip_prefix(&clone)
                .unwrap_or(&canonical)
                .to_string_lossy()
                .replace('\\', "/");
            if !matches!(git(ctx.env, &clone, &["diff", "--quiet", "HEAD", "--", &rel]), Ok((Some(0), _, _))) {
                ctx.add(
                    Probe::CargoConfig,
                    format!("{} differs from the commit (`git diff HEAD` is not empty).", canonical.display()),
                    "A tracked cargo configuration was edited after checkout: the tag's own file is the only one allowed.",
                );
            }
        }
    }
    // `.cargo/ci.toml` is written by build-folder: it may hold only `[build] target-dir`.
    let ci = clone.join(".cargo").join("ci.toml");
    if ci.is_file() {
        match std::fs::read_to_string(&ci)
            .map_err(|e| e.to_string())
            .and_then(|t| t.parse::<toml::Table>().map_err(|e| e.to_string()))
        {
            Err(e) => ctx.add(
                Probe::CargoConfig,
                format!("check broken: {} cannot be read: {e}.", ci.display()),
                "Fix the file build-folder wrote.",
            ),
            Ok(t) => {
                let build = t.get("build").and_then(toml::Value::as_table);
                let only_target = t.len() == 1 && build.is_some_and(|b| b.len() == 1 && b.contains_key("target-dir"));
                if !only_target {
                    ctx.add(
                        Probe::CargoConfig,
                        format!("{} holds more than `[build] target-dir`.", ci.display()),
                        "build-folder writes only the target-dir.",
                    );
                } else if let (Some(dir), Some(target)) = (
                    build.and_then(|b| b.get("target-dir")).and_then(toml::Value::as_str),
                    ctx.folder(Kind::Target),
                ) && winpath::real(Path::new(dir)).map(|p| p != target).unwrap_or(true)
                {
                    ctx.add(
                        Probe::CargoConfig,
                        format!(
                            "{} sets target-dir {dir}, not the build folder {} of the ledger.",
                            ci.display(),
                            target.display()
                        ),
                        "build-folder must write the folder `setup` made.",
                    );
                }
            }
        }
    }
}

/// No `core.hooksPath` and no `url.*.insteadOf`, with no scope option: system, global, `include`, `includeIf`, worktree and
/// command line all count. A canary shows that git prints the origin of a value before an empty answer is believed.
pub(crate) fn git_probe(ctx: &mut Ctx) {
    let Some(clone) = ctx.folder(Kind::Workspace) else { return };
    match git(
        ctx.env,
        &clone,
        &[
            "-c",
            "core.hooksPath=canary",
            "config",
            "--show-origin",
            "--get-all",
            "core.hooksPath",
        ],
    ) {
        Ok((Some(0), out, _)) if out.contains("canary") => {}
        other => {
            ctx.add(
                Probe::Git,
                format!("check broken: the probe did not show a planted core.hooksPath ({other:?}), so an empty answer proves nothing."),
                "Check that git is installed and that `git config --show-origin` works in the clone.",
            );
            return;
        }
    }
    let probes: [(&str, &[&str]); 2] = [
        ("core.hooksPath", &["config", "--show-origin", "--get-all", "core.hooksPath"]),
        (
            "url.*.insteadOf",
            &["config", "--show-origin", "--get-regexp", r"^url\..*\.(push)?insteadof$"],
        ),
    ];
    for (what, args) in probes {
        match git(ctx.env, &clone, args) {
            Ok((Some(1 | 0), out, _)) if out.trim().is_empty() => {}
            Ok((Some(0), out, _)) => {
                for line in out.lines().filter(|l| !l.trim().is_empty()) {
                    ctx.add(
                        Probe::Git,
                        format!("git configuration sets {what}: {line}"),
                        "A runner-level git setting changes where code comes from or which hooks run: remove it from the file named (system, global or an include) before the job.",
                    );
                }
            }
            other => ctx.add(
                Probe::Git,
                format!("check broken: `git config` for {what} gave {other:?}."),
                "Check git in the clone.",
            ),
        }
    }
}

/// No npm configuration outside the tagged tree.
pub(crate) fn npm(ctx: &mut Ctx) {
    let Some(clone) = ctx.folder(Kind::Workspace) else { return };
    let files = tracked(ctx.env, &clone).unwrap_or_default();
    let mut candidates: Vec<(String, PathBuf)> = Vec::new();
    if let Some(home) = ctx.env.home() {
        candidates.push(("the user's ~/.npmrc".into(), home.join(".npmrc")));
    }
    for var in [
        "NPM_CONFIG_USERCONFIG",
        "npm_config_userconfig",
        "NPM_CONFIG_GLOBALCONFIG",
        "npm_config_globalconfig",
    ] {
        if let Some(v) = ctx.env.get(var) {
            candidates.push((format!("the file {var} names"), PathBuf::from(v)));
        }
    }
    // npm itself says where its user and global files are (it knows its own prefix; Homebrew's node is a link into the
    // Cellar and the prefix is that of the link). Missing or failing npm: the probe cannot look.
    for key in ["userconfig", "globalconfig"] {
        match npm_get(ctx.env, &clone, key) {
            Ok(Some(p)) => candidates.push((format!("the file `npm config get {key}` names"), PathBuf::from(p))),
            Ok(None) => {}
            Err(e) => ctx.add(
                Probe::Npm,
                format!("check broken: `npm config get {key}` {e}, so the npm configuration was not measured."),
                "Put the job's npm on PATH before `verify`.",
            ),
        }
    }
    candidates.extend(
        npm_prefix_candidates(ctx.env)
            .into_iter()
            .map(|p| ("the global npmrc under node's prefix".to_string(), p)),
    );
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    for (what, path) in candidates {
        if path.is_file() {
            let canonical = winpath::real(&path).unwrap_or_else(|_| path.clone());
            if !seen.insert(canonical.clone()) {
                continue;
            }
            if !inside_tagged_tree(&canonical, &clone, &files) {
                ctx.add(
                    Probe::Npm,
                    format!(
                        "{what} exists: {} (npm reads it for registry, ignore-scripts and script-shell).",
                        canonical.display()
                    ),
                    "Remove it from the runner, or give the job its own HOME.",
                );
            }
        }
    }
}

/// `name` on the job's own `PATH` (Windows: `Path`, with `.exe` or `.cmd` in `PATHEXT` order).
fn which(env: &Env, name: &str) -> Option<PathBuf> {
    let path = env.get("PATH")?;
    super::pathfind::find_program(path, name, cfg!(windows), &super::pathfind::extensions(env.get("PATHEXT")))
}

/// `npm config get <key>` in the clone with the job's environment, at most 10 s. `Ok(None)` for `undefined` or nothing.
fn npm_get(env: &Env, clone: &Path, key: &str) -> Result<Option<String>, String> {
    // The npm of the job's own PATH, found here: a bare name is looked up by the system as well, and on Windows through the parent's
    // PATH, so a runner with npm on its PATH would answer for a job whose PATH has none (the check then passed instead of saying
    // it was broken; seen on the Windows runner, run 7904).
    let program = which(env, "npm").ok_or_else(|| "cannot be run (npm is not on the job's PATH)".to_string())?;
    let mut cmd = command(program);
    cmd.args(["config", "get", key]).current_dir(clone).env_clear();
    for (k, v) in &env.0 {
        cmd.env(k, v);
    }
    let out = run_bounded(cmd, Limits::new(Duration::from_secs(10))).map_err(|e| format!("cannot be run ({e})"))?;
    match out.outcome {
        Outcome::Exited(s) if s.success() => {
            let text = out.stdout_text();
            let t = text.trim();
            Ok((!t.is_empty() && t != "undefined" && t != "null").then(|| t.to_string()))
        }
        Outcome::Exited(s) => Err(format!("exited with {:?}: {}", s.code(), out.stderr_text().trim())),
        _ => Err("did not finish within 10 s".to_string()),
    }
}

/// Where the global npmrc may be without asking npm, as a second reading: `<prefix>/etc/npmrc` for `npm_config_prefix` and for
/// the node on PATH, by **both** its PATH spelling and its canonical one (Homebrew: `/opt/homebrew/bin/node` is a link into the
/// Cellar, and npm's prefix is `/opt/homebrew`); on Windows the node folder's `etc\npmrc` and `%APPDATA%\npm\etc\npmrc`.
fn npm_prefix_candidates(env: &Env) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(prefix) = env.get("npm_config_prefix").or_else(|| env.get("NPM_CONFIG_PREFIX")) {
        out.push(Path::new(prefix).join("etc").join("npmrc"));
    }
    let raw = which(env, "node");
    if let Some(raw) = raw {
        for node_path in [Some(raw.clone()), winpath::real(&raw).ok()].into_iter().flatten() {
            if cfg!(windows) {
                if let Some(dir) = node_path.parent() {
                    out.push(dir.join("etc").join("npmrc"));
                }
            } else if let Some(prefix) = node_path.parent().and_then(Path::parent) {
                out.push(prefix.join("etc").join("npmrc"));
            }
        }
    }
    if cfg!(windows)
        && let Some(appdata) = env.get("APPDATA")
    {
        out.push(Path::new(appdata).join("npm").join("etc").join("npmrc"));
    }
    out
}

/// The Angular cache of each app is absent or a real folder inside the checkout.
pub(crate) fn angular(ctx: &mut Ctx) {
    let Some(clone) = ctx.folder(Kind::Workspace) else { return };
    for app in &ctx.opts.app_dirs {
        let cache = clone.join(app).join(".angular").join("cache");
        match std::fs::symlink_metadata(&cache) {
            Err(_) => {}
            Ok(m) if m.file_type().is_symlink() => ctx.add(
                Probe::Angular,
                format!("{} is a link: the build would read a kept Angular cache.", cache.display()),
                "Do not link the Angular cache in release mode; let the build write it inside the checkout.",
            ),
            Ok(_) => {
                let inside = winpath::real(&cache).map(|p| p.starts_with(&clone)).unwrap_or(false);
                if !inside {
                    ctx.add(
                        Probe::Angular,
                        format!("{} leads outside the checkout (a folder above it is a link).", cache.display()),
                        "Remove the link; the cache must live inside the job's own checkout.",
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process_env_in(home: &Path) -> Env {
        let mut env = Env::from_process();
        env.set("HOME", home.display().to_string());
        env.set("USERPROFILE", home.display().to_string());
        env.set("GIT_CONFIG_NOSYSTEM", "1".into());
        env
    }

    #[test]
    fn a_git_failure_in_a_finding_carries_its_cause_from_standard_error() {
        let d = tempfile::tempdir().unwrap();
        let err = tracked(&process_env_in(d.path()), d.path()).unwrap_err();
        assert!(err.contains("fatal:"), "{err}");
    }

    #[test]
    fn git_standard_error_is_a_separate_field_so_a_warning_never_turns_not_found_into_broken() {
        let d = tempfile::tempdir().unwrap();
        let mut env = process_env_in(d.path());
        env.set("GIT_TRACE", "1".into()); // git then writes trace lines to standard error
        let (code, out, err) = git(&env, d.path(), &["config", "--get-all", "core.hooksPath"]).unwrap();
        assert_eq!(code, Some(1), "an absent key is exit 1");
        assert!(out.trim().is_empty(), "standard output stays empty: {out:?}");
        assert!(!err.is_empty(), "the trace is in the standard error field");
    }

    #[test]
    fn the_standard_error_quoted_is_its_last_two_kib_at_a_character_boundary() {
        let long = format!("{}{}", "é".repeat(5000), "END");
        let t = tail_text(&long, STDERR_TAIL);
        assert!(t.len() <= STDERR_TAIL && t.ends_with("END"), "{}", t.len());
        assert_eq!(tail_text("  short \n", STDERR_TAIL), "short");
    }
}
