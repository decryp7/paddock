//! `dev ci stable-workspace`: the checkout of a host job moves to a fixed folder of the job's slot in the CI home and a link
//! stays at the old path, so the build runs from the same physical path every time (the port of the `stable-workspace` action).
//! sccache keys a Rust compile on its working folder; the runner's per-job hashed work folder made every path crate miss.
//!
//! Behaviour, from the action's shell:
//!
//! 1. A Linux container has a fixed workspace already: nothing to do. A work folder that is already a link: nothing to do.
//! 2. The kept folder is `ci isolation path src <name>` (the job's slot is appended); without one the job builds in place.
//! 3. The move is a rename, so both folders must be on one volume; across volumes it would copy the whole checkout (7 to 293 s
//!    on a loaded Mac, runs 7706 to 7761): the job builds in place and the warning says how to fix it. A volume that cannot be
//!    read is unknown, never "the same".
//! 4. Where the folder cannot move (Windows under act keeps a handle on it) a clone of the checkout at the same commit is kept
//!    at the stable path (a local clone and fetch: hard links, no network), and the job builds there.
//! 5. After a move a link (a junction on Windows, [`links`](super::links)) is left at the old path; if that fails the folder
//!    moves back.
//! 6. `PADDOCK_WORKSPACE` goes to `$GITHUB_ENV`: later steps use it as their working folder.
//!
//! Every phase that touches the disk or git is bounded (`PADDOCK_STABLE_WORKSPACE_TIMEOUT` seconds, default 300): a hang (a file
//! held open by a process left from an earlier job, antivirus, git waiting on a lock) ends in one `::error::` line that names the
//! phase, and each phase logs its duration (ER-27). Any other problem only costs cache hits and never fails the job.

use super::links::{Links, OsLinks};
use super::phase::{Phase, bounded};
use super::safe_remove::{self, RemoveError};
use super::{Step, folder_name, isolation_full, tail_flat};
use crate::ci_isolation::home;
use crate::ci_tool::ToolCall;
use clap::Args as ClapArgs;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// `dev ci stable-workspace`.
#[derive(ClapArgs, Debug)]
pub struct StableWorkspaceArgs {
    /// Folder name under the CI home's `src/` (the job's slot is appended): one per repository and job.
    #[arg(long)]
    pub name: String,
}

/// The short name the CLI uses.
pub type Args = StableWorkspaceArgs;

/// The disk, git and link operations the step needs, so a test can stand in for the parts that cannot run here (a Windows
/// handle that blocks the rename, another volume, a hung phase).
pub trait Ops: Send + Sync {
    /// Whether `p` is a symlink (or a junction).
    fn is_symlink(&self, p: &Path) -> bool;
    /// `pwd -P`: the physical path of a folder.
    fn canonical(&self, p: &Path) -> io::Result<PathBuf>;
    /// An id of the volume `p` is on; `None` when it cannot be read.
    fn volume_of(&self, p: &Path) -> Option<String>;
    /// `mkdir -p`.
    fn create_dir_all(&self, p: &Path) -> io::Result<()>;
    /// `rm -rf target`, guarded by `root` ([`safe_remove`]).
    fn remove(&self, root: &Path, target: &Path) -> Result<bool, String>;
    /// `mv`: a rename on one volume.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    /// Whether `p` is a folder.
    fn is_dir(&self, p: &Path) -> bool;
    /// `git <args>`: its standard output (trimmed), or its error output.
    fn git(&self, args: &[String]) -> Result<String, String>;
    /// The link at `link` to `target`.
    fn link_dir(&self, target: &Path, link: &Path) -> Result<(), String>;
}

/// The machine this process runs on.
pub struct RealOps {
    env: HashMap<String, String>,
    windows: bool,
    git_limit: Duration,
}

impl Ops for RealOps {
    fn is_symlink(&self, p: &Path) -> bool {
        std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
    }

    fn canonical(&self, p: &Path) -> io::Result<PathBuf> {
        paddock_env::winpath::real(p)
    }

    fn volume_of(&self, p: &Path) -> Option<String> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(p).ok().map(|m| m.dev().to_string())
        }
        #[cfg(not(unix))]
        {
            // The drive (or share) of the physical path: the part before the first separator, lower-cased.
            let c = std::fs::canonicalize(p).ok()?;
            match c.components().next()? {
                std::path::Component::Prefix(pre) => Some(pre.as_os_str().to_string_lossy().to_lowercase()),
                _ => None,
            }
        }
    }

    fn create_dir_all(&self, p: &Path) -> io::Result<()> {
        std::fs::create_dir_all(p)
    }

    fn remove(&self, root: &Path, target: &Path) -> Result<bool, String> {
        let home = self.env.get("HOME").or_else(|| self.env.get("USERPROFILE")).map(PathBuf::from);
        safe_remove::remove_under(root, target, home.as_deref()).map_err(|e| e.to_string())
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn is_dir(&self, p: &Path) -> bool {
        p.is_dir()
    }

    fn git(&self, args: &[String]) -> Result<String, String> {
        let call = ToolCall::new("git").args(args.iter().cloned()).timeout(self.git_limit);
        let ran = call.run(&self.env, &mut io::sink())?;
        if ran.success {
            Ok(ran.stdout.trim().to_string())
        } else {
            Err(format!("git {} failed ({}): {}", args.join(" "), ran.how, ran.stderr.trim()))
        }
    }

    fn link_dir(&self, target: &Path, link: &Path) -> Result<(), String> {
        OsLinks {
            env: &self.env,
            windows: self.windows,
        }
        .link_dir(target, link)
    }
}

/// The `::error::` of a phase that did not finish.
fn timeout_error(step: &mut Step<'_>, label: &str, limit: Duration, ws: &str, last: &str) -> i32 {
    step.fail(
        format!(
            "'{label}' did not finish within {} s (the step moves the checkout of {ws} to a kept folder); a process left from an earlier job on this runner probably holds a file or lock there. Last output: {}.",
            limit.as_secs(),
            tail_flat(last, 300)
        ),
        "list and end processes using the CI home's src folder on the runner, then re-run the job.",
    )
}

/// The command over the real machine.
pub fn run(env: &HashMap<String, String>, windows: bool, args: &Args, out: &mut dyn Write) -> i32 {
    let limit = Duration::from_secs(
        env.get("PADDOCK_STABLE_WORKSPACE_TIMEOUT")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(300),
    );
    let ops: Arc<dyn Ops> = Arc::new(RealOps {
        env: env.clone(),
        windows,
        git_limit: limit,
    });
    run_with(env, windows, args, ops, limit, out)
}

/// The command over explicit operations (tests).
pub fn run_with(env: &HashMap<String, String>, windows: bool, args: &Args, ops: Arc<dyn Ops>, limit: Duration, out: &mut dyn Write) -> i32 {
    let mut step = Step {
        env,
        out,
        windows,
        name: "stable-workspace",
    };
    let Some(os) = step.var("RUNNER_OS").map(str::to_string) else {
        return step.fail("RUNNER_OS is not set.", "run this step in a runner job.");
    };
    // The Linux jobs run in a container with a fixed workspace already.
    if os == "Linux" {
        return 0;
    }
    let Some(work) = step.var("GITHUB_WORKSPACE").map(str::to_string) else {
        return step.fail("GITHUB_WORKSPACE is not set.", "run this step in a runner job.");
    };
    if ops.is_symlink(Path::new(&work)) {
        step.say("stable-workspace: already linked");
        return 0;
    }
    let ws = match ops.canonical(Path::new(&work)) {
        Ok(p) => p,
        Err(e) => {
            return step.fail(
                format!("cannot read the work folder {work}: {e}."),
                "check the runner's work folder.",
            );
        }
    };
    let ws_text = ws.display().to_string();
    let folder = folder_name(&args.name);

    // The job slot's own copy (job-isolation: src/<name>-<slot>), so a concurrent job of the same name never moves or cleans this
    // one. No slot (the job-isolation action did not run): build in place, which only costs cache hits.
    let resolved = {
        let (env2, win) = (env.clone(), windows);
        let folder = folder.clone();
        bounded(&mut step, "resolve the job slot's kept folder", limit, move || {
            let (code, o, e) = isolation_full(&env2, win, &["path", "src", &folder]);
            // The isolation's messages (a migration notice, an error) go to the log either way.
            if code == 0 { Ok((o, e)) } else { Err(e) }
        })
    };
    let stable_text = match resolved {
        Ok((path, notes)) => {
            if !notes.is_empty() {
                step.say(notes);
            }
            path
        }
        Err(Phase::Timeout) => return timeout_error(&mut step, "resolve the job slot's kept folder", limit, &ws_text, ""),
        Err(Phase::Failed(why)) => {
            step.say(why);
            step.warn("no kept folder of this job's slot (see the message above); building from the work folder.");
            return 0;
        }
    };
    let stable = PathBuf::from(&stable_text);
    let root = stable.parent().map(Path::to_path_buf).unwrap_or_default();
    let root_text = root.display().to_string();

    // The folder later steps build from: PADDOCK_WORKSPACE, which a step uses as its working-directory.
    let use_folder = |step: &mut Step<'_>, p: &Path| {
        let shown = if windows {
            home::native(&p.display().to_string())
        } else {
            p.display().to_string()
        };
        if let Err(e) = step.append_to("GITHUB_ENV", &format!("PADDOCK_WORKSPACE={shown}")) {
            step.warn(format!("could not export PADDOCK_WORKSPACE: {e}"));
        }
        step.say(format!("stable-workspace: building from {shown}"));
    };

    // The move is a rename only on one volume. Across volumes a move (and a clone) copies the whole checkout, which took 7 to 293 s
    // on a loaded Mac and hit the bound nine times: build in place instead, and say how to fix it. A volume that cannot be read is
    // unknown, never "the same": two empty answers must not let the move run.
    let same = {
        let (ops, ws, root) = (ops.clone(), ws.clone(), root.clone());
        bounded(
            &mut step,
            &format!("compare the volumes of {ws_text} and {root_text}"),
            limit,
            move || {
                ops.create_dir_all(&root)
                    .map_err(|e| format!("cannot create {}: {e}", root.display()))?;
                match (ops.volume_of(&ws), ops.volume_of(&root)) {
                    (Some(a), Some(b)) if !a.is_empty() && a == b => Ok(()),
                    (Some(_), Some(_)) => Err(String::new()),
                    _ => Err("a volume could not be read".to_string()),
                }
            },
        )
    };
    match same {
        Ok(()) => {}
        Err(Phase::Timeout) => {
            let label = format!("compare the volumes of {ws_text} and {root_text}");
            return timeout_error(&mut step, &label, limit, &ws_text, "");
        }
        Err(Phase::Failed(why)) => {
            step.warn(format!(
                "the work folder {ws_text} is on another volume than the kept folder {stable_text} (or one could not be read: {}), so moving it would copy the whole checkout; building from the work folder (sccache misses the workspace crates). Next step: put the runner's host.workdir_parent (config.yaml) on the CI home's volume.",
                tail_flat(&why, 300)
            ));
            return 0;
        }
    }

    // An unusable move must never fail the job: it only costs the cache hits of the workspace crates (a hang does: it ends the
    // step with the error above).
    let moved = {
        let (ops, ws, stable, root) = (ops.clone(), ws.clone(), stable.clone(), root.clone());
        bounded(&mut step, &format!("move the checkout to {stable_text}"), limit, move || {
            ops.create_dir_all(&root).map_err(|e| e.to_string())?;
            ops.remove(&root, &stable)?;
            ops.rename(&ws, &stable).map_err(|e| e.to_string())
        })
    };
    match moved {
        Ok(()) => {}
        Err(Phase::Timeout) => {
            let label = format!("move the checkout to {stable_text}");
            return timeout_error(&mut step, &label, limit, &ws_text, "");
        }
        Err(Phase::Failed(why)) => {
            // Under act on Windows the runner keeps a handle on the work folder, so it cannot move. Keep a clone at the stable
            // path instead and bring it to this commit.
            let label = format!("keep a clone of the checkout at {stable_text}");
            let cloned = {
                let (ops, ws, stable, root) = (ops.clone(), ws.clone(), stable.clone(), root.clone());
                bounded(&mut step, &label, limit, move || clone_checkout(ops.as_ref(), &ws, &stable, &root))
            };
            match cloned {
                Ok(()) => {
                    let head = ops
                        .git(&["-C".into(), stable_text.clone(), "rev-parse".into(), "HEAD".into()])
                        .unwrap_or_default();
                    let why = if why.is_empty() { "no message".to_string() } else { why };
                    step.say(format!(
                        "stable-workspace: could not move the checkout ({why}); a clone at {stable_text} is at {head} instead."
                    ));
                    use_folder(&mut step, &stable);
                }
                Err(Phase::Timeout) => return timeout_error(&mut step, &label, limit, &ws_text, &why),
                Err(Phase::Failed(clone_why)) => {
                    let why = if why.is_empty() { "no message".to_string() } else { why };
                    step.warn(format!(
                        "could not move the checkout to {stable_text} ({why}) nor clone it there ({clone_why}); building from the work folder (sccache misses the workspace crates)."
                    ));
                }
            }
            return 0;
        }
    }

    // A link (a junction on Windows: Git bash's `ln -s` copies there) resolves the old path to the stable folder.
    let linked = {
        let (ops, ws, stable) = (ops.clone(), ws.clone(), stable.clone());
        bounded(&mut step, &format!("link {ws_text} to {stable_text}"), limit, move || {
            ops.link_dir(&stable, &ws)
        })
    };
    match linked {
        Ok(()) => {}
        Err(Phase::Timeout) => {
            let label = format!("link {ws_text} to {stable_text}");
            return timeout_error(&mut step, &label, limit, &ws_text, "");
        }
        Err(Phase::Failed(_)) => {
            // Put the checkout back where the runner expects it.
            if let Err(e) = ops.rename(&stable, &ws) {
                step.say(format!("stable-workspace: could not move the checkout back to {ws_text}: {e}"));
            }
            step.warn(format!("could not link {ws_text} to {stable_text}; building from the work folder."));
            return 0;
        }
    }
    step.say(format!("stable-workspace: {ws_text} -> {stable_text}"));
    use_folder(&mut step, &stable);
    0
}

/// A local clone of the checkout at the stable path, at the checkout's commit, with every file that is not git's removed
/// (ignored files stay: no build output lives there, the build folder is elsewhere).
fn clone_checkout(ops: &dyn Ops, ws: &Path, stable: &Path, root: &Path) -> Result<(), String> {
    let s = |p: &Path| p.display().to_string();
    let sha = ops.git(&["-C".into(), s(ws), "rev-parse".into(), "HEAD".into()])?;
    if !ops.is_dir(&stable.join(".git")) {
        ops.remove(root, stable)?;
        ops.git(&[
            "clone".into(),
            "-q".into(),
            "--no-checkout".into(),
            "--no-tags".into(),
            s(ws),
            s(stable),
        ])?;
    }
    // The checkout is shallow: --update-shallow lets the clone take a commit whose parents it does not have.
    ops.git(&[
        "-C".into(),
        s(stable),
        "fetch".into(),
        "-q".into(),
        "--no-tags".into(),
        "--update-shallow".into(),
        s(ws),
        "HEAD".into(),
    ])?;
    let fetched = ops.git(&["-C".into(), s(stable), "rev-parse".into(), "FETCH_HEAD".into()])?;
    if fetched != sha {
        return Err(format!("the clone fetched {fetched}, not {sha}"));
    }
    ops.git(&[
        "-C".into(),
        s(stable),
        "checkout".into(),
        "-q".into(),
        "-f".into(),
        "--detach".into(),
        sha,
    ])?;
    ops.git(&["-C".into(), s(stable), "clean".into(), "-q".into(), "-ffd".into()])?;
    Ok(())
}

impl From<RemoveError> for String {
    fn from(e: RemoveError) -> String {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::super::links::fake::RecordingLinks;
    use super::*;
    use std::sync::Mutex;

    /// A scripted machine: records what was done, answers the volumes, fails what it is told to.
    #[derive(Default)]
    struct Fake {
        log: Mutex<Vec<String>>,
        symlink: bool,
        volumes: Option<(Option<&'static str>, Option<&'static str>)>,
        rename_fails: bool,
        git_fails_on: Option<&'static str>,
        link_fails: bool,
        /// While set, `create_dir_all` blocks until the gate is opened: a phase that hangs.
        stall: Option<Arc<(Mutex<bool>, std::sync::Condvar)>>,
        links: RecordingLinks,
    }

    impl Fake {
        fn did(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    impl Ops for Fake {
        fn is_symlink(&self, _: &Path) -> bool {
            self.symlink
        }
        fn canonical(&self, p: &Path) -> io::Result<PathBuf> {
            Ok(p.to_path_buf())
        }
        fn volume_of(&self, p: &Path) -> Option<String> {
            let (a, b) = self.volumes.unwrap_or((Some("1"), Some("1")));
            let v = if p.starts_with("/work") { a } else { b };
            v.map(str::to_string)
        }
        fn create_dir_all(&self, p: &Path) -> io::Result<()> {
            if let Some(gate) = &self.stall {
                let (open, cv) = &**gate;
                let mut guard = open.lock().unwrap();
                while !*guard {
                    guard = cv.wait(guard).unwrap();
                }
            }
            self.log.lock().unwrap().push(format!("mkdir {}", p.display()));
            Ok(())
        }
        fn remove(&self, root: &Path, target: &Path) -> Result<bool, String> {
            self.log
                .lock()
                .unwrap()
                .push(format!("rm {} under {}", target.display(), root.display()));
            Ok(true)
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            self.log.lock().unwrap().push(format!("mv {} {}", from.display(), to.display()));
            if self.rename_fails && !to.starts_with("/work") {
                return Err(io::Error::other("Access is denied. (os error 5)"));
            }
            Ok(())
        }
        fn is_dir(&self, _: &Path) -> bool {
            false
        }
        fn git(&self, args: &[String]) -> Result<String, String> {
            let a = args.join(" ");
            self.log.lock().unwrap().push(format!("git {a}"));
            if self.git_fails_on.is_some_and(|f| a.contains(f)) {
                return Err("fatal: boom".into());
            }
            Ok("abc123".to_string())
        }
        fn link_dir(&self, target: &Path, link: &Path) -> Result<(), String> {
            if self.link_fails {
                return Err("mklink failed".into());
            }
            self.links.link_dir(target, link)
        }
    }

    struct Scene {
        dir: tempfile::TempDir,
        env: HashMap<String, String>,
    }

    /// The runner's variables; the CI home is a scratch folder, so `ci isolation path` runs for real (and answers `/ci/...`
    /// paths through `home`? no: a real path under the scratch folder).
    fn scene(os: &str) -> Scene {
        let dir = crate::ci_host::canonical_tempdir();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let env: HashMap<String, String> = [
            ("RUNNER_OS", os.to_string()),
            ("PADDOCK_CI_SLOT", "s1".into()),
            ("HOME", home.display().to_string()),
            ("GITHUB_WORKSPACE", "/work/abc/hostexecutor".into()),
            ("GITHUB_ENV", dir.path().join("github-env").display().to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        Scene { dir, env }
    }

    fn go(sc: &Scene, win: bool, fake: Arc<Fake>, limit: u64) -> (i32, String) {
        let mut out = Vec::new();
        let code = run_with(
            &sc.env,
            win,
            &Args {
                name: "decryp7/paddock-crate".into(),
            },
            fake,
            Duration::from_secs(limit),
            &mut out,
        );
        (code, String::from_utf8(out).unwrap())
    }

    fn written(sc: &Scene) -> String {
        std::fs::read_to_string(sc.dir.path().join("github-env")).unwrap_or_default()
    }

    fn kept(sc: &Scene) -> String {
        format!("{}/.paddock-ci/src/decryp7_paddock-crate-s1", sc.dir.path().join("home").display())
    }

    #[test]
    fn a_linux_container_and_an_already_linked_folder_are_left_alone() {
        let sc = scene("Linux");
        let f = Arc::new(Fake::default());
        assert_eq!(go(&sc, false, f.clone(), 5), (0, String::new()));
        assert!(f.did().is_empty());
        let sc = scene("macOS");
        let f = Arc::new(Fake {
            symlink: true,
            ..Fake::default()
        });
        assert_eq!(go(&sc, false, f, 5), (0, "stable-workspace: already linked\n".to_string()));
    }

    #[test]
    fn the_checkout_moves_to_the_slots_folder_and_a_link_stays_where_it_was() {
        let sc = scene("macOS");
        let f = Arc::new(Fake::default());
        let (code, out) = go(&sc, false, f.clone(), 5);
        assert_eq!(code, 0, "{out}");
        let k = kept(&sc);
        assert!(
            out.contains(&format!("stable-workspace: move the checkout to {k}: done in ")),
            "{out}"
        );
        assert!(out.contains(&format!("stable-workspace: /work/abc/hostexecutor -> {k}")), "{out}");
        assert!(out.contains(&format!("stable-workspace: building from {k}")), "{out}");
        assert_eq!(written(&sc), format!("PADDOCK_WORKSPACE={k}\n"));
        assert!(f.did().iter().any(|l| l == &format!("mv /work/abc/hostexecutor {k}")));
        assert_eq!(
            f.links.made.lock().unwrap().as_slice(),
            [(k.clone(), "/work/abc/hostexecutor".to_string())]
        );
        // The old kept folder is removed under the CI home's src folder, never anywhere else.
        assert!(
            f.did()
                .iter()
                .any(|l| l == &format!("rm {k} under {}", std::path::Path::new(&k).parent().unwrap().display()))
        );
    }

    #[test]
    fn no_slot_or_another_volume_or_an_unreadable_one_builds_in_place_with_a_warning() {
        let mut sc = scene("macOS");
        sc.env.remove("PADDOCK_CI_SLOT");
        let (code, out) = go(&sc, false, Arc::new(Fake::default()), 5);
        assert_eq!(code, 0);
        assert!(
            out.contains("::warning::stable-workspace: no kept folder of this job's slot"),
            "{out}"
        );
        assert!(written(&sc).is_empty());

        let sc = scene("macOS");
        let f = Arc::new(Fake {
            volumes: Some((Some("1"), Some("2"))),
            ..Fake::default()
        });
        let (code, out) = go(&sc, false, f.clone(), 5);
        assert_eq!(code, 0);
        assert!(
            out.contains("::warning::stable-workspace: the work folder /work/abc/hostexecutor is on another volume than the kept folder"),
            "{out}"
        );
        assert!(out.contains("Next step: put the runner's host.workdir_parent"), "{out}");
        assert!(!f.did().iter().any(|l| l.starts_with("mv ")), "nothing is copied across volumes");

        for unreadable in [(None, Some("1")), (Some("1"), None), (None, None), (Some(""), Some(""))] {
            let f = Arc::new(Fake {
                volumes: Some(unreadable),
                ..Fake::default()
            });
            let (code, out) = go(&sc, false, f.clone(), 5);
            assert_eq!(code, 0);
            assert!(
                out.contains("::warning::stable-workspace: the work folder"),
                "{unreadable:?}: {out}"
            );
            assert!(
                !f.did().iter().any(|l| l.starts_with("mv ")),
                "{unreadable:?}: an unknown volume is never the same"
            );
        }
    }

    #[test]
    fn a_folder_that_cannot_move_keeps_a_clone_at_the_same_commit_and_builds_there() {
        let sc = scene("Windows");
        let f = Arc::new(Fake {
            rename_fails: true,
            ..Fake::default()
        });
        let (code, out) = go(&sc, true, f.clone(), 5);
        assert_eq!(code, 0, "{out}");
        let k = kept(&sc);
        let git: Vec<String> = f.did().into_iter().filter(|l| l.starts_with("git ")).collect();
        assert_eq!(git[0], "git -C /work/abc/hostexecutor rev-parse HEAD");
        assert_eq!(git[1], format!("git clone -q --no-checkout --no-tags /work/abc/hostexecutor {k}"));
        assert!(
            git[2].contains("fetch -q --no-tags --update-shallow /work/abc/hostexecutor HEAD"),
            "{git:?}"
        );
        assert!(
            git[4].contains("checkout -q -f --detach abc123") && git[5].contains("clean -q -ffd"),
            "{git:?}"
        );
        assert!(
            out.contains("could not move the checkout (Access is denied. (os error 5)); a clone at"),
            "{out}"
        );
        assert!(out.contains("is at abc123 instead."), "{out}");
        assert!(written(&sc).starts_with("PADDOCK_WORKSPACE="), "the job builds in the clone");
        assert!(f.links.made.lock().unwrap().is_empty(), "no link when nothing moved");
    }

    #[test]
    fn a_failed_clone_after_a_failed_move_warns_and_builds_in_place() {
        let sc = scene("Windows");
        let f = Arc::new(Fake {
            rename_fails: true,
            git_fails_on: Some("clone"),
            ..Fake::default()
        });
        let (code, out) = go(&sc, true, f, 5);
        assert_eq!(code, 0);
        assert!(out.contains("::warning::stable-workspace: could not move the checkout to"), "{out}");
        assert!(out.contains("nor clone it there (fatal: boom)"), "{out}");
        assert!(written(&sc).is_empty());
    }

    #[test]
    fn a_clone_that_fetched_another_commit_than_the_checkouts_is_an_error_never_a_silent_wrong_tree() {
        struct Skewed(Fake);
        impl Ops for Skewed {
            fn is_symlink(&self, p: &Path) -> bool {
                self.0.is_symlink(p)
            }
            fn canonical(&self, p: &Path) -> io::Result<PathBuf> {
                self.0.canonical(p)
            }
            fn volume_of(&self, p: &Path) -> Option<String> {
                self.0.volume_of(p)
            }
            fn create_dir_all(&self, p: &Path) -> io::Result<()> {
                self.0.create_dir_all(p)
            }
            fn remove(&self, r: &Path, t: &Path) -> Result<bool, String> {
                self.0.remove(r, t)
            }
            fn rename(&self, a: &Path, b: &Path) -> io::Result<()> {
                self.0.rename(a, b)
            }
            fn is_dir(&self, p: &Path) -> bool {
                self.0.is_dir(p)
            }
            fn git(&self, args: &[String]) -> Result<String, String> {
                let a = args.join(" ");
                Ok(if a.contains("FETCH_HEAD") {
                    "def456".into()
                } else {
                    "abc123".into()
                })
            }
            fn link_dir(&self, t: &Path, l: &Path) -> Result<(), String> {
                self.0.link_dir(t, l)
            }
        }
        let sc = scene("Windows");
        let mut out = Vec::new();
        let ops: Arc<dyn Ops> = Arc::new(Skewed(Fake {
            rename_fails: true,
            ..Fake::default()
        }));
        let code = run_with(&sc.env, true, &Args { name: "n".into() }, ops, Duration::from_secs(5), &mut out);
        let out = String::from_utf8(out).unwrap();
        assert_eq!(code, 0);
        assert!(out.contains("nor clone it there (the clone fetched def456, not abc123)"), "{out}");
        assert!(written(&sc).is_empty());
    }

    #[test]
    fn a_link_that_cannot_be_made_moves_the_checkout_back_and_builds_in_place() {
        let sc = scene("macOS");
        let f = Arc::new(Fake {
            link_fails: true,
            ..Fake::default()
        });
        let (code, out) = go(&sc, false, f.clone(), 5);
        assert_eq!(code, 0);
        let k = kept(&sc);
        assert!(
            out.contains("::warning::stable-workspace: could not link /work/abc/hostexecutor to"),
            "{out}"
        );
        assert!(
            f.did().iter().any(|l| l == &format!("mv {k} /work/abc/hostexecutor")),
            "moved back: {:?}",
            f.did()
        );
        assert!(written(&sc).is_empty());
    }

    #[test]
    fn a_junction_is_the_link_on_windows() {
        let sc = scene("Windows");
        let f = Arc::new(Fake::default());
        let (code, out) = go(&sc, true, f.clone(), 5);
        assert_eq!(code, 0, "{out}");
        assert_eq!(f.links.made.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_hung_phase_ends_in_one_error_line_that_names_it() {
        let sc = scene("macOS");
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let f = Arc::new(Fake {
            stall: Some(gate.clone()),
            ..Fake::default()
        });
        let (code, out) = go(&sc, false, f, 1);
        // Let the hung phase end, so no thread is left waiting.
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        assert_eq!(code, 1, "{out}");
        let errors: Vec<&str> = out.lines().filter(|l| l.starts_with("::error::")).collect();
        assert_eq!(errors.len(), 1, "{out}");
        assert!(
            errors[0].contains("stable-workspace: 'compare the volumes of /work/abc/hostexecutor and"),
            "{}",
            errors[0]
        );
        assert!(errors[0].contains("did not finish within 1 s"), "{}", errors[0]);
        assert!(
            errors[0].contains("Next step: list and end processes using the CI home's src folder"),
            "{}",
            errors[0]
        );
        assert!(
            out.contains("stable-workspace: resolve the job slot's kept folder: done in "),
            "phases log their duration: {out}"
        );
    }
}
