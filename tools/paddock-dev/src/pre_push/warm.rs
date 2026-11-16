//! `./dev check --warm-linux [workspace...]`: builds the Linux Clippy's dependencies in the CI image's kept volume, one container
//! run per workspace, with no push budget and the container's own output live. The first run of a workspace compiles everything
//! (see docs/benchmarks.md, "Pre-push Linux Clippy"); the push gate's row then only ever sees warm runs. It runs the push gate's own command ([`gates::linux_gate`]), so what it warms is what the row reuses; a lint it finds in
//! the workspace itself is reported (exit 5) but the dependencies are warm either way.

use super::budget::with_heavy_flags;
use super::exec::{Done, Row};
use super::exec::{docker_answers, heavy_command_for, on_path};
use super::gates::{self, Gate, LinuxTarget};
use super::linux_inputs::{ImageProbe, PROBE_NEXT, ci_version, image_probe, left_out};
use super::outcome::subject_of;
use super::{render, repo_root};
use crate::plan::{Request, plan_with_graph};
use paddock_cli::{CliError, Report};
use paddock_repo_graph::GitHistory;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The hang backstop of one workspace's run (ER-12, ER-32): about twice the largest cold run measured on the native image
/// (apps/dyno, 173.7 s; docs/benchmarks.md, "Pre-push Linux Clippy, native"), so a hung container ends, a loaded machine still fits.
const RUN_MAX: Duration = Duration::from_secs(360);
/// The wait for the machine-wide slot: one full `--warm-linux` of another agent holds it for the sum of the seven cold runs
/// measured (767.6 s), rounded up to 15 minutes.
const QUEUE_MAX: Duration = Duration::from_secs(15 * 60);
/// The hang backstop of the local CI image build (`linux-container.sh --ci-image`: the garage stage and ci-image/Dockerfile): about
/// twice the cold build measured with `--no-cache`, 379 s (docs/benchmarks.md, "Pre-push Linux Clippy, native"): 760 s.
const IMAGE_MAX: Duration = Duration::from_secs(760);

/// The workspaces to warm: the ones named (each must be a Cargo workspace folder), else every Cargo workspace.
pub fn targets(root: &Path, named: &[String], all_workspaces: &[String]) -> Result<Vec<String>, CliError> {
    if named.is_empty() {
        return Ok(all_workspaces.to_vec());
    }
    let mut out = Vec::new();
    for w in named {
        let w = w.trim_end_matches('/');
        if !root.join(w).join("Cargo.toml").is_file() {
            return Err(
                CliError::failure(format!("`{w}` is not a Cargo workspace folder (no {w}/Cargo.toml)."))
                    .with_hint("Name a folder like `crates`, `tools/paddock-dev` or `apps/dyno`."),
            );
        }
        out.push(w.to_string());
    }
    Ok(out)
}

/// One container run per workspace, the push gate's own command; `left_out` reads a workspace's Tauri crates.
pub fn runs(targets: &[String], version: &str, left_out: &dyn Fn(&str) -> Result<Vec<String>, String>) -> Result<Vec<Gate>, String> {
    targets
        .iter()
        .map(|w| {
            let t = LinuxTarget {
                ws: w.clone(),
                left_out: left_out(w)?,
            };
            Ok(gates::linux_gate(&[t], version))
        })
        .collect()
}

/// A report of one row that did not run: the shared table, never a hand-made object.
fn one_row(id: &str, row: Row) -> Report {
    render("warm-linux", &[(id.to_string(), row)])
}

fn not_measured(why: &str, next: &str) -> Report {
    one_row("warm-linux", Row::not_measured(why, next))
}

/// Runs `argv` (a heavy prefix and a command) in `root` with the child's output live; its row says how it ended.
fn step(root: &Path, id: &str, argv: &[String], what: &str) -> (String, Row, bool) {
    eprintln!("warm-linux: {id}: start ({what}; the container's own output follows)");
    let started = Instant::now();
    let status = paddock_proc::command(&argv[0]).args(&argv[1..]).current_dir(root).status();
    let secs = started.elapsed().as_secs_f64();
    let subject = subject_of(root, None);
    let (done, word) = match status {
        Ok(s) => (
            Done {
                success: s.success(),
                how: format!("exit {}", s.code().map_or("?".to_string(), |c| c.to_string())),
                tail: String::new(),
                log: None,
                timing: None,
            },
            if s.success() { "done" } else { "failed" },
        ),
        Err(e) => (
            Done {
                success: false,
                how: format!("could not start: {e}"),
                tail: String::new(),
                log: None,
                timing: None,
            },
            "could not start",
        ),
    };
    eprintln!("warm-linux: {id}: {word} after {secs:.1} s ({})", done.how);
    let ok = done.success;
    let cause = format!("{id}: {} (clippy or the build failed; what compiled is kept)", done.how);
    let row = if ok {
        Row::from_run(id, &subject, &done, cause, secs)
    } else {
        Row::failed(
            id,
            &subject,
            cause,
            String::new(),
            "read the output above, fix it, and run `./dev check --warm-linux` again".into(),
            secs,
        )
    };
    (id.to_string(), row, ok)
}

/// Runs `./dev check --warm-linux`.
pub fn command(root: &Option<PathBuf>, named: &[String]) -> Result<Report, CliError> {
    let root = repo_root(root)?;
    let env: HashMap<String, String> = std::env::vars().collect();
    let version = ci_version(&root).map_err(|e| CliError::failure(format!("{e}.")).with_hint("Run it from a full checkout."))?;
    let all = if named.is_empty() {
        let history = GitHistory::new(&root, Duration::from_secs(60));
        let req = Request {
            files: Vec::new(),
            all: true,
            explain: false,
            root: None,
            base: None,
            head: "HEAD".into(),
        };
        let (_, plan) = plan_with_graph(&root, &history, &req)
            .map_err(|e| CliError::failure(format!("The planner could not list the workspaces: {e}.")))?;
        gates::workspaces(&plan)
    } else {
        Vec::new()
    };
    let todo = targets(&root, named, &all)?;
    if todo.is_empty() {
        return Ok(one_row("warm-linux", Row::skipped("no Cargo workspace to warm")));
    }
    if !docker_answers(&env) {
        return Ok(not_measured(
            "no container runtime answers (Docker or OrbStack is not running)",
            "start it yourself (an agent never starts or restarts it, ER-14) and run `./dev check --warm-linux` again",
        ));
    }
    let built = std::sync::OnceLock::new();
    let base_prefix = match heavy_command_for(&root, &env, &|p| on_path(&env, p), &built, Duration::from_secs(600)) {
        Ok(p) => p,
        Err(e) => {
            return Ok(not_measured(
                &e,
                "make `heavy` available (it is built from tools/heavy) and run it again",
            ));
        }
    };
    let prefix = with_heavy_flags(&base_prefix, false, QUEUE_MAX, RUN_MAX);
    let mut rows: Vec<(String, Row)> = Vec::new();
    // The local CI image is its own step with its own bound: it is minutes and gigabytes, and neither a workspace's run limit nor
    // the push budget may cut it off. Built once per change to ci-image/; the probe decides, and a broken probe is not "missing".
    let probe = paddock_proc::command("bash")
        .args([
            "tools/linux-container.sh",
            "--id",
            "paddock",
            "--ci-image",
            "--image-ready",
            "--root",
        ])
        .arg(&root)
        .current_dir(&root)
        .status();
    match image_probe(probe.as_ref().ok().and_then(|s| s.code())) {
        ImageProbe::Ready => {}
        ImageProbe::Broken(how) => {
            return Ok(not_measured(&format!("image probe broken ({how})"), PROBE_NEXT));
        }
        ImageProbe::Missing => {
            let mut argv = with_heavy_flags(&base_prefix, false, QUEUE_MAX, IMAGE_MAX);
            argv.extend(["bash", "tools/linux-container.sh", "--id", "paddock", "--ci-image", "--", "true"].map(String::from));
            let what = format!("local CI image, once per change to ci-image/; limit {} s", IMAGE_MAX.as_secs());
            let (id, row, ok) = step(&root, "local CI image", &argv, &what);
            rows.push((id, row));
            if !ok {
                return Ok(render("warm-linux", &rows));
            }
        }
    }
    let gates = runs(&todo, &version, &|ws| left_out(&root, ws)).map_err(|e| {
        CliError::failure(format!("{e}.")).with_hint("Run `cargo metadata --no-deps` in that workspace and fix what it prints.")
    })?;
    for (ws, g) in todo.iter().zip(gates) {
        let mut argv: Vec<String> = prefix.clone();
        argv.push(g.program.clone());
        argv.extend(g.args.iter().cloned());
        let (id, row, _) = step(&root, ws, &argv, &format!("no push budget; limit {} s", RUN_MAX.as_secs()));
        rows.push((id, row));
    }
    Ok(render("warm-linux", &rows))
}
