//! `dev ci host-probe`: how fast this host starts programs and writes small files, measured in a folder of the caller's choice, so a
//! slow test run can be told apart from a slow host (train 14, 2026-11-18: the same tests were 15 times slower on one tree while these
//! numbers stayed the same; docs/benchmarks.md). It is the probe a per-job host check can call, and the before/after measurement of a
//! host change (a Defender exclusion).
//!
//! ```text
//! dev ci host-probe [--dir <folder>] [--starts <n>] [--repos <n>] [--files <n>] [--label <text>]
//! ```
//!
//! Four phases, each timed per item: a program that does nothing (`cmd /c exit 0` on Windows, `true` elsewhere: the OS's own cost of
//! a process), `git --version` (a git start), a new repository with one commit (`init`, `add`, `commit`: four git starts and git's small
//! files) and a 4 KiB file written, read back and removed (the file system and any scanner on it). It writes only in a new folder
//! under `--dir` (default: `TMPDIR`, `TEMP` or `TMP`), which it removes; git runs with no global or system config. One summary line per phase,
//! one `host-probe-json:` line, and the same lines appended to `$GITHUB_STEP_SUMMARY` when it is set. A phase that cannot run says why
//! and the others still run; the step never fails the job.

use super::Step;
use clap::Args as ClapArgs;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// `dev ci host-probe`.
#[derive(ClapArgs, Debug, Clone)]
pub struct Args {
    /// The folder to measure in (a new folder is made inside it and removed); default: `TMPDIR`, `TEMP` or `TMP`.
    #[arg(long)]
    pub dir: Option<PathBuf>,
    /// How many empty programs and how many `git --version` to start.
    #[arg(long, default_value_t = 100)]
    pub starts: u32,
    /// How many repositories with one commit to make.
    #[arg(long, default_value_t = 30)]
    pub repos: u32,
    /// How many 4 KiB files to write, read and remove.
    #[arg(long, default_value_t = 300)]
    pub files: u32,
    /// A word for the lines (`before`, `after`).
    #[arg(long, default_value = "probe")]
    pub label: String,
}

/// The most of each count: a probe is a few seconds, never a load test.
const MAX_COUNT: u32 = 2000;

/// The timings of one phase, in milliseconds per item.
#[derive(Debug, Clone, PartialEq)]
pub struct Phase {
    /// The phase's name (`empty-start`, `git-start`, `repo`, `small-file`).
    pub name: &'static str,
    /// Items that ran.
    pub n: usize,
    /// Median ms per item.
    pub median: f64,
    /// 90th percentile ms per item.
    pub p90: f64,
    /// Why the phase did not run, or stopped early.
    pub problem: Option<String>,
}

/// The median and the 90th percentile of `ms` (nearest rank); `None` when it is empty.
pub fn summarise(ms: &[f64]) -> Option<(f64, f64)> {
    if ms.is_empty() {
        return None;
    }
    let mut v = ms.to_vec();
    v.sort_by(f64::total_cmp);
    let rank = |q: f64| v[((q * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1];
    Some((rank(0.5), rank(0.9)))
}

/// Runs `item` up to `n` times, timing each; stops at the first failure and says which.
fn phase(name: &'static str, n: u32, mut item: impl FnMut(u32) -> Result<(), String>) -> Phase {
    let mut ms = Vec::new();
    let mut problem = None;
    for i in 0..n.min(MAX_COUNT) {
        let t = Instant::now();
        if let Err(e) = item(i) {
            problem = Some(format!("item {} of {n}: {e}", i + 1));
            break;
        }
        ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let (median, p90) = summarise(&ms).unwrap_or((0.0, 0.0));
    Phase {
        name,
        n: ms.len(),
        median,
        p90,
        problem,
    }
}

/// Starts `cmd` and waits; an error unless it exits 0 within a minute.
fn run(mut cmd: std::process::Command) -> Result<(), String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start: {e}"))?;
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            return if st.success() { Ok(()) } else { Err(format!("exited with {st}")) };
        }
        if Instant::now() > end {
            let _ = child.kill();
            let _ = child.wait();
            return Err("did not end within 60 s".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A git child with no user or system config and a fixed identity.
fn git(dir: &Path, args: &[&str]) -> std::process::Command {
    let mut cmd = crate::git_env::git();
    cmd.arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=probe",
            "-c",
            "user.email=probe@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" });
    cmd
}

/// An empty program: the OS's own start cost.
fn empty_program() -> std::process::Command {
    if cfg!(windows) {
        let mut c = paddock_proc::command("cmd");
        c.args(["/d", "/c", "exit 0"]);
        c
    } else {
        paddock_proc::command("true")
    }
}

/// The four phases in a new folder under `base`, which is removed afterwards.
pub fn measure(base: &Path, a: &Args) -> (PathBuf, Vec<Phase>) {
    let work = base.join(format!(
        "paddock-host-probe-{}-{}",
        std::process::id(),
        a.label.replace(['/', '\\', ' '], "-")
    ));
    let _ = std::fs::create_dir_all(&work);
    let phases = vec![
        phase("empty-start", a.starts, |_| run(empty_program())),
        phase("git-start", a.starts, |_| run(git(&work, &["--version"]))),
        phase("repo", a.repos, |i| {
            let r = work.join(format!("r{i}"));
            std::fs::create_dir_all(&r).map_err(|e| e.to_string())?;
            run(git(&r, &["init", "-q"]))?;
            std::fs::write(r.join("a.txt"), format!("{i}\n")).map_err(|e| e.to_string())?;
            run(git(&r, &["add", "a.txt"]))?;
            run(git(&r, &["commit", "-q", "-m", "probe"]))?;
            run(git(&r, &["rev-parse", "HEAD"]))
        }),
        phase("small-file", a.files, |i| {
            let f = work.join(format!("f{i}.bin"));
            let block = vec![b'x'; 4096];
            std::fs::write(&f, &block).map_err(|e| e.to_string())?;
            let back = std::fs::read(&f).map_err(|e| e.to_string())?;
            std::fs::remove_file(&f).map_err(|e| e.to_string())?;
            (back.len() == block.len())
                .then_some(())
                .ok_or_else(|| "read back a different size".to_string())
        }),
    ];
    let _ = std::fs::remove_dir_all(&work);
    (work, phases)
}

/// The summary lines and the one JSON line.
pub fn render(label: &str, dir: &Path, phases: &[Phase]) -> Vec<String> {
    let mut lines: Vec<String> = phases
        .iter()
        .map(|p| match &p.problem {
            None => format!(
                "host-probe {label}: {} median {:.1} ms, p90 {:.1} ms ({} items)",
                p.name, p.median, p.p90, p.n
            ),
            Some(why) => format!("::warning::host-probe {label}: {} stopped after {} items: {why}", p.name, p.n),
        })
        .collect();
    let json = serde_json::json!({
        "label": label,
        "dir": dir.display().to_string(),
        "phases": phases.iter().map(|p| serde_json::json!({
            "name": p.name, "n": p.n, "medianMs": (p.median * 10.0).round() / 10.0, "p90Ms": (p.p90 * 10.0).round() / 10.0,
            "problem": p.problem,
        })).collect::<Vec<_>>(),
    });
    lines.push(format!("host-probe-json: {json}"));
    lines
}

/// The command over the real machine. Always exits 0.
pub fn run_step(env: &HashMap<String, String>, a: &Args, out: &mut dyn Write) -> i32 {
    let mut step = Step {
        env,
        out,
        windows: cfg!(windows),
        name: "host-probe",
    };
    // The job's temp folder from the environment the step was given (never read live: rule of `process_env`).
    let Some(base) = a
        .dir
        .clone()
        .or_else(|| ["TMPDIR", "TEMP", "TMP"].iter().find_map(|k| step.var(k)).map(PathBuf::from))
    else {
        step.warn("no --dir and no TMPDIR, TEMP or TMP; nothing measured. Next step: pass --dir <folder>");
        return 0;
    };
    if !base.is_dir() {
        step.warn(format!(
            "{} is not a folder; nothing measured. Next step: pass an existing --dir",
            base.display()
        ));
        return 0;
    }
    let (work, phases) = measure(&base, a);
    let lines = render(&a.label, &work, &phases);
    for l in &lines {
        step.say(l);
    }
    if let Some(summary) = step.var("GITHUB_STEP_SUMMARY").map(str::to_string) {
        let text: String = lines.iter().map(|l| format!("    {l}\n")).collect();
        if let Err(e) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&summary)
            .and_then(|mut f| f.write_all(text.as_bytes()))
        {
            step.warn(format!("the job summary could not be written ({e})"));
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_and_p90_are_nearest_rank_and_nothing_is_none() {
        assert_eq!(summarise(&[]), None);
        assert_eq!(summarise(&[5.0]), Some((5.0, 5.0)));
        let v: Vec<f64> = (1..=10).map(f64::from).rev().collect();
        assert_eq!(summarise(&v), Some((5.0, 9.0)));
    }

    /// A real, tiny probe: every phase runs its items in a scratch folder, which is gone afterwards; the lines name each phase.
    #[test]
    fn a_small_probe_measures_every_phase_and_leaves_nothing_behind() {
        let d = tempfile::tempdir().unwrap();
        let a = Args {
            dir: None,
            starts: 2,
            repos: 1,
            files: 3,
            label: "t".into(),
        };
        let (work, phases) = measure(d.path(), &a);
        assert!(!work.exists(), "the probe's folder is removed");
        let names: Vec<_> = phases.iter().map(|p| p.name).collect();
        assert_eq!(names, ["empty-start", "git-start", "repo", "small-file"]);
        for p in &phases {
            assert!(p.problem.is_none(), "{p:?}");
            assert!(p.n > 0 && p.median > 0.0 && p.p90 >= p.median, "{p:?}");
        }
        let lines = render("t", &work, &phases);
        assert_eq!(lines.len(), 5);
        assert!(lines[1].starts_with("host-probe t: git-start median"), "{lines:?}");
        assert!(lines[4].starts_with("host-probe-json: {"), "{lines:?}");
    }

    /// Positive control: a phase whose item fails stops there and says so, as a warning, never a number.
    #[test]
    fn a_failing_item_stops_its_phase_and_is_reported() {
        let p = phase(
            "git-start",
            5,
            |i| if i == 2 { Err("could not start: no git".into()) } else { Ok(()) },
        );
        assert_eq!(p.n, 2);
        assert_eq!(p.problem.as_deref(), Some("item 3 of 5: could not start: no git"));
        let lines = render("x", Path::new("/w"), &[p]);
        assert!(
            lines[0].starts_with("::warning::host-probe x: git-start stopped after 2 items"),
            "{lines:?}"
        );
    }
}
