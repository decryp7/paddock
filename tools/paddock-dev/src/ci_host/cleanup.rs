//! `dev ci host-cleanup`: leaves nothing of this job behind on a host runner: its stray test processes, its temp folders and
//! (optionally) its build outputs, never another job's (job isolation, `.gitea/actions/README.md`), so any number of jobs can run on
//! the host at once. The port of `tools/host-cleanup.sh`.
//!
//! ```text
//! dev ci host-cleanup --id pitwall [--process-pattern '{build}[/\\]debug[/\\]pitwall-server' ...]
//!                     [--temp-prefix pitwall-it-] [--build-outputs]
//! ```
//!
//! - `--process-pattern`: a regular expression for command lines, anchored on a folder of this job: `{tmp}` (the job's own temp
//!   folder), `{build}` (its build folder) or `{slot}` (its job slot, in a kept folder's name), filled in by
//!   `ci isolation pattern`. A pattern that names none of them, or whose folder this job does not have, could match another
//!   job's processes: it is skipped with a warning. There are no exact process names (they match every job's).
//! - Temp folders: `<job temp>/<prefix>*` (default prefix: the id and a dash), only in the job's own temp folder
//!   (`PADDOCK_JOB_TEMP`, set by the job-isolation action); without one the sweep is skipped.
//! - `--build-outputs` removes `$CARGO_TARGET_DIR/{dist,debug}` (only when it is set) and `dist`, `app/dist`, `app/coverage`,
//!   `app/playwright-report` in the current folder.
//! - Always exits 0 once the arguments are valid (a clean-up must not fail a job); 2 for a bad argument.
//!
//! Ending processes: the table is read fresh through `paddock_proc::processes_with_command_lines` (one reading that gives each
//! process its identity, pid plus start time, on every OS), the command lines are matched here, and each match is ended through
//! `paddock_proc::terminate` with that identity (rule 53): a match listed first and ended seconds later (each one may take up to
//! [`GRACE`]) whose pid went to another process meanwhile is left alone, because the identity no longer matches. Asked to stop,
//! then ended after a grace of [`GRACE`]. The shell sent `pkill -f`'s SIGTERM only, so a process that ignored it stayed; now it goes. Afterwards the leak check
//! lists any process that still matches one of the job's patterns (a `::warning::` with its pid and command line). Drivers that
//! live outside the job's folders (`tauri-driver` in `~/.cargo/bin`, `safaridriver`, `WebKitWebDriver`) cannot be scoped by a path
//! and are never ended by name (another job may own one): the suite that starts one stops it, and a pattern can still find it
//! through an argument that names the job's folder. `PADDOCK_PS` replaces the table by a program's output (tests: those processes are never ended).

use super::Step;
use super::safe_remove;
use crate::ci_tool::ToolCall;
use clap::Args as ClapArgs;
use paddock_proc::{ProcessId, StartTime};
use regex::Regex;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a process asked to stop has before it is ended.
pub const GRACE: Duration = Duration::from_secs(5);
const LIST_TIMEOUT: Duration = Duration::from_secs(60);

/// `dev ci host-cleanup`.
#[derive(ClapArgs, Debug)]
pub struct CleanupArgs {
    /// The app id (lower-case letters, digits, dashes); temp folders named `<id>-*` are removed.
    #[arg(long)]
    pub id: Option<String>,
    /// A regular expression anchored on a folder of this job (`{tmp}`, `{build}`, `{slot}`); repeat for several.
    #[arg(long = "process-pattern", allow_hyphen_values = true)]
    pub process_pattern: Vec<String>,
    /// Start of the temp folders to remove (`<job temp>/<prefix>*`); empty: `<id>-`.
    #[arg(long)]
    pub temp_prefix: Option<String>,
    /// Also remove dist, the target folder's dist and debug, and the app's dist and coverage.
    #[arg(long)]
    pub build_outputs: bool,
}

/// The short name the CLI uses.
pub type Args = CleanupArgs;

/// One process of the table: its identity (pid and start time, read while the table was listed), its command line, and the line
/// as listed. There is no bare pid anywhere below this point: ending a process takes only its [`ProcessId`] (rule 53, ADR 0033).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    /// The process, by identity.
    pub id: ProcessId,
    /// The command line.
    pub cmd: String,
    /// `<pid> <command line>` as listed (the `PADDOCK_PS` test hook keeps the table's padding).
    pub line: String,
}

/// The process table and the way to end a process.
pub trait Procs {
    /// Every process with a command line, from a fresh reading, each with the identity taken in that reading.
    fn list(&self) -> Vec<Proc>;
    /// Ends the process `id` names: asked to stop, then ended after [`GRACE`], but only while its pid still belongs to it. A pid
    /// handed on to another process since the listing is left alone. Whether this call ended it.
    fn end(&self, id: ProcessId) -> bool;
}

/// The machine this process runs on.
pub struct OsProcs<'a> {
    env: &'a HashMap<String, String>,
}

impl Procs for OsProcs<'_> {
    fn list(&self) -> Vec<Proc> {
        // A test hook: a program that prints `ps -axo pid=,command=` lines instead of the real table. Its processes get a start time
        // no real process has, so ending one is always refused by the identity check and the real table is never touched (ER-14).
        if let Some(ps) = self.env.get("PADDOCK_PS").filter(|v| !v.is_empty()) {
            let call = ToolCall::new(ps).args(["-axo", "pid=,command="]).timeout(LIST_TIMEOUT);
            let Ok(ran) = call.run(self.env, &mut std::io::sink()) else {
                return Vec::new();
            };
            if !ran.success {
                return Vec::new();
            }
            return parse_table(&ran.stdout)
                .into_iter()
                .map(|(pid, cmd, line)| Proc {
                    id: ProcessId::new(pid, StartTime::from_micros(0)),
                    cmd,
                    line,
                })
                .collect();
        }
        paddock_proc::processes_with_command_lines()
            .into_iter()
            .map(|(id, cmd)| Proc {
                id,
                line: format!("{} {cmd}", id.pid()),
                cmd,
            })
            .collect()
    }

    fn end(&self, id: ProcessId) -> bool {
        matches!(
            paddock_proc::terminate(id, GRACE),
            paddock_proc::Terminated::Gracefully | paddock_proc::Terminated::Killed
        )
    }
}

/// The lines of a process listing as (pid, command line, line) (blank and malformed lines are dropped, carriage returns removed).
pub fn parse_table(text: &str) -> Vec<(u32, String, String)> {
    text.lines()
        .filter_map(|raw| {
            let line = raw.trim_end_matches('\r');
            let t = line.trim_start();
            let (pid, cmd) = t.split_once(char::is_whitespace)?;
            Some((pid.parse().ok()?, cmd.trim_start().to_string(), line.to_string()))
        })
        .collect()
}

/// The processes whose command line matches `re`, never this process.
fn matching(table: Vec<Proc>, re: &Regex, own: u32) -> Vec<Proc> {
    table.into_iter().filter(|p| p.id.pid() != own && re.is_match(&p.cmd)).collect()
}

/// The command over the real machine.
pub fn run(env: &HashMap<String, String>, windows: bool, args: &Args, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();
    run_with(env, windows, args, &OsProcs { env }, &cwd, std::process::id(), out, err)
}

/// The command over an explicit process table and folder (tests).
#[allow(clippy::too_many_arguments)]
pub fn run_with(
    env: &HashMap<String, String>,
    windows: bool,
    args: &Args,
    procs: &dyn Procs,
    cwd: &Path,
    own_pid: u32,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let id = args.id.clone().unwrap_or_default();
    let valid = id.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid {
        let _ = writeln!(err, "host-cleanup: --id <app id> is needed (lower-case letters, digits, dashes)");
        return 2;
    }
    let prefix = args
        .temp_prefix
        .clone()
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| format!("{id}-"));
    if prefix.contains('/') || prefix.contains('\\') || prefix == "." || prefix == ".." {
        let _ = writeln!(err, "host-cleanup: the temp prefix must be a plain name start");
        return 2;
    }
    let mut step = Step {
        env,
        out,
        windows,
        name: "host-cleanup",
    };
    let home = step.var("HOME").or_else(|| step.var("USERPROFILE")).map(PathBuf::from);

    let mut scoped_patterns: Vec<Regex> = Vec::new();
    for p in args.process_pattern.iter().filter(|p| !p.is_empty()) {
        match step.isolation(&["pattern", p]) {
            Ok(scoped) => match Regex::new(&scoped) {
                Ok(re) => {
                    let found = matching(procs.list(), &re, own_pid);
                    let ended = found.iter().filter(|f| procs.end(f.id)).count();
                    step.say(format!(
                        "host-cleanup: pattern {scoped}: {} process(es) found, {ended} ended",
                        found.len()
                    ));
                    scoped_patterns.push(re);
                }
                Err(e) => step.warn(format!(
                    "skipped the process pattern '{p}': it is not a valid regular expression ({e})"
                )),
            },
            Err(_) => step.warn(format!(
                "skipped the process pattern '{p}': it is not anchored on a folder of this job ({{tmp}}, {{build}} or {{slot}})"
            )),
        }
    }
    // The leak check: what this job started and the clean-up could not end.
    for re in &scoped_patterns {
        let left = matching(procs.list(), re, own_pid);
        if !left.is_empty() {
            step.say(format!(
                "::warning::host-cleanup: processes of this job are still running after the clean-up (pattern {}):",
                re.as_str()
            ));
            for p in left {
                step.say(format!("  {}", p.line.chars().take(300).collect::<String>()));
            }
        }
    }

    match step.var("PADDOCK_JOB_TEMP").map(PathBuf::from).filter(|t| t.is_dir()) {
        Some(tmp) => sweep_temp(&mut step, &tmp, &prefix, home.as_deref()),
        None => step.say("host-cleanup: no temp folder of this job (PADDOCK_JOB_TEMP); temp folders are left to the runner"),
    }
    if args.build_outputs {
        if let Some(target) = step.var("CARGO_TARGET_DIR").map(PathBuf::from) {
            remove_all(&mut step, &target, &["dist", "debug"], home.as_deref());
        }
        remove_all(
            &mut step,
            cwd,
            &["dist", "app/dist", "app/coverage", "app/playwright-report"],
            home.as_deref(),
        );
    }
    0
}

/// Removes `<tmp>/<prefix>*` (files and folders, never a link's target).
fn sweep_temp(step: &mut Step<'_>, tmp: &Path, prefix: &str, home: Option<&Path>) {
    let mut names: Vec<String> = std::fs::read_dir(tmp)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with(prefix))
        .collect();
    names.sort();
    for n in names {
        let p = tmp.join(&n);
        // A link is left as it is: the shell's `[ ! -L ]`.
        if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
            continue;
        }
        if let Err(e) = safe_remove::remove_under(tmp, &p, home) {
            step.warn(format!("could not remove {}: {e}", p.display()));
        }
    }
}

fn remove_all(step: &mut Step<'_>, root: &Path, rel: &[&str], home: Option<&Path>) {
    for r in rel {
        let p = root.join(r);
        if let Err(e) = safe_remove::remove_under(root, &p, home) {
            step.warn(format!("could not remove {}: {e}", p.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A scripted process table. Each process has the start time it was listed with; `handed_on` are pids that another process
    /// takes over right after the table was listed (the pid is the same, the start time is not). Ending a process whose identity no
    /// longer matches the table is refused, as `paddock_proc::terminate` does. A `stubborn` process stays after being ended.
    struct Fake {
        table: RefCell<Vec<(ProcessId, String, String)>>,
        ended: RefCell<Vec<u32>>,
        stubborn: Vec<u32>,
        handed_on: Vec<u32>,
    }

    impl Fake {
        fn new(lines: &[&str]) -> Fake {
            let table = parse_table(&lines.join("\n"))
                .into_iter()
                .map(|(pid, cmd, line)| (ProcessId::new(pid, StartTime::from_micros(100)), cmd, line))
                .collect();
            Fake {
                table: RefCell::new(table),
                ended: RefCell::default(),
                stubborn: Vec::new(),
                handed_on: Vec::new(),
            }
        }
    }

    impl Procs for Fake {
        fn list(&self) -> Vec<Proc> {
            let seen = self
                .table
                .borrow()
                .iter()
                .map(|(id, cmd, line)| Proc {
                    id: *id,
                    cmd: cmd.clone(),
                    line: line.clone(),
                })
                .collect();
            // The listing is taken; now the pids change hands.
            for (id, _, _) in self.table.borrow_mut().iter_mut() {
                if self.handed_on.contains(&id.pid()) {
                    *id = ProcessId::new(id.pid(), StartTime::from_micros(200));
                }
            }
            seen
        }
        fn end(&self, id: ProcessId) -> bool {
            let now = self.table.borrow().iter().find(|(p, _, _)| p.pid() == id.pid()).map(|(p, _, _)| *p);
            if now != Some(id) {
                return false;
            }
            self.ended.borrow_mut().push(id.pid());
            if !self.stubborn.contains(&id.pid()) {
                self.table.borrow_mut().retain(|(p, _, _)| p.pid() != id.pid());
            }
            true
        }
    }

    struct Scene {
        d: tempfile::TempDir,
        tmp: PathBuf,
        cwd: PathBuf,
        env: HashMap<String, String>,
    }

    fn scene() -> Scene {
        let d = crate::ci_host::canonical_tempdir();
        let root = std::fs::canonicalize(d.path()).unwrap();
        let tmp = root.join("jobtemp");
        let cwd = root.join("work");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        let env = [
            ("RUNNER_OS", "macOS".to_string()),
            ("PADDOCK_CI_SLOT", "s2".into()),
            ("PADDOCK_JOB_TEMP", tmp.display().to_string()),
            ("PADDOCK_BUILD_FOLDER", "/ci/target-pitwall-app-s2".into()),
            ("HOME", root.join("home").display().to_string()),
            ("RUNNER_TEMP", tmp.display().to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        Scene { d, tmp, cwd, env }
    }

    fn args(id: &str, patterns: &[&str]) -> Args {
        Args {
            id: Some(id.into()),
            process_pattern: patterns.iter().map(|s| s.to_string()).collect(),
            temp_prefix: None,
            build_outputs: false,
        }
    }

    fn go(sc: &Scene, a: &Args, procs: &dyn Procs) -> (i32, String, String) {
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let code = run_with(&sc.env, false, a, procs, &sc.cwd, 1, &mut o, &mut e);
        (code, String::from_utf8(o).unwrap(), String::from_utf8(e).unwrap())
    }

    #[test]
    fn only_processes_anchored_on_this_job_are_ended_and_the_unanchored_patterns_are_skipped_with_a_warning() {
        let sc = scene();
        let fake = Fake::new(&[
            "  41 /ci/target-pitwall-app-s2/debug/pitwall-server --x",
            "  42 /ci/target-pitwall-app-s3/debug/pitwall-server --x",
            "  43 /Users/u/test-browser-pitwall-s2/chrome",
            "  44 /usr/bin/other",
        ]);
        let a = args(
            "pitwall",
            &[
                "{build}[/\\\\]debug[/\\\\]pitwall-server",
                "fake[-_]engine",
                "[/\\\\]test-browser-pitwall-{slot}",
                "chrome-{slot}",
            ],
        );
        let (code, out, _) = go(&sc, &a, &fake);
        assert_eq!(code, 0, "{out}");
        assert_eq!(*fake.ended.borrow(), vec![41, 43], "each pattern scoped to this job's slot");
        assert!(
            out.contains("skipped the process pattern 'chrome-{slot}'"),
            "{{slot}} must follow a kept folder's name: {out}"
        );
        assert!(
            out.contains("skipped the process pattern 'fake[-_]engine': it is not anchored"),
            "an unscoped pattern could kill another job's processes: {out}"
        );
        assert!(!out.contains("still running"), "{out}");
    }

    #[test]
    fn the_leak_check_names_what_still_runs_and_only_this_jobs_processes() {
        let sc = scene();
        let mut fake = Fake::new(&[
            "  41 /ci/target-pitwall-app-s2/debug/pitwall-server --fake-engine",
            "  42 /ci/target-pitwall-app-s3/debug/pitwall-server --fake-engine",
            "  43 /usr/bin/other",
        ]);
        fake.stubborn = vec![41];
        let (code, out, _) = go(&sc, &args("pitwall", &["{build}[/\\\\]debug[/\\\\]pitwall-server"]), &fake);
        assert_eq!(code, 0);
        assert!(
            out.contains("::warning::host-cleanup: processes of this job are still running after the clean-up"),
            "{out}"
        );
        assert!(
            out.lines()
                .any(|l| l == "    41 /ci/target-pitwall-app-s2/debug/pitwall-server --fake-engine"),
            "the table's own padding after two spaces: {out}"
        );
        assert!(!out.contains(" 42 ") && !out.contains(" 43 "), "{out}");
    }

    #[test]
    fn this_process_is_never_ended_even_when_its_command_line_matches() {
        let sc = scene();
        let fake = Fake::new(&["  1 /ci/target-pitwall-app-s2/debug/pitwall-server"]);
        let (_, _, _) = go(&sc, &args("pitwall", &["{build}[/\\\\]debug"]), &fake);
        assert!(fake.ended.borrow().is_empty());
    }

    #[test]
    fn temp_folders_with_the_prefix_go_from_the_jobs_own_temp_folder_only_and_build_outputs_stay_unless_asked() {
        let sc = scene();
        for d in ["pitwall-abc", "pitwall-def", "other-job"] {
            std::fs::create_dir_all(sc.tmp.join(d)).unwrap();
        }
        std::fs::write(sc.tmp.join("pitwall-file"), "x").unwrap();
        for f in ["dist/x", "app/dist/y"] {
            std::fs::create_dir_all(sc.cwd.join(f).parent().unwrap()).unwrap();
            std::fs::write(sc.cwd.join(f), "1").unwrap();
        }
        let fake = Fake::new(&[]);
        let (code, _, _) = go(&sc, &args("pitwall", &[]), &fake);
        assert_eq!(code, 0);
        assert!(!sc.tmp.join("pitwall-abc").exists() && !sc.tmp.join("pitwall-def").exists() && !sc.tmp.join("pitwall-file").exists());
        assert!(sc.tmp.join("other-job").exists(), "other folders stay");
        assert!(sc.cwd.join("dist/x").exists(), "build outputs stay without --build-outputs");
        let target = sc.d.path().join("target");
        for p in ["dist", "debug", "release"] {
            std::fs::create_dir_all(target.join(p)).unwrap();
            std::fs::write(target.join(p).join("blob"), "x").unwrap();
        }
        let mut sc2 = scene();
        sc2.env.insert("CARGO_TARGET_DIR".into(), target.display().to_string());
        std::fs::create_dir_all(sc2.cwd.join("app/coverage")).unwrap();
        std::fs::create_dir_all(sc2.cwd.join("dist")).unwrap();
        let mut a = args("pitwall", &[]);
        a.build_outputs = true;
        go(&sc2, &a, &fake);
        assert!(!sc2.cwd.join("dist").exists() && !sc2.cwd.join("app/coverage").exists());
        assert!(!target.join("dist").exists() && !target.join("debug").exists());
        assert!(target.join("release/blob").exists(), "only dist and debug of the target folder");
    }

    #[test]
    fn without_a_temp_folder_of_its_own_the_sweep_leaves_the_shared_temp_alone() {
        let mut sc = scene();
        sc.env.remove("PADDOCK_JOB_TEMP");
        std::fs::create_dir_all(sc.tmp.join("pitwall-it-other-job")).unwrap();
        let mut a = args("pitwall", &[]);
        a.temp_prefix = Some("pitwall-it-".into());
        let (code, out, _) = go(&sc, &a, &Fake::new(&[]));
        assert_eq!(code, 0);
        assert!(sc.tmp.join("pitwall-it-other-job").exists(), "a concurrent job's test folder stays");
        assert!(out.contains("no temp folder of this job"), "{out}");
    }

    #[test]
    fn a_bad_id_or_prefix_is_refused_before_anything_is_removed_and_a_custom_prefix_is_honoured() {
        let sc = scene();
        std::fs::create_dir_all(sc.tmp.join("keep-me")).unwrap();
        std::fs::create_dir_all(sc.tmp.join("job-1")).unwrap();
        let fake = Fake::new(&[]);
        assert_eq!(
            go(
                &sc,
                &Args {
                    id: None,
                    ..args("x", &[])
                },
                &fake
            )
            .0,
            2
        );
        assert_eq!(go(&sc, &args("Bad Id", &[]), &fake).0, 2);
        let mut a = args("x", &[]);
        a.temp_prefix = Some("../".into());
        assert_eq!(go(&sc, &a, &fake).0, 2);
        a.temp_prefix = Some("job-".into());
        assert_eq!(go(&sc, &a, &fake).0, 0);
        assert!(sc.tmp.join("keep-me").exists() && !sc.tmp.join("job-1").exists());
        assert!(fake.ended.borrow().is_empty());
    }

    #[test]
    fn a_listing_is_parsed_with_the_tables_padding_and_windows_line_ends() {
        let t = parse_table("  41 /a/b --x\r\n 7 c d\n\nnot a pid\n  9\n");
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].0, t[0].1.as_str()), (41, "/a/b --x"));
        assert_eq!(t[0].2, "  41 /a/b --x");
        assert_eq!(t[1].1, "c d");
    }

    #[test]
    fn a_pid_handed_to_another_process_between_the_listing_and_the_end_is_left_alone() {
        let sc = scene();
        let mut fake = Fake::new(&[
            "  41 /ci/target-pitwall-app-s2/debug/pitwall-server --x",
            "  43 /ci/target-pitwall-app-s2/debug/pitwall-server --y",
        ]);
        fake.handed_on = vec![43];
        let (code, out, _) = go(&sc, &args("pitwall", &["{build}[/\\\\]debug[/\\\\]pitwall-server"]), &fake);
        assert_eq!(code, 0, "{out}");
        assert_eq!(
            *fake.ended.borrow(),
            vec![41],
            "the process that took pid 43 over is not this job's"
        );
        assert!(out.contains("2 process(es) found, 1 ended"), "{out}");
    }

    #[cfg(unix)]
    #[test]
    fn the_real_end_refuses_an_identity_whose_pid_now_belongs_to_a_later_process_and_ends_the_right_one() {
        let sc = scene();
        let marker = sc.tmp.join("marker-stale");
        let mut child = paddock_proc::command("perl")
            .arg("-e")
            .arg("sleep 30")
            .arg(&marker)
            .spawn()
            .unwrap();
        let env = HashMap::new();
        let procs = OsProcs { env: &env };
        let id = ProcessId::of(child.id()).unwrap();
        // The same pid with another start time: what a pid handed on to a later process looks like to the identity check.
        let stale = ProcessId::new(id.pid(), StartTime::from_micros(id.started().as_micros() + 5_000_000));
        assert!(!procs.end(stale), "a stale identity ends nothing");
        assert!(child.try_wait().unwrap().is_none(), "the process that holds the pid runs on");
        assert!(procs.end(id));
        let status = child.wait().unwrap();
        assert!(!status.success(), "ended by a signal: {status:?}");
    }
}
