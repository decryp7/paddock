//! `dev ci kept-clone`: the kept clone of a host job (the port of `conformance/ci/kept-clone.sh` and the action `.gitea/actions/kept-clone`;
//! ADR 0025, "Kept clones"; docs/plans/ci-in-rust.md, WP-5).
//!
//! A host runner checks out into a per-job folder that is deleted afterwards, so every job paid for a full fetch and a write of the whole
//! tree. Instead each job slot keeps ONE git clone in its own kept folder (`ci isolation path src <name>`), brought to the job's exact
//! commit by a fetch of that commit and a checkout, so only what changed since the slot's last job crosses the network or the disk, and a
//! profile (`conformance/ci/checkout-profiles.txt`) leaves out the paths the job never reads.
//!
//! The caller first runs `actions/checkout` sparse and partial (the bootstrap folders). This step then:
//!
//! 1. resolves the slot's kept folder (per slot, so concurrent jobs never share a clone);
//! 2. makes the clone (no checkout, `blob:none`, depth 1) when it is missing, broken, of another repository, marked by nobody, carries a
//!    git config key this step did not write, or is larger than `PADDOCK_KEPT_CLONE_MAX_MIB` (default 3072); a tag run (`--fresh`) always
//!    makes it again;
//! 3. cleans a reused clone, fetches the exact commit, writes the profile's sparse patterns, checks the commit out and re-applies them;
//! 4. verifies: HEAD is the commit, the tree has no change or stray file, no excluded path exists;
//! 5. exports `PADDOCK_WORKSPACE` and, off Windows, leaves a link at the work folder.
//!
//! Every phase is bounded (`PADDOCK_KEPT_CLONE_TIMEOUT` seconds, default 900; one phase's own limit `PADDOCK_KEPT_CLONE_TIMEOUT_<ID>`) and logs its duration (ER-27). A clone that cannot be made
//! never fails the job, and neither does a phase that hangs: a phase that fails is retried once on a new clone (stale `.git` locks removed
//! first), and then the work folder is expanded to the profile instead (one `::warning::`; only the compile cache of the workspace crates
//! is lost). The clone's `.git` survives between jobs and is shared by every job of the slot, so nothing in it is trusted: every git
//! command runs with `core.hooksPath` on an empty folder this step owns, `.git/hooks` is emptied, `info/attributes`, `info/exclude` and
//! `refs/replace` are removed, and a local config key outside an allow-list makes the clone be made again.
//! The checkout's credentials (its http extra header) reach git through the environment, never a command line, never kept.

use super::phase::{Phase, bounded_hung};
use super::safe_remove;
use super::{Step, folder_name, isolation_full};
use crate::ci_isolation::home;
use crate::ci_tool::{NETWORK_ENV, ToolCall};
use clap::Args as ClapArgs;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// `dev ci kept-clone`.
#[derive(ClapArgs, Debug)]
pub struct KeptCloneArgs {
    /// Folder name under the CI home's `src/` (the job's slot is appended): one per repository and job.
    #[arg(long)]
    pub name: String,
    /// The checkout profile of `conformance/ci/checkout-profiles.txt` (`full` leaves nothing out).
    #[arg(long)]
    pub profile: String,
    /// Never reuse the slot's clone, make it again (a tag run).
    #[arg(long)]
    pub fresh: bool,
    /// The profile file (default: `KEPT_CLONE_PROFILES`, else `conformance/ci/checkout-profiles.txt` of the action's checkout).
    #[arg(long)]
    pub profiles: Option<PathBuf>,
}

/// The short name the CLI uses.
pub type Args = KeptCloneArgs;

const NAME: &str = "kept-clone";

/// Local config keys a clone of this step may carry: what git itself writes for a partial, shallow clone, the remote, and the sparse
/// settings this step sets. A key such as `core.autocrlf`, `core.fsmonitor`, `core.sshCommand`, `credential.helper` or an include changes
/// what a git command does, so the clone that carries one is not reused.
const ALLOWED_KEYS: &str = r"^(core\.(repositoryformatversion|filemode|bare|logallrefupdates|ignorecase|precomposeunicode|symlinks|sparsecheckout|sparsecheckoutcone)|extensions\.(partialclone|objectformat)|remote\.origin\.(url|fetch|tagopt|promisor|partialclonefilter)|branch\.[^.=]+\.(remote|merge))$";

/// The bound of a git command outside a phase (reading the work folder, the health probe, the fallback's expansion): the default phase
/// limit. `PADDOCK_KEPT_CLONE_TIMEOUT` is seconds per phase; the shell runs these reads unbounded, so a short phase limit must not end
/// them (it failed the job under load with the hung-phase test's 1 s limit, where the shell went on).
const OUTSIDE_PHASE_LIMIT: Duration = Duration::from_secs(900);

thread_local! {
    /// Set on a phase's own thread: its git commands get that phase's limit.
    static IN_PHASE: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
}

/// The phases, each with its own limit `PADDOCK_KEPT_CLONE_TIMEOUT_<ID>` over the general `PADDOCK_KEPT_CLONE_TIMEOUT` (the shell's
/// `phase <ID>`): a job (or a test) can bound one phase tightly without the others meeting that bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PhaseId {
    Resolve,
    Create,
    Clean,
    Fetch,
    Checkout,
    Verify,
    Link,
}

impl PhaseId {
    /// The variable of the phase's own limit.
    fn var(self) -> &'static str {
        match self {
            PhaseId::Resolve => "PADDOCK_KEPT_CLONE_TIMEOUT_RESOLVE",
            PhaseId::Create => "PADDOCK_KEPT_CLONE_TIMEOUT_CREATE",
            PhaseId::Clean => "PADDOCK_KEPT_CLONE_TIMEOUT_CLEAN",
            PhaseId::Fetch => "PADDOCK_KEPT_CLONE_TIMEOUT_FETCH",
            PhaseId::Checkout => "PADDOCK_KEPT_CLONE_TIMEOUT_CHECKOUT",
            PhaseId::Verify => "PADDOCK_KEPT_CLONE_TIMEOUT_VERIFY",
            PhaseId::Link => "PADDOCK_KEPT_CLONE_TIMEOUT_LINK",
        }
    }
}

/// The phase's limit: its own variable when it is a whole number above 0 (digits, no leading zero, as the shell tests it), else
/// `general`.
fn phase_limit(env: &HashMap<String, String>, id: PhaseId, general: Duration) -> Duration {
    env.get(id.var())
        .filter(|v| !v.is_empty() && !v.starts_with('0') && v.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(general, Duration::from_secs)
}

/// What the phases share: the job's environment, the credentials for git, the folders and the two buffers a phase writes to (what the
/// shell's `$scratch/err` and the `run:` lines were).
struct Kc {
    env: HashMap<String, String>,
    windows: bool,
    limit: Duration,
    /// A git command ended at its limit (the phase is then reported as one that did not finish, as the shell's `bounded` did).
    timed_out: std::sync::atomic::AtomicBool,
    /// The checkout's `http.*.extraheader` value, for git's environment only.
    header: Option<String>,
    /// An empty folder this step owns: `core.hooksPath` of every git command.
    hooks: PathBuf,
    home: Option<PathBuf>,
    /// The phase's error output (cleared at the start of each phase).
    err: Mutex<String>,
    /// `run:` and `ran:` lines of the noisy git commands, flushed to the job log after each phase.
    log: Mutex<Vec<u8>>,
}

impl Kc {
    fn note(&self, text: impl AsRef<str>) {
        // A command that wrote nothing (a git ended at its limit) adds nothing, as the shell's `$scratch/err` (no blank line).
        if text.as_ref().is_empty() {
            return;
        }
        if let Ok(mut e) = self.err.lock() {
            e.push_str(text.as_ref());
            if !text.as_ref().ends_with('\n') {
                e.push('\n');
            }
        }
    }

    /// The last four whole, non-blank lines of the phase's error output on one line (never cut in the middle of a path).
    fn last_lines(&self) -> String {
        let text = self.err.lock().map(|e| e.clone()).unwrap_or_default();
        let lines: Vec<&str> = text.split('\n').filter(|l| !l.trim().is_empty()).collect();
        let from = lines.len().saturating_sub(4);
        lines[from..].join("\n").replace(['\r', '\n'], " ")
    }

    /// One git command with this step's guard: no hooks, no replace objects, no prompts, the credentials through the environment.
    /// `noisy`: the command is logged and a failure names it in the phase's error output (the shell's `g`; `gq` is the quiet probe).
    /// `Ok` is standard output without the trailing newlines; `Err` is how it ended.
    fn git(&self, noisy: bool, args: &[&str]) -> Result<String, String> {
        let mut call = ToolCall::new("git")
            .args(["-c".to_string(), format!("core.hooksPath={}", self.hooks.display())])
            .args(args.iter().map(|a| a.to_string()))
            .pass(NETWORK_ENV.iter().copied())
            .pass(["GIT_CONFIG_NOSYSTEM", "GIT_CONFIG_GLOBAL", "GIT_SSL_CAINFO", "GIT_SSL_CAPATH"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .timeout(IN_PHASE.with(std::cell::Cell::get).unwrap_or(OUTSIDE_PHASE_LIMIT));
        if let Some(h) = &self.header {
            call = call
                .env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", "http.extraheader")
                .env("GIT_CONFIG_VALUE_0", h.clone())
                .secret(h.clone());
        }
        let sub = subcommand(args);
        let ran = if noisy {
            let mut log = self.log.lock().map_err(|_| "the log is poisoned".to_string())?;
            call.run(&self.env, &mut *log)
        } else {
            call.run(&self.env, &mut io::sink())
        };
        match ran {
            Ok(r) => {
                self.note(&r.stderr);
                if r.success {
                    return Ok(r.stdout.trim_end_matches(['\n', '\r']).to_string());
                }
                if noisy {
                    self.note(format!("{NAME}: git {sub} failed ({})", r.how));
                }
                if r.how.starts_with("timed out") {
                    self.timed_out.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                Err(r.how)
            }
            Err(e) => {
                self.note(&e);
                Err(e)
            }
        }
    }

    /// `rm -rf target`, guarded: only strictly under `root`, a link as a link ([`safe_remove`]).
    fn remove(&self, root: &Path, target: &Path) -> Result<(), String> {
        safe_remove::remove_under(root, target, self.home.as_deref())
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// The git subcommand: the first word that is not `-C <dir>` or `-c <key=value>` (for the failure line).
fn subcommand(args: &[&str]) -> String {
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        match *a {
            "-C" | "-c" => skip = true,
            x if x.starts_with('-') => {}
            x => return x.to_string(),
        }
    }
    String::new()
}

/// What the profile file leaves out for `profile`: the paths of its `exclude <profile> <path> ...` lines, a trailing `/` dropped.
pub fn excludes_of(text: &str, profile: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            let (kind, prof, path) = (w.next()?, w.next()?, w.next()?);
            (kind == "exclude" && prof == profile).then(|| path.trim_end_matches('/').to_string())
        })
        .collect()
}

/// One path segment pattern against a name: `*` is any run of characters, every other character stands for itself; a name that starts with
/// a dot is matched only by a pattern that starts with one (a shell glob without `dotglob`).
fn segment_matches(pattern: &str, name: &str) -> bool {
    if name.starts_with('.') && !pattern.starts_with('.') {
        return false;
    }
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ni, mut star, mut mark) = (0, 0, None, 0);
    while ni < n.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if pi < p.len() && p[pi] == n[ni] {
            pi += 1;
            ni += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// The paths under `base` that `pattern` (folder names, `*` inside a name) names and that exist (`compgen -G`).
pub fn glob_expand(base: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut found = vec![base.to_path_buf()];
    for seg in pattern.split('/').filter(|s| !s.is_empty()) {
        let mut next = Vec::new();
        for dir in &found {
            if seg.contains('*') {
                let Ok(rd) = std::fs::read_dir(dir) else { continue };
                let mut names: Vec<String> = rd
                    .flatten()
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|n| segment_matches(seg, n))
                    .collect();
                names.sort();
                next.extend(names.into_iter().map(|n| dir.join(n)));
            } else {
                let p = dir.join(seg);
                if std::fs::symlink_metadata(&p).is_ok() {
                    next.push(p);
                }
            }
        }
        found = next;
    }
    found
}

/// The keys of `git config --local --list` output that this step did not write: the first one, lower-cased (empty when there is none).
fn foreign_key_of(list: &str) -> String {
    // A pattern that cannot be built (it can not, a test builds it) would count every key as foreign: the clone is made again.
    let Ok(allowed) = regex::Regex::new(ALLOWED_KEYS) else {
        return "(the allow-list)".to_string();
    };
    list.lines()
        .map(|l| l.split('=').next().unwrap_or("").to_lowercase())
        .find(|k| !allowed.is_match(k))
        .unwrap_or_default()
}

/// The size of a clone in KiB from `git count-objects -v` (the sum of `size:` and `size-pack:`).
fn clone_kib_of(counts: &str) -> u64 {
    counts
        .lines()
        .filter(|l| l.starts_with("size: ") || l.starts_with("size-pack: "))
        .filter_map(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        .sum()
}

/// The `.angular/cache` links of a clone (a symlink, a junction on Windows) up to six levels deep, outside `node_modules` and `.git`.
/// They point to a kept folder outside the clone and are removed by themselves first: a clean must never reach the target through them.
fn angular_cache_links(stable: &Path) -> Vec<PathBuf> {
    fn is_link(p: &Path, meta: &std::fs::Metadata) -> bool {
        meta.file_type().is_symlink() || (cfg!(windows) && std::fs::read_link(p).is_ok())
    }
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
            let name = e.file_name();
            if name == "cache" && dir.file_name().is_some_and(|d| d == ".angular") && is_link(&p, &meta) {
                out.push(p.clone());
            }
            if depth < 6 && meta.is_dir() && !is_link(&p, &meta) && name != "node_modules" && name != ".git" {
                walk(&p, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(stable, 1, &mut out);
    out
}

/// The command over the real machine.
pub fn run(env: &HashMap<String, String>, windows: bool, args: &Args, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut step = Step {
        env,
        out,
        windows,
        name: NAME,
    };
    let Some(os) = step.var("RUNNER_OS").map(str::to_string) else {
        return step.fail("RUNNER_OS is not set.", "run this step in a runner job.");
    };
    // Linux jobs run in a container with a fixed workspace.
    if os == "Linux" {
        return 0;
    }
    let started = Instant::now();
    let limit = Duration::from_secs(
        env.get("PADDOCK_KEPT_CLONE_TIMEOUT")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(900),
    );
    let max_mib: u64 = env.get("PADDOCK_KEPT_CLONE_MAX_MIB").and_then(|v| v.parse().ok()).unwrap_or(3072);
    let profile = args.profile.clone();
    let profiles: PathBuf = args
        .profiles
        .clone()
        .or_else(|| step.var("KEPT_CLONE_PROFILES").map(PathBuf::from))
        .or_else(|| {
            step.var("GITHUB_ACTION_PATH")
                .map(|p| Path::new(p).join("../../../conformance/ci/checkout-profiles.txt"))
        })
        .unwrap_or_else(|| {
            PathBuf::from(step.var("GITHUB_WORKSPACE").unwrap_or("."))
                .join("conformance")
                .join("ci")
                .join("checkout-profiles.txt")
        });
    let hint = "list and end processes using the CI home's src folder on the runner (a git or an editor left from an earlier job), then re-run the job.";

    // --- the profile ---
    let excludes = excludes_of(&std::fs::read_to_string(&profiles).unwrap_or_default(), &profile);
    if profile != "full" && excludes.is_empty() {
        return step.fail(
            format!(
                "profile '{profile}' is not declared in {} (an `exclude {profile} <path>` line).",
                profiles.display()
            ),
            "declare it there, or use 'full'.",
        );
    }

    // --- what the plain checkout left ---
    let Some(work) = step.var("GITHUB_WORKSPACE").map(str::to_string) else {
        return step.fail(
            "GITHUB_WORKSPACE is not set.",
            "run this step in a runner job, after actions/checkout.",
        );
    };
    let ws = match paddock_env::winpath::real(Path::new(&work)) {
        Ok(p) => p,
        Err(e) => {
            return step.fail(
                format!("cannot read the work folder {work}: {e}."),
                "check the runner's work folder.",
            );
        }
    };
    let ws_text = ws.display().to_string();
    // The empty hooks folder goes in the job's temp folder; without one in the environment, beside the work folder (the runner's own
    // per-job folder, which is writable).
    let Some(temp_root) = super::temp_folder(env).or_else(|| ws.parent().map(Path::to_path_buf)) else {
        return step.fail(
            "no folder for the empty hooks folder (no temp variable is set and the work folder has no parent).",
            "run this step in a runner job.",
        );
    };
    let hooks = temp_root.join(format!("paddock-kept-clone-{}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&hooks) {
        return step.fail(
            format!("cannot make the empty hooks folder {}: {e}.", hooks.display()),
            "make the runner's temp folder writable.",
        );
    }
    let kc = Arc::new(Kc {
        env: env.clone(),
        windows,
        limit,
        timed_out: Default::default(),
        header: None,
        hooks: hooks.clone(),
        home: env.get("HOME").or_else(|| env.get("USERPROFILE")).map(PathBuf::from),
        err: Mutex::new(String::new()),
        log: Mutex::new(Vec::new()),
    });
    let code = run_inner(
        &mut step, kc, args, &profile, &excludes, &ws, &ws_text, started, limit, max_mib, hint, err,
    );
    let _ = std::fs::remove_dir(&hooks);
    code
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    step: &mut Step<'_>,
    kc: Arc<Kc>,
    args: &Args,
    profile: &str,
    excludes: &[String],
    ws: &Path,
    ws_text: &str,
    started: Instant,
    limit: Duration,
    max_mib: u64,
    hint: &str,
    err: &mut dyn Write,
) -> i32 {
    let git_ws = |kc: &Kc, rest: &[&str]| -> Result<String, String> {
        let mut a = vec!["-C", ws_text];
        a.extend_from_slice(rest);
        kc.git(true, &a)
    };
    let (sha, url) = match (git_ws(&kc, &["rev-parse", "HEAD"]), git_ws(&kc, &["remote", "get-url", "origin"])) {
        (Ok(s), Ok(u)) => (s, u),
        (a, b) => {
            let why = a.err().or(b.err()).unwrap_or_default();
            let _ = write!(err, "{}", kc.err.lock().map(|e| e.clone()).unwrap_or_default());
            return step.fail(
                format!("cannot read the commit and the origin of the work folder {ws_text} ({why})."),
                "run this step after actions/checkout, in the job's checkout.",
            );
        }
    };
    // The checkout's credential header, passed through the environment (git >= 2.31), never on a command line. Read with the quiet
    // probe: a checkout without one is fine.
    let header = kc
        .git(
            false,
            &["-C", ws_text, "config", "--local", "--get-regexp", r"^http\..*\.extraheader$"],
        )
        .ok()
        .and_then(|t| t.lines().next().map(str::to_string))
        .and_then(|l| l.split_once(' ').map(|(_, v)| v.to_string()))
        .filter(|v| !v.is_empty());
    // The shared context is made again with the header (it is immutable behind the Arc).
    let kc = Arc::new(Kc {
        env: kc.env.clone(),
        windows: kc.windows,
        limit: kc.limit,
        timed_out: Default::default(),
        header,
        hooks: kc.hooks.clone(),
        home: kc.home.clone(),
        err: Mutex::new(String::new()),
        log: Mutex::new(Vec::new()),
    });
    let mut flow = Flow {
        kc,
        ws: ws.to_path_buf(),
        ws_text: ws_text.to_string(),
        stable: PathBuf::new(),
        sha,
        url,
        excludes: excludes.to_vec(),
        limit,
        hint: hint.to_string(),
    };

    // fallback <why>: build from the work folder, expanded to the profile; never fails the job for lack of a clone.
    macro_rules! fallback {
        ($why:expr) => {{
            step.warn(format!(
                "{}; building from the work folder (the compile cache misses the workspace crates).",
                $why
            ));
            flow.apply_profile(&flow.ws.clone()).ok();
            match flow.kc.git(true, &["-C", &flow.ws_text, "read-tree", "-mu", "HEAD"]) {
                Ok(t) => {
                    if !t.is_empty() {
                        step.say(t);
                    }
                }
                Err(_) => {
                    flow.flush(step);
                    step.say(flow.kc.err.lock().map(|e| e.clone()).unwrap_or_default().trim_end().to_string());
                    step.warn("could not expand the work folder to the profile; steps that read paths outside the bootstrap will fail.");
                }
            }
            flow.flush(step);
            return 0;
        }};
    }

    // --- the slot's kept folder ---
    let folder = folder_name(&args.name);
    let resolved = {
        let (env2, win) = (flow.kc.env.clone(), flow.kc.windows);
        flow.phase(step, PhaseId::Resolve, "resolve the job slot's kept folder", move |_| {
            let (code, o, e) = isolation_full(&env2, win, &["path", "src", &folder]);
            if code == 0 { Ok((o, e)) } else { Err(e) }
        })
    };
    let stable_text = match resolved {
        Ok((path, notes)) => {
            if !notes.is_empty() {
                let _ = writeln!(err, "{notes}");
            }
            path
        }
        Err(Failed(why)) => {
            let _ = writeln!(err, "{why}");
            fallback!("no kept folder of this job's slot (see the message above)");
        }
        Err(Hung) => {
            fallback!("no kept folder of this job's slot (see the message above)");
        }
    };
    let stable = PathBuf::from(&stable_text);
    flow.stable = stable.clone();

    // --- the clone ---
    let mut mode_created = args.fresh;
    if !mode_created && flow.healthy() {
        let kib = flow
            .kc
            .git(false, &["-C", &stable_text, "count-objects", "-v"])
            .map(|t| clone_kib_of(&t))
            .unwrap_or(0);
        let bad = flow.foreign_key();
        if !bad.is_empty() {
            step.warn(format!(
                "the clone at {stable_text} carries the local git config key '{bad}', which this script did not write; starting again."
            ));
            mode_created = true;
        } else if kib > max_mib * 1024 {
            step.say(format!(
                "{NAME}: the clone at {stable_text} is {} MiB, larger than {max_mib} MiB; starting again.",
                kib / 1024
            ));
            mode_created = true;
        }
    } else {
        mode_created = true;
    }
    if mode_created && flow.create(step, err).is_err() {
        fallback!(format!("could not create the clone at {stable_text} ({})", flow.kc.last_lines()));
    }

    // bring_up: clean (a reused clone), fetch, check out, verify; each phase bounded.
    if !flow.bring_up(step, mode_created) {
        step.warn(format!(
            "the clone at {stable_text} could not be brought to {} ({}); removing its locks and making it again.",
            flow.sha,
            flow.kc.last_lines()
        ));
        flow.remove_locks();
        mode_created = true;
        if flow.create(step, err).is_err() {
            fallback!(format!(
                "could not make the clone again at {stable_text} ({})",
                flow.kc.last_lines()
            ));
        }
        if !flow.bring_up(step, mode_created) {
            fallback!(format!(
                "the clone at {stable_text} could not be brought to {} ({})",
                flow.sha,
                flow.kc.last_lines()
            ));
        }
    }
    let files = flow
        .kc
        .git(true, &["-C", &stable_text, "ls-files", "-v"])
        .map(|t| t.lines().filter(|l| !l.starts_with('S')).count())
        .unwrap_or(0);
    flow.flush(step);

    // --- hand over ---
    // RUNNER_OS decides, as in the shell (the runner's OS, not the host's semantics).
    let runner_windows = step.var("RUNNER_OS") == Some("Windows");
    let shown = if runner_windows {
        home::native(&stable_text)
    } else {
        stable_text.clone()
    };
    if let Err(e) = step.append_to("GITHUB_ENV", &format!("PADDOCK_WORKSPACE={shown}")) {
        return step.fail(
            format!("could not export PADDOCK_WORKSPACE: {e}."),
            "run this step in a runner job.",
        );
    }
    // Where the work folder can be replaced (not under act on Windows, which holds it open) a link keeps every path that names it valid.
    if !runner_windows {
        let (ws2, stable2) = (flow.ws.clone(), stable.clone());
        let label = format!("link {ws_text} to {stable_text}");
        if flow
            .phase(step, PhaseId::Link, &label, move |kc| link_work_folder(kc, &ws2, &stable2))
            .is_err()
        {
            step.warn(format!(
                "could not link {ws_text} to {stable_text} ({}); steps build from PADDOCK_WORKSPACE.",
                flow.kc.last_lines()
            ));
        }
    }
    step.say(format!(
        "{NAME}: ready: {} ({}, profile {profile}, {files} files) in {} s",
        flow.sha,
        if mode_created { "created" } else { "reused" },
        started.elapsed().as_secs()
    ));
    0
}

/// Rename, then link, then delete the old folder: a folder held open (or a failed link) leaves the work folder where it was.
fn link_work_folder(kc: &Kc, ws: &Path, stable: &Path) -> Result<(), String> {
    let parent = ws.parent().ok_or("the work folder has no parent")?;
    let old = PathBuf::from(format!("{}.old-{}", ws.display(), std::process::id()));
    std::fs::rename(ws, &old).map_err(|e| {
        kc.note(format!("cannot move {} aside: {e}", ws.display()));
        e.to_string()
    })?;
    let linked = super::links::OsLinks {
        env: &kc.env,
        windows: kc.windows,
    };
    if let Err(e) = super::links::Links::link_dir(&linked, stable, ws) {
        let _ = std::fs::rename(&old, ws);
        kc.note(format!("cannot link {} to {}: {e}", ws.display(), stable.display()));
        return Err(e);
    }
    if let Err(e) = kc.remove(parent, &old) {
        kc.note(format!("{NAME}: could not delete the replaced work folder {}: {e}", old.display()));
    }
    Ok(())
}

/// Why a soft phase gave no value.
enum Soft {
    /// It failed with this message.
    Failed(String),
    /// It did not finish in time (the warning is already printed).
    Hung,
}
use Soft::{Failed, Hung};

/// The kept clone's state through the phases.
struct Flow {
    kc: Arc<Kc>,
    ws: PathBuf,
    ws_text: String,
    /// The kept folder (set once resolved).
    stable: PathBuf,
    sha: String,
    url: String,
    excludes: Vec<String>,
    limit: Duration,
    hint: String,
}

impl Flow {
    fn stable_text(&self) -> String {
        self.stable.display().to_string()
    }

    /// Moves the `run:` lines of the noisy git commands to the job log.
    fn flush(&self, step: &mut Step<'_>) {
        let text = self
            .kc
            .log
            .lock()
            .map(|mut l| String::from_utf8_lossy(&std::mem::take(&mut *l)).into_owned())
            .unwrap_or_default();
        for line in text.lines() {
            step.say(line);
        }
    }

    /// A bounded phase that never fails the job (the shell's `BOUNDED_SOFT=1`): a failure is the caller's to handle, a timeout is one
    /// `::warning::` naming the phase, the last output and the next step.
    fn phase<T: Send + 'static>(
        &self,
        step: &mut Step<'_>,
        id: PhaseId,
        label: &str,
        f: impl FnOnce(&Kc) -> Result<T, String> + Send + 'static,
    ) -> Result<T, Soft> {
        let limit = phase_limit(&self.kc.env, id, self.limit);
        if let Ok(mut e) = self.kc.err.lock() {
            e.clear();
        }
        self.kc.timed_out.store(false, std::sync::atomic::Ordering::SeqCst);
        let kc = self.kc.clone();
        // A git command ends itself at the limit (and its whole process tree with it); the phase waits one second longer so that it is
        // the git command's end that is seen first, and a phase that hangs elsewhere (a file call) is abandoned after that.
        let r = bounded_hung(step, label, limit + Duration::from_secs(1), Some(&self.kc.timed_out), move || {
            IN_PHASE.with(|p| p.set(Some(limit)));
            f(&kc)
        });
        self.flush(step);
        match r {
            Ok(v) => Ok(v),
            Err(Phase::Failed(e)) => Err(Failed(e)),
            Err(Phase::Timeout) => {
                step.warn(format!(
                    "'{label}' did not finish within {} s. Last output: {}. Next step: {}",
                    limit.as_secs(),
                    self.kc.last_lines(),
                    self.hint
                ));
                Err(Hung)
            }
        }
    }

    /// The sparse patterns of the profile (everything but the excluded paths) for the tree's git.
    fn apply_profile(&self, tree: &Path) -> Result<(), String> {
        let t = tree.display().to_string();
        self.kc
            .git(true, &["-C", &t, "config", "core.sparseCheckout", "true"])
            .map_err(|e| e.to_string())?;
        self.kc
            .git(true, &["-C", &t, "config", "core.sparseCheckoutCone", "false"])
            .map_err(|e| e.to_string())?;
        let info = tree.join(".git").join("info");
        std::fs::create_dir_all(&info).map_err(|e| e.to_string())?;
        let mut text = String::from("/*\n");
        for p in &self.excludes {
            text.push_str(&format!("!/{p}/\n"));
        }
        std::fs::write(info.join("sparse-checkout"), text).map_err(|e| e.to_string())
    }

    /// Exit 0 when the kept folder holds a usable clone of this repository, made by this step (the marker).
    fn healthy(&self) -> bool {
        let t = self.stable_text();
        self.stable.join(".git").join("paddock-kept-clone").is_file()
            && self.kc.git(false, &["-C", &t, "rev-parse", "--git-dir"]).is_ok()
            && self
                .kc
                .git(false, &["-C", &t, "config", "remote.origin.url"])
                .is_ok_and(|u| u == self.url)
    }

    /// The first local config key outside the allow-list (empty when there is none).
    fn foreign_key(&self) -> String {
        let t = self.stable_text();
        self.kc
            .git(false, &["-C", &t, "config", "--local", "--list"])
            .map(|l| foreign_key_of(&l))
            .unwrap_or_default()
    }

    /// Nothing a job could have planted in `.git` is left to act: hooks (also never run: core.hooksPath), attributes, excludes, replace
    /// objects.
    fn sanitize(&self) -> Result<(), String> {
        let dotgit = self.stable.join(".git");
        self.kc.remove(&dotgit, &dotgit.join("hooks"))?;
        std::fs::create_dir_all(dotgit.join("hooks")).map_err(|e| e.to_string())?;
        self.kc.remove(&dotgit, &dotgit.join("info").join("attributes"))?;
        self.kc.remove(&dotgit, &dotgit.join("info").join("exclude"))?;
        self.kc.remove(&dotgit, &dotgit.join("refs").join("replace"))
    }

    /// A job's `.angular/cache` is a link to a kept folder outside the clone; each link is removed by itself before a clean.
    fn unlink_caches(&self) {
        for l in angular_cache_links(&self.stable) {
            // `remove_file` on a symlink, `remove_dir` on a junction (removes only the junction).
            if std::fs::remove_file(&l).or_else(|_| std::fs::remove_dir(&l)).is_err() {
                self.kc.note(format!("{NAME}: could not remove the link {}", l.display()));
            }
        }
    }

    /// Makes the clone: no checkout, no tags, depth 1, no blobs; the marker says this step made it.
    fn create(&self, step: &mut Step<'_>, err: &mut dyn Write) -> Result<(), Soft> {
        let (stable, url) = (self.stable.clone(), self.url.clone());
        let flow = Flow {
            kc: self.kc.clone(),
            ws: self.ws.clone(),
            ws_text: self.ws_text.clone(),
            stable: self.stable.clone(),
            sha: self.sha.clone(),
            url: self.url.clone(),
            excludes: self.excludes.clone(),
            limit: self.limit,
            hint: self.hint.clone(),
        };
        let r = self.phase(step, PhaseId::Create, "create the clone", move |kc| {
            flow.unlink_caches();
            let parent = stable.parent().ok_or("the kept folder has no parent")?.to_path_buf();
            kc.remove(&parent, &stable)?;
            std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
            let s = stable.display().to_string();
            kc.git(
                true,
                &[
                    "clone",
                    "-q",
                    "--no-checkout",
                    "--no-tags",
                    "--depth=1",
                    "--filter=blob:none",
                    &url,
                    &s,
                ],
            )
            .map_err(|e| e.to_string())?;
            std::fs::write(stable.join(".git").join("paddock-kept-clone"), "1\n").map_err(|e| e.to_string())
        });
        if r.is_err() {
            let _ = write!(err, "{}", self.kc.err.lock().map(|e| e.clone()).unwrap_or_default());
        }
        r
    }

    /// Clean (a reused clone), fetch, check out, verify; each phase bounded. `true` when the clone is at the commit.
    fn bring_up(&self, step: &mut Step<'_>, created: bool) -> bool {
        let t = self.stable_text();
        let make = |f: fn(&Flow) -> Result<(), String>| {
            let flow = Flow {
                kc: self.kc.clone(),
                ws: self.ws.clone(),
                ws_text: self.ws_text.clone(),
                stable: self.stable.clone(),
                sha: self.sha.clone(),
                url: self.url.clone(),
                excludes: self.excludes.clone(),
                limit: self.limit,
                hint: self.hint.clone(),
            };
            move |_: &Kc| f(&flow)
        };
        // A reused clone is cleaned first; a clone just created has nothing to clean (its patterns come first: before them, a reset
        // would write the whole tree of the clone's branch, run 7808).
        if !created && self.phase(step, PhaseId::Clean, "clean", make(Flow::clean)).is_err() {
            return false;
        }
        let _ = &t;
        self.phase(step, PhaseId::Fetch, &format!("fetch {}", self.sha), make(Flow::fetch))
            .is_ok()
            && self.phase(step, PhaseId::Checkout, "check out", make(Flow::checkout)).is_ok()
            && self.phase(step, PhaseId::Verify, "verify", make(Flow::verify)).is_ok()
    }

    /// The shell's `clean_clone` is not a chain: `set -e` does not act inside a function that runs under `||`, so only the status of its
    /// last command, `git clean`, is the phase's. The steps before it note their failure and go on (a reset that fails on a lock is then
    /// found by the check-out, which names it); the port keeps that, so a clone is not made again for what a later step repairs.
    fn clean(&self) -> Result<(), String> {
        let t = self.stable_text();
        if let Err(e) = self.sanitize() {
            self.kc.note(format!("{NAME}: {e}"));
        }
        self.unlink_caches();
        if let Err(e) = self.apply_profile(&self.stable) {
            self.kc.note(format!("{NAME}: {e}"));
        }
        if self.kc.git(false, &["-C", &t, "rev-parse", "--verify", "-q", "HEAD"]).is_ok() {
            // A failure is named in the phase's error output by the command's own line.
            let _ = self.kc.git(true, &["-C", &t, "reset", "-q", "--hard", "HEAD"]);
        }
        // conformance: scoped-cleanup — the kept clone is this step's own folder (a slot's src/<name>-<slot>): nothing else lives in it, and a clean of the whole clone is its job
        self.kc.git(true, &["-C", &t, "clean", "-q", "-ffdx"]).map(|_| ())
    }

    fn fetch(&self) -> Result<(), String> {
        let t = self.stable_text();
        self.kc.git(
            true,
            &[
                "-C",
                &t,
                "fetch",
                "-q",
                "--depth=1",
                "--no-tags",
                "--filter=blob:none",
                "origin",
                &self.sha,
            ],
        )?;
        let head = self.kc.git(true, &["-C", &t, "rev-parse", "FETCH_HEAD"])?;
        if head == self.sha {
            Ok(())
        } else {
            Err(format!("FETCH_HEAD is {head}"))
        }
    }

    fn checkout(&self) -> Result<(), String> {
        let t = self.stable_text();
        self.sanitize()?;
        self.apply_profile(&self.stable)?;
        self.kc.git(
            true,
            &[
                "-C",
                &t,
                "-c",
                "advice.detachedHead=false",
                "checkout",
                "-q",
                "-f",
                "--detach",
                &self.sha,
            ],
        )?;
        self.kc.git(true, &["-C", &t, "read-tree", "-mu", "HEAD"])?;
        // conformance: scoped-cleanup — the kept clone is this step's own folder (a slot's src/<name>-<slot>): nothing else lives in it, and a clean of the whole clone is its job
        self.kc.git(true, &["-C", &t, "clean", "-q", "-ffdx"])?;
        // git clean leaves untracked files outside the sparse patterns alone (git 2.34+), so a file a job wrote into an excluded folder
        // would stay for ever: the excluded paths are removed by name.
        for p in &self.excludes {
            for d in glob_expand(&self.stable, p) {
                self.kc.remove(&self.stable, &d)?;
            }
        }
        Ok(())
    }

    fn verify(&self) -> Result<(), String> {
        let t = self.stable_text();
        let fail = |msg: String| {
            self.kc.note(&msg);
            Err(msg)
        };
        let bad = self.foreign_key();
        if !bad.is_empty() {
            return fail(format!("the local git config key '{bad}' is not one this script writes"));
        }
        let head = self.kc.git(true, &["-C", &t, "rev-parse", "HEAD"])?;
        if head != self.sha {
            return fail(format!("HEAD is {head}, not the job's commit {}", self.sha));
        }
        let status = self.kc.git(true, &["-C", &t, "status", "--porcelain", "--untracked-files=all"])?;
        if !status.is_empty() {
            let first: Vec<&str> = status.lines().take(3).collect();
            return fail(format!("the tree is not clean: {} ", first.join(" ")));
        }
        for p in &self.excludes {
            if !glob_expand(&self.stable, p).is_empty() {
                return fail(format!("excluded path {p} exists"));
            }
        }
        Ok(())
    }

    /// Stale `.git` locks (an earlier job's git that died), at most three levels deep.
    fn remove_locks(&self) {
        fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                let Ok(m) = std::fs::symlink_metadata(&p) else { continue };
                if m.is_file() && p.extension().is_some_and(|x| x == "lock") {
                    out.push(p);
                } else if m.is_dir() && !m.file_type().is_symlink() && depth < 3 {
                    walk(&p, depth + 1, out);
                }
            }
        }
        let dotgit = self.stable.join(".git");
        let mut locks = Vec::new();
        walk(&dotgit, 1, &mut locks);
        for l in locks {
            let _ = self.kc.remove(&dotgit, &l);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [PhaseId; 7] = [
        PhaseId::Resolve,
        PhaseId::Create,
        PhaseId::Clean,
        PhaseId::Fetch,
        PhaseId::Checkout,
        PhaseId::Verify,
        PhaseId::Link,
    ];

    #[test]
    fn each_phase_reads_only_its_own_limit_and_falls_back_to_the_general_one() {
        let general = Duration::from_secs(900);
        for id in ALL {
            let env = HashMap::from([(id.var().to_string(), "7".to_string())]);
            for other in ALL {
                let want = if other == id { Duration::from_secs(7) } else { general };
                assert_eq!(phase_limit(&env, other, general), want, "{id:?} set, {other:?} read");
            }
        }
        // As the shell's `^[1-9][0-9]*$`: anything else is the general limit.
        for bad in ["", "0", "07", "-1", "+5", "1.5", " 5", "x"] {
            let env = HashMap::from([(PhaseId::Fetch.var().to_string(), bad.to_string())]);
            assert_eq!(phase_limit(&env, PhaseId::Fetch, general), general, "{bad:?}");
        }
    }
}
