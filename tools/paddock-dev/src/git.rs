//! Running git for the rest of paddock-dev (core layer). [`Git`] is the bounded reader of the merge check: a failure names the
//! command and git's own words (ER-27), so a caller can turn it into "not measured" (ADR 0089) instead of an empty answer;
//! [`git_text`] is the planner's lighter one (no answer on any failure) and [`state_dir`] finds the folder of a checkout's gate logs.

use paddock_proc::{Limits, run_bounded};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The longest one git call may run, in seconds.
pub const TIMEOUT_SECS: u64 = 120;
const TIMEOUT: Duration = Duration::from_secs(TIMEOUT_SECS);
/// The most any read may print (256 MiB): the larger of the two caps the readers had (the merge check's 256 MiB and the worktree
/// reader's 64 MiB), so a read that fitted one of them still fits. A longer read is an error, never a cut answer.
const MAX_OUTPUT: usize = 1 << 28;

/// A git command that failed, with the words that explain it.
#[derive(Debug)]
pub struct GitError {
    /// The git arguments (never secrets: only paths, refs and flags).
    pub args: String,
    /// What git printed on standard error (or why it could not run).
    pub stderr: String,
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`git {}` failed: {}", self.args, self.stderr.trim())
    }
}

impl GitError {
    /// Git's own words, when it printed any: `--quiet` questions answer "no" with exit status 1 and nothing on standard error, while
    /// a git that could not run or open the repository says why (ER-27), and a caller that turns the error into its own message keeps it.
    pub fn cause(&self) -> Option<&str> {
        Some(self.stderr.trim()).filter(|s| !s.is_empty())
    }

    /// `message`, with git's words after it in parentheses when it printed any.
    pub fn explain(&self, message: &str) -> String {
        match self.cause() {
            Some(c) => format!("{message} ({c})"),
            None => message.to_string(),
        }
    }
}

impl From<GitError> for String {
    fn from(e: GitError) -> String {
        e.to_string()
    }
}

/// What a git run produced, whatever its status.
pub struct Out {
    /// Exit status 0.
    pub ok: bool,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

/// What git's own merge of two parents gives.
#[derive(Debug, Default)]
pub struct AutoMerge {
    /// Every file of the tree it wrote, with its blob id.
    pub blobs: HashMap<String, String>,
    /// The paths it lists as conflicted: never evidence of a clean merge, whatever their blob is.
    pub conflicted: std::collections::HashSet<String>,
}

/// A path git's rename detection says is gone from where it was: renamed (with the path it has now) or deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Moved {
    /// The file now lives at `to` (`git diff -M`: at least half of it is unchanged).
    Renamed {
        /// The path before.
        from: String,
        /// The path after.
        to: String,
    },
    /// The file is gone (or was rewritten so much that git sees a deletion and an addition).
    Deleted(String),
}

impl Moved {
    /// The path the change took away.
    pub fn old(&self) -> &str {
        match self {
            Moved::Renamed { from, .. } => from,
            Moved::Deleted(p) => p,
        }
    }
}

/// Parses `git diff --name-status -z --diff-filter=DR` (the one parser of that output, ER-42): `D NUL path NUL` and
/// `R<score> NUL from NUL to NUL`. Any other status, a status with no path or text that is not UTF-8 is an error, never skipped:
/// a half-read list would pass what it did not see.
pub fn parse_moves(bytes: &[u8]) -> Result<Vec<Moved>, String> {
    let mut parts = bytes.split(|b| *b == 0).filter(|p| !p.is_empty());
    let mut out = Vec::new();
    let text = |p: &[u8]| String::from_utf8(p.to_vec()).map_err(|_| format!("a path that is not UTF-8: {:?}", String::from_utf8_lossy(p)));
    while let Some(status) = parts.next() {
        let status = text(status)?;
        let mut path = || {
            parts
                .next()
                .ok_or_else(|| format!("the status {status} has no path"))
                .and_then(text)
        };
        match status.chars().next() {
            Some('D') => out.push(Moved::Deleted(path()?)),
            Some('R') => {
                let from = path()?;
                let to = path()?;
                out.push(Moved::Renamed { from, to });
            }
            _ => return Err(format!("the status {status} is neither D nor R")),
        }
    }
    Ok(out)
}

/// A repository folder to run git in.
pub struct Git<'a> {
    root: Cow<'a, Path>,
    max_output: usize,
    /// Whether git may take its optional locks (`status` refreshing the index); a reader of a checkout other work uses turns it off.
    optional_locks: bool,
    /// Whether the child keeps the caller's environment and git configuration (a lookup that leaves the machine needs the user's
    /// own credentials); the default removes both ([`crate::child_env::scrub`]).
    own_environment: bool,
    /// Texts already read, by full object id and path: the detectors read the same file at the same revision more than once.
    texts: RefCell<HashMap<(String, String), Option<String>>>,
    /// Answers of the reads [`cacheable`] allows, by arguments: they depend only on immutable objects.
    memo: RefCell<HashMap<MemoKey, Vec<u8>>>,
    /// How many git processes this reader started (tests read it).
    starts: Cell<usize>,
    /// Blobs and trees through one shared reader (ADR 0108 G2: in process with gix, else one `cat-file` process), made on first use.
    objects: RefCell<Option<paddock_git::Reader>>,
    /// Tree listings already walked, by full object id.
    walks: RefCell<HashMap<String, Listing>>,
}

/// A tree's files by path, each with its mode and object id (shared between the reads of one check).
type Listing = std::rc::Rc<HashMap<String, (paddock_git::Mode, String)>>;

/// A remembered read: its arguments, and whether exit status 1 was an answer.
type MemoKey = (Vec<String>, bool);

/// The read commands whose answer depends only on the objects their full ids name, when every revision they name is one.
const IMMUTABLE_READS: &[&str] = &["ls-tree", "diff", "merge-base", "merge-tree", "log", "cat-file"];

/// The options that make a read select refs (where branches, tags or the reflog point now), whatever ids it also names.
const REF_SELECTING: &[&str] = &[
    "--all",
    "--branches",
    "--tags",
    "--remotes",
    "--glob",
    "--exclude",
    "--reflog",
    "--stdin",
    "--alternate-refs",
    "--bisect",
];

/// A full object id (lower-case hex, SHA-1 or SHA-256 length): it names one immutable object.
fn is_full_id(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Whether `git <args>` may be answered from memory within one check: a command of [`IMMUTABLE_READS`] whose revisions are all
/// full object ids (`<id>`, `^<id>`, `<id>..<id>`, `<id>^{commit}`, `<id>:<path>`) and that selects no refs ([`REF_SELECTING`]),
/// or `rev-parse --is-shallow-repository` (a property of the clone, not of a ref). A ref, `HEAD` or a short id can move or become
/// ambiguous, so a read that names one runs git again.
pub(crate) fn cacheable(args: &[&str]) -> bool {
    if args == ["rev-parse", "--is-shallow-repository"] {
        return true;
    }
    let Some((cmd, rest)) = args.split_first() else {
        return false;
    };
    if !IMMUTABLE_READS.contains(cmd) {
        return false;
    }
    let is_id = is_full_id;
    let is_rev = |r: &str| {
        let r = r.strip_prefix('^').unwrap_or(r);
        let r = r.split_once(':').map_or(r, |(rev, _path)| rev);
        let r = r.split_once("^{").map_or(r, |(rev, _peel)| rev);
        is_id(r)
    };
    let mut saw_rev = false;
    for a in rest.iter().take_while(|a| **a != "--") {
        let option = a.split_once('=').map_or(*a, |(name, _)| name);
        if REF_SELECTING.contains(&option) {
            return false;
        }
        if a.starts_with('-') || a.bytes().all(|b| b.is_ascii_digit()) || (*cmd == "cat-file" && matches!(*a, "blob" | "tree" | "commit")) {
            continue;
        }
        let parts: Vec<&str> = if let Some((x, y)) = a.split_once("...").or_else(|| a.split_once("..")) {
            vec![x, y]
        } else {
            vec![a]
        };
        if !parts.iter().all(|p| is_rev(p)) {
            return false;
        }
        saw_rev = true;
    }
    saw_rev
}

impl<'a> Git<'a> {
    /// Reads the repository at `root`.
    pub fn new(root: &'a Path) -> Git<'a> {
        Git::at(Cow::Borrowed(root))
    }

    /// Reads the repository at `root`, which the reader owns (one that outlives the caller's path).
    pub fn owned(root: &Path) -> Git<'static> {
        Git::at(Cow::Owned(root.to_path_buf()))
    }

    fn at(root: Cow<'a, Path>) -> Git<'a> {
        Git {
            root,
            max_output: MAX_OUTPUT,
            optional_locks: true,
            own_environment: false,
            texts: RefCell::new(HashMap::new()),
            memo: RefCell::new(HashMap::new()),
            starts: Cell::new(0),
            objects: RefCell::new(None),
            walks: RefCell::new(HashMap::new()),
        }
    }

    /// The same reader, but git never takes its optional locks: a reader must not collide with the work it reads.
    pub fn without_optional_locks(mut self) -> Git<'a> {
        self.optional_locks = false;
        self
    }

    /// The same reader, but the child keeps the caller's environment and git configuration: only for a command that leaves the
    /// machine (`ls-remote`) with the user's own credentials.
    pub fn with_own_environment(mut self) -> Git<'a> {
        self.own_environment = true;
        self
    }

    /// How many git processes this reader has started (its object reader's included).
    pub fn starts(&self) -> usize {
        self.starts.get() + self.objects.borrow().as_ref().map_or(0, paddock_git::Reader::starts)
    }

    /// Runs `f` on the object reader, made on first use with this reader's git policy (the same `-c` settings and scrubbed
    /// environment as every other call).
    fn with_objects<T>(&self, f: impl FnOnce(&mut paddock_git::Reader) -> T) -> T {
        let mut slot = self.objects.borrow_mut();
        let reader = slot.get_or_insert_with(|| {
            let root = self.root.to_path_buf();
            let (locks, own) = (self.optional_locks, self.own_environment);
            let parent: Vec<String> = std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()).collect();
            paddock_git::Reader::open(&self.root, move || git_command(&root, &[], &parent, locks, own))
        });
        f(reader)
    }

    /// The same reader with another output cap (a read longer than it is an error, never a cut answer).
    pub fn with_max_output(mut self, bytes: usize) -> Git<'a> {
        self.max_output = bytes;
        self
    }

    /// Standard output of `git -C root <args>` as bytes; an error names the command and git's last words.
    pub fn bytes(&self, args: &[&str]) -> Result<Vec<u8>, GitError> {
        self.run(args, false)
    }

    /// Like [`Git::bytes`], but exit status 1 is an answer too (`git grep` with no match prints nothing and exits 1).
    pub fn bytes_or_none(&self, args: &[&str]) -> Result<Vec<u8>, GitError> {
        self.run(args, true)
    }

    /// Git's own merge of two commits (`git merge-tree --write-tree`): the files of the tree it writes, and the paths it lists
    /// as conflicted (exit status 1 is an answer here). A conflicted path's blob in the tree is not a merge result: for a text
    /// file it holds conflict markers, for a binary file it is parent 1's version.
    pub fn auto_merge(&self, a: &str, b: &str) -> Result<AutoMerge, String> {
        let out = self.run(&["merge-tree", "--write-tree", "-z", "--no-messages", a, b], true)?;
        let text = String::from_utf8_lossy(&out).into_owned();
        let mut tokens = text.split('\0');
        let tree = tokens
            .next()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or("git merge-tree printed no tree")?
            .to_string();
        // "<mode> <oid> <stage>\t<path>" for every stage of every conflicted path, up to the first empty token.
        let conflicted = tokens
            .take_while(|t| !t.is_empty())
            .filter_map(|t| t.split_once('\t').map(|(_, path)| path.to_string()))
            .collect();
        Ok(AutoMerge {
            blobs: self.blob_ids(&tree)?,
            conflicted,
        })
    }

    /// The git child of one call: stable output, hooks off, and none of the queue's credentials (`parent`: the names of this
    /// process's environment) - the merge check reads a merged tree that holds pull request code.
    pub(crate) fn command_for(&self, args: &[&str], parent: &[String]) -> std::process::Command {
        git_command(&self.root, args, parent, self.optional_locks, self.own_environment)
    }

    fn run(&self, args: &[&str], status_1_is_empty: bool) -> Result<Vec<u8>, GitError> {
        let key = cacheable(args).then(|| (args.iter().map(|a| a.to_string()).collect::<Vec<_>>(), status_1_is_empty));
        if let Some(hit) = key.as_ref().and_then(|k| self.memo.borrow().get(k).cloned()) {
            return Ok(hit);
        }
        let out = self.run_git(args, status_1_is_empty)?;
        if let Some(k) = key {
            self.memo.borrow_mut().insert(k, out.clone());
        }
        Ok(out)
    }

    /// Runs git once: what it printed whatever its exit status, or why it could not run, did not finish or wrote too much.
    fn exec(&self, args: &[&str]) -> Result<paddock_proc::Output, GitError> {
        self.starts.set(self.starts.get() + 1);
        let parent: Vec<String> = std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()).collect();
        let cmd = self.command_for(args, &parent);
        let fail = |why: String| GitError {
            args: args.join(" "),
            stderr: why,
        };
        let out = run_bounded(cmd, Limits::new(TIMEOUT).max_output(self.max_output)).map_err(|e| fail(format!("could not run: {e}")))?;
        if out.truncated {
            return Err(fail(format!("wrote more than {} bytes", self.max_output)));
        }
        if !matches!(out.outcome, paddock_proc::Outcome::Exited(_)) {
            return Err(fail(format!("did not finish ({:?})", out.outcome)));
        }
        Ok(out)
    }

    fn run_git(&self, args: &[&str], status_1_is_empty: bool) -> Result<Vec<u8>, GitError> {
        let out = self.exec(args)?;
        if status_1_is_empty && matches!(out.outcome, paddock_proc::Outcome::Exited(st) if st.code() == Some(1)) {
            return Ok(out.stdout);
        }
        if !out.success() {
            let err = out.stderr_text();
            let tail: String = err.trim().chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
            return Err(GitError {
                args: args.join(" "),
                stderr: tail,
            });
        }
        Ok(out.stdout)
    }

    /// Runs git and returns what it printed whatever its exit status (an error only when it did not run or finish).
    pub fn output(&self, args: &[&str]) -> Result<Out, GitError> {
        let out = self.exec(args)?;
        Ok(Out {
            ok: out.success(),
            stdout: out.stdout_text(),
            stderr: out.stderr_text(),
        })
    }

    /// Runs git for a yes/no question (`rev-parse --verify --quiet`): exit 0 is true, any other status false.
    pub fn succeeds(&self, args: &[&str]) -> Result<bool, GitError> {
        Ok(self.exec(args)?.success())
    }

    /// Standard output as text (lossy UTF-8).
    pub fn text(&self, args: &[&str]) -> Result<String, GitError> {
        self.bytes(args).map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// The NUL-separated paths `args` print. A path that is not UTF-8 is an error: decoded lossily, two such paths would become one.
    pub fn paths(&self, args: &[&str]) -> Result<Vec<String>, GitError> {
        self.bytes(args)?
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| {
                String::from_utf8(p.to_vec()).map_err(|_| GitError {
                    args: args.join(" "),
                    stderr: format!("printed the path {:?}, which is not UTF-8", String::from_utf8_lossy(p)),
                })
            })
            .collect()
    }

    /// The files git's rename detection (`-M`) says were renamed or deleted between `revs` (a merge base, or a base and a tip; with
    /// one revision the working tree is the other side), one `--name-status` read parsed by [`parse_moves`].
    pub fn moves(&self, revs: &[&str]) -> Result<Vec<Moved>, GitError> {
        let mut args = vec!["diff", "--name-status", "-M", "-z", "--diff-filter=DR", "--no-ext-diff"];
        args.extend(revs);
        let bytes = self.bytes(&args)?;
        parse_moves(&bytes).map_err(|stderr| GitError {
            args: args.join(" "),
            stderr,
        })
    }

    /// The paths that differ between two tree-ish revisions (no rename detection: both names of a rename are listed).
    pub fn changed_paths(&self, from: &str, to: &str) -> Result<Vec<String>, String> {
        // In process from the two trees' listings (a path whose mode or object differs, or that only one side has), as `git diff
        // --name-only --no-renames` lists them, in git's byte order; through git when a revision is not readable that way.
        if let (Some(a), Some(b)) = (self.entries(from), self.entries(to)) {
            let mut out: Vec<String> = a
                .iter()
                .filter(|(p, e)| b.get(*p) != Some(*e))
                .map(|(p, _)| p.clone())
                .chain(b.keys().filter(|p| !a.contains_key(*p)).cloned())
                .collect();
            out.sort_unstable();
            return Ok(out);
        }
        self.paths(&["diff", "--name-only", "-z", "--no-renames", from, to])
            .map_err(String::from)
    }

    /// Every file of the tree `rev` names, by path, with its canonical mode and object id; `None` when the reader cannot list it
    /// (a name that is not UTF-8 among them). Remembered for a full object id only: a ref can move between two reads.
    fn entries(&self, rev: &str) -> Option<Listing> {
        if let Some(hit) = self.walks.borrow().get(rev) {
            return Some(hit.clone());
        }
        let spec = format!("{rev}^{{tree}}");
        let entries = self.with_objects(|r| r.walk(&spec)).ok()??;
        let map: Listing = std::rc::Rc::new(entries.into_iter().map(|e| (e.path, (e.mode, e.oid))).collect());
        if is_full_id(rev) {
            self.walks.borrow_mut().insert(rev.to_string(), map.clone());
        }
        Some(map)
    }

    /// Whether `ancestor` is reachable from `of` (`git merge-base --is-ancestor`: exit 0 yes, 1 no, anything else an error).
    pub fn is_ancestor(&self, ancestor: &str, of: &str) -> Result<bool, String> {
        let args = ["merge-base", "--is-ancestor", ancestor, of];
        self.starts.set(self.starts.get() + 1);
        let parent: Vec<String> = std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()).collect();
        let out = run_bounded(self.command_for(&args, &parent), Limits::new(TIMEOUT).max_output(1 << 16))
            .map_err(|e| format!("git {} could not run: {e}", args.join(" ")))?;
        match out.outcome {
            paddock_proc::Outcome::Exited(st) if st.code() == Some(0) => Ok(true),
            paddock_proc::Outcome::Exited(st) if st.code() == Some(1) => Ok(false),
            _ => Err(format!(
                "git {} failed ({:?}): {}",
                args.join(" "),
                out.outcome,
                out.stderr_text().trim()
            )),
        }
    }

    /// Whether this clone is shallow (its history is cut, so parents may be missing).
    pub fn is_shallow(&self) -> Result<bool, String> {
        Ok(self.text(&["rev-parse", "--is-shallow-repository"])?.trim() == "true")
    }

    /// Every file of `rev` with its blob id.
    pub fn blob_ids(&self, rev: &str) -> Result<HashMap<String, String>, String> {
        match self.entries(rev) {
            Some(m) => Ok(m.iter().map(|(p, (_, oid))| (p.clone(), oid.clone())).collect()),
            None => {
                let spec = format!("{rev}^{{tree}}");
                match self.with_objects(|r| r.walk(&spec)) {
                    Ok(_) => Err(format!("git ls-tree -r {rev}: no such revision")),
                    Err(e) => Err(format!("git ls-tree -r {rev}: {e}")),
                }
            }
        }
    }

    /// Whether `rev` names a commit this clone has.
    pub fn has_commit(&self, rev: &str) -> Result<bool, String> {
        let spec = format!("{rev}^{{commit}}");
        self.with_objects(|r| r.info(&spec))
            .map(|i| i.is_some_and(|i| i.kind == paddock_git::Kind::Commit))
            .map_err(|e| format!("git cat-file -e {spec}: {e}"))
    }

    /// The bytes of `path` at `rev` (a commit or tree), `None` when the revision has no such file.
    pub fn file_at(&self, rev: &str, path: &str) -> Result<Option<Vec<u8>>, String> {
        self.with_objects(|r| r.file_at(rev, path))
            .map_err(|e| format!("git cat-file blob {rev}:{path}: {e}"))
    }

    /// The file as UTF-8 text (read once per path for a full object id; a ref can move, so a ref name reads again); a file that is
    /// not UTF-8 is an error (it cannot be measured).
    pub fn text_at(&self, rev: &str, path: &str) -> Result<Option<String>, String> {
        let key = (rev.to_string(), path.to_string());
        if let Some(hit) = self.texts.borrow().get(&key) {
            return Ok(hit.clone());
        }
        let text = match self.file_at(rev, path)? {
            None => None,
            Some(b) => Some(String::from_utf8(b).map_err(|_| format!("{path} at {} is not UTF-8", short(rev)))?),
        };
        if is_full_id(rev) {
            self.texts.borrow_mut().insert(key, text.clone());
        }
        Ok(text)
    }
}

/// The folder `safe.directory` names for a read of `root`: its resolved path in the spelling a program takes (git compares resolved
/// paths, and the verbatim `\\?\` spelling of Windows is not one it matches); `root` as given when it cannot be resolved. The scrubbed child reads no global or system configuration,
/// where a CI image names `safe.directory` for a checkout another user owns; the folder it was asked to read is trusted here instead,
/// on the command line (the command scope is protected configuration, git-config(1) "safe.directory"), and no other folder is.
fn trusted(root: &Path) -> String {
    paddock_env::winpath::real_for_tools(root).unwrap_or_else(|_| root.display().to_string())
}

/// The git child of one merge-check call (see [`Git::command_for`]).
fn git_command(root: &Path, args: &[&str], parent: &[String], optional_locks: bool, own_environment: bool) -> std::process::Command {
    let mut cmd = crate::git_env::git();
    // Stable, parseable output whatever the user's git config says.
    cmd.arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.quotePath=false",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "log.showSignature=false",
        ])
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(["-c", &format!("safe.directory={}", trusted(root))])
        .args(args);
    if !own_environment {
        crate::child_env::scrub(&mut cmd, parent);
    }
    if !optional_locks {
        cmd.env("GIT_OPTIONAL_LOCKS", "0");
    }
    cmd
}

/// Runs `git -C root args` through [`Git`] and returns standard output when git succeeded, `None` on any failure (for a caller whose
/// answer to "git could not tell" is the same as to "no").
pub fn git_text(root: &Path, args: &[&str]) -> Option<String> {
    Git::new(root).text(args).ok()
}

/// The folder for this checkout's gate logs and override log: `<git common dir>/paddock-pre-push`.
///
/// `PADDOCK_PREPUSH_STATE` replaces it (a test's own folder, so it never waits for a build-folder lock a real gate of this checkout holds,
/// ER-14, ER-51); the run, the test cache and the slow-row records all read it through here.
pub fn state_dir(root: &Path) -> PathBuf {
    if let Some(own) = state_override("PADDOCK_PREPUSH_STATE") {
        return own;
    }
    git_text(root, &["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .map(|t| PathBuf::from(t.trim()))
        .unwrap_or_else(|| root.join(".git"))
        .join("paddock-pre-push")
}

/// The folder a state-folder setting names (`PADDOCK_PREPUSH_STATE`, `PADDOCK_WAIT_STATE`: test-only rows of the settings register), when
/// it is set and not empty. A tool's state otherwise lives under the git common dir, which every worktree shares; the test harness
/// sets every one of these to the test's own folder (`paddock_testkit::process::STATE_FOLDERS`, ER-14, ER-51). In the core layer so
/// [`state_dir`] and the wait lock read it.
pub fn state_override(name: &str) -> Option<PathBuf> {
    paddock_env::vars()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The first nine characters of a revision name or hash, for messages.
pub fn short(rev: &str) -> &str {
    rev.get(..9).unwrap_or(rev)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder that is a git repository (no commit needed).
    fn empty_repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let out = crate::git_env::git()
            .arg("-C")
            .arg(d.path())
            .arg("init")
            .arg("-q")
            .output()
            .unwrap();
        assert!(out.status.success());
        d
    }

    /// The hardened child reads no global or system configuration, which is where a CI image names `safe.directory` for a checkout
    /// another user owns ("dubious ownership"); the folder it reads is trusted on its own command line instead (git honours
    /// `safe.directory` from the command scope: git-config(1), "protected configuration").
    #[test]
    fn the_child_trusts_the_folder_it_reads_on_its_own_command_line() {
        let d = empty_repo();
        let git = Git::new(d.path());
        let cmd = git.command_for(&["status"], &[]);
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        let want = format!("safe.directory={}", paddock_env::winpath::real_for_tools(d.path()).unwrap());
        assert!(args.windows(2).any(|w| w[0] == "-c" && w[1] == want), "{args:?}");
        assert!(git.text(&["rev-parse", "--is-inside-work-tree"]).unwrap().starts_with("true"));
    }

    #[test]
    fn git_text_runs_with_hooks_off_and_unquoted_paths_like_every_other_reader() {
        let d = empty_repo();
        assert_eq!(
            git_text(d.path(), &["config", "--get", "core.hooksPath"]).as_deref(),
            Some("/dev/null\n")
        );
        assert_eq!(
            git_text(d.path(), &["config", "--get", "core.quotePath"]).as_deref(),
            Some("false\n")
        );
    }

    /// A repository with one commit; its full id.
    fn one_commit() -> (tempfile::TempDir, String) {
        let d = tempfile::tempdir().unwrap();
        let git = |a: &[&str]| {
            let out = crate::git_env::git().arg("-C").arg(d.path()).args(a).output().unwrap();
            assert!(out.status.success(), "git {a:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(d.path().join("a.txt"), "1\n").unwrap();
        git(&["add", "a.txt"]);
        git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "one",
        ]);
        let sha = git(&["rev-parse", "HEAD"]);
        (d, sha)
    }

    /// A read whose answer depends only on object ids (a full hash, no ref or `HEAD`) runs git once per check: the detectors asked
    /// the same `ls-tree`, `diff`, `merge-base` and `log` twice each, 33 git starts for a two-commit merge (about 60 ms each on the
    /// Windows runner). A read that names a ref, and a failed read, run again.
    #[test]
    fn a_read_of_immutable_objects_runs_git_once_and_a_ref_or_a_failure_runs_it_again() {
        let (d, sha) = one_commit();
        let g = Git::new(d.path());
        let first = g.text(&["ls-tree", "-r", "-z", &sha]).unwrap();
        assert_eq!(g.text(&["ls-tree", "-r", "-z", &sha]).unwrap(), first);
        assert_eq!(g.starts(), 1, "the same read of a full id is answered from memory");
        assert!(g.is_shallow().is_ok() && g.is_shallow().is_ok());
        assert_eq!(g.starts(), 2, "whether the clone is shallow is asked once");
        g.text(&["ls-tree", "-r", "-z", "HEAD"]).unwrap();
        g.text(&["ls-tree", "-r", "-z", "HEAD"]).unwrap();
        assert_eq!(g.starts(), 4, "a ref can move: asked each time");
        let missing = "0".repeat(40);
        assert!(g.text(&["ls-tree", "-r", "-z", &missing]).is_err());
        assert!(g.text(&["ls-tree", "-r", "-z", &missing]).is_err());
        assert_eq!(g.starts(), 6, "a failure is never remembered");
        // Positive control of the rule itself.
        assert!(cacheable(&["diff", "--name-only", "-z", &sha, &sha]));
        assert!(cacheable(&["log", "--format=%H", &format!("{sha}..{sha}")]));
        assert!(cacheable(&["cat-file", "blob", &format!("{sha}:a.txt")]));
        assert!(!cacheable(&["diff", "--name-only", "main", &sha]));
        assert!(!cacheable(&["rev-parse", "--verify", "HEAD"]));
        assert!(!cacheable(&["show", "-s", &sha]), "a command off the list");
    }

    /// A read that selects refs (`--all`, `--branches`, `--glob=...`) answers from where the refs point now, so it is never
    /// remembered, even when it also names a full id.
    #[test]
    fn a_read_that_selects_refs_is_never_remembered() {
        let sha = "a".repeat(40);
        for opt in [
            "--all",
            "--branches",
            "--tags",
            "--remotes",
            "--glob=refs/heads/*",
            "--exclude=x",
            "--reflog",
            "--stdin",
        ] {
            assert!(!cacheable(&["log", "--format=%H", opt, &sha]), "{opt}");
        }
        assert!(
            cacheable(&["log", "--format=%H", "--first-parent", &sha]),
            "an option that selects no refs keeps it cacheable"
        );
    }

    /// A ref can move between two reads of one check: a file or a listing read by a ref name is read again; one read by a full
    /// id is remembered.
    #[test]
    fn a_file_or_listing_read_by_a_ref_name_is_read_again_after_the_ref_moves() {
        let t = Trees::new();
        let sha = t.commit(&t.tree(&[("100644", "a.txt", &t.blob("1\n"))]), "");
        t.git(&["update-ref", "refs/heads/main", &sha]);
        let g = Git::new(t.dir.path());
        assert_eq!(g.text_at("main", "a.txt").unwrap().as_deref(), Some("1\n"));
        assert!(g.changed_paths(&sha, "main").unwrap().is_empty());
        let second = t.commit(&t.tree(&[("100644", "a.txt", &t.blob("2\n"))]), &sha);
        t.git(&["update-ref", "refs/heads/main", &second]);
        assert_eq!(g.text_at("main", "a.txt").unwrap().as_deref(), Some("2\n"), "main moved");
        assert_eq!(g.changed_paths(&sha, "main").unwrap(), ["a.txt"], "main moved");
        assert_eq!(g.text_at(&sha, "a.txt").unwrap().as_deref(), Some("1\n"));
    }

    /// A repository whose trees are made with the index plumbing, never the file system: a symlink, a submodule and two names
    /// that differ only in case are the same on every OS.
    struct Trees {
        dir: tempfile::TempDir,
    }

    impl Trees {
        fn new() -> Trees {
            let t = Trees {
                dir: tempfile::tempdir().unwrap(),
            };
            t.git(&["init", "-q", "-b", "main"]);
            t.git(&["config", "core.symlinks", "false"]);
            t
        }

        fn run(&self, dir: &Path, args: &[&str], input: &[u8]) -> Vec<u8> {
            use std::io::Write;
            let mut c = crate::git_env::git();
            c.arg("-C").arg(dir).args(args);
            c.stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let mut child = c.spawn().unwrap();
            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(input).unwrap();
            drop(stdin);
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            out.stdout
        }

        fn git(&self, args: &[&str]) -> String {
            String::from_utf8(self.run(self.dir.path(), args, b"")).unwrap().trim().to_string()
        }

        fn blob(&self, text: &str) -> String {
            String::from_utf8(self.run(self.dir.path(), &["hash-object", "-w", "--stdin"], text.as_bytes()))
                .unwrap()
                .trim()
                .to_string()
        }

        /// A tree of `(mode, path, object id)` entries, folders made from the paths.
        fn tree(&self, entries: &[(&str, &str, &str)]) -> String {
            self.git(&["read-tree", "--empty"]);
            for (mode, path, oid) in entries {
                self.git(&["update-index", "--add", "--cacheinfo", &format!("{mode},{oid},{path}")]);
            }
            self.git(&["write-tree"])
        }

        /// A tree written byte for byte (`hash-object --literally`), for what the index would correct: a mode spelled `100664`,
        /// a name that is not UTF-8.
        fn raw_tree(&self, entries: &[(&[u8], &[u8], &str)]) -> String {
            let mut data = Vec::new();
            for (mode, name, oid) in entries {
                data.extend_from_slice(mode);
                data.push(b' ');
                data.extend_from_slice(name);
                data.push(0);
                data.extend((0..oid.len()).step_by(2).map(|i| u8::from_str_radix(&oid[i..i + 2], 16).unwrap()));
            }
            String::from_utf8(self.run(
                self.dir.path(),
                &["hash-object", "-t", "tree", "-w", "--literally", "--stdin"],
                &data,
            ))
            .unwrap()
            .trim()
            .to_string()
        }

        fn commit(&self, tree: &str, parent: &str) -> String {
            let mut args = vec!["-c", "user.name=t", "-c", "user.email=t@t.invalid", "commit-tree", tree, "-m", "c"];
            if !parent.is_empty() {
                args.extend(["-p", parent]);
            }
            self.git(&args)
        }

        /// `git diff --name-only --no-renames -z a b` in `dir`.
        fn git_diff(&self, dir: &Path, a: &str, b: &str) -> Vec<String> {
            let out = self.run(dir, &["diff", "--name-only", "--no-renames", "-z", a, b], b"");
            String::from_utf8(out)
                .unwrap()
                .split('\0')
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect()
        }
    }

    /// `changed_paths(a, b)` lists what `git diff --name-only --no-renames -z a b` lists, in its order, read in process (no git
    /// start: the case tests the in-process diff, not the fallback).
    fn same_as_git_diff(t: &Trees, a: &str, b: &str) {
        let g = Git::new(t.dir.path());
        let ours = g.changed_paths(a, b).unwrap();
        assert_eq!(g.starts(), 0, "read in process");
        let theirs = t.git_diff(t.dir.path(), a, b);
        assert_eq!(ours, theirs, "changed_paths against git diff");
        assert!(!theirs.is_empty() || a == b || ours.is_empty());
    }

    #[test]
    fn changed_paths_is_git_diff_for_a_mode_change() {
        let t = Trees::new();
        let b = t.blob("x\n");
        same_as_git_diff(
            &t,
            &t.tree(&[("100644", "f", &b), ("100644", "g", &b)]),
            &t.tree(&[("100755", "f", &b), ("100644", "g", &b)]),
        );
    }

    #[test]
    fn changed_paths_is_git_diff_for_a_mode_spelled_another_way() {
        let t = Trees::new();
        let b = t.blob("x\n");
        let a = t.raw_tree(&[(b"100644", b"f", &b)]);
        let odd = t.raw_tree(&[(b"100664", b"f", &b)]);
        assert_ne!(a, odd, "two different trees");
        same_as_git_diff(&t, &a, &odd);
    }

    #[test]
    fn changed_paths_is_git_diff_for_a_symlink() {
        let t = Trees::new();
        let (x, y) = (t.blob("x"), t.blob("y"));
        let link = t.tree(&[("120000", "l", &x)]);
        same_as_git_diff(&t, &link, &t.tree(&[("120000", "l", &y)]));
        same_as_git_diff(&t, &link, &t.tree(&[("100644", "l", &x)]));
    }

    #[test]
    fn changed_paths_is_git_diff_for_a_submodule() {
        let t = Trees::new();
        let (c1, c2) = ("1".repeat(40), "2".repeat(40));
        let file = t.blob("x\n");
        let none = t.tree(&[("100644", "f", &file)]);
        let one = t.tree(&[("100644", "f", &file), ("160000", "sub", &c1)]);
        same_as_git_diff(&t, &none, &one);
        same_as_git_diff(&t, &one, &t.tree(&[("100644", "f", &file), ("160000", "sub", &c2)]));
    }

    #[test]
    fn changed_paths_is_git_diff_for_a_file_that_becomes_a_folder_and_back() {
        let t = Trees::new();
        let b = t.blob("x\n");
        let around = [
            ("100644", "a-b", b.as_str()),
            ("100644", "a.txt", b.as_str()),
            ("100644", "a0", b.as_str()),
        ];
        let mut file = around.to_vec();
        file.push(("100644", "a", &b));
        let mut folder = around.to_vec();
        folder.push(("100644", "a/x", &b));
        let (file, folder) = (t.tree(&file), t.tree(&folder));
        same_as_git_diff(&t, &file, &folder);
        same_as_git_diff(&t, &folder, &file);
    }

    #[test]
    fn changed_paths_is_git_diff_for_a_case_only_rename() {
        let t = Trees::new();
        let b = t.blob("x\n");
        same_as_git_diff(&t, &t.tree(&[("100644", "Readme.md", &b)]), &t.tree(&[("100644", "README.md", &b)]));
    }

    #[test]
    fn changed_paths_is_git_diff_in_a_linked_worktree() {
        let t = Trees::new();
        let first = t.commit(&t.tree(&[("100644", "a", &t.blob("1\n"))]), "");
        let second = t.commit(
            &t.tree(&[("100644", "a", &t.blob("2\n")), ("100644", "d/b", &t.blob("b\n"))]),
            &first,
        );
        let linked = tempfile::tempdir().unwrap();
        let wt = linked.path().join("wt");
        t.git(&["worktree", "add", "-q", "--detach", &wt.to_string_lossy(), &first]);
        let g = Git::new(&wt);
        let ours = g.changed_paths(&first, &second).unwrap();
        assert_eq!(g.starts(), 0, "read in process in a linked worktree too");
        assert_eq!(ours, t.git_diff(&wt, &first, &second));
        assert_eq!(ours, ["a", "d/b"]);
    }

    /// Two names that are not UTF-8 would both decode to U+FFFD: the in-process diff and git's both refuse them, never merge them.
    #[test]
    fn changed_paths_refuses_names_that_are_not_utf8() {
        let t = Trees::new();
        let b = t.blob("x\n");
        let a = t.raw_tree(&[(b"100644", b"\xfe", &b)]);
        let both = t.raw_tree(&[(b"100644", b"\xfe", &b), (b"100644", b"\xff", &b)]);
        let err = Git::new(t.dir.path()).changed_paths(&a, &both).unwrap_err();
        assert!(err.contains("UTF-8"), "{err}");
    }

    #[test]
    fn name_status_moves_are_parsed_once_and_anything_else_is_an_error() {
        let got = parse_moves(b"R100\0a/x.rs\0b/x.rs\0D\0gone.txt\0R071\0m\0n\0").unwrap();
        assert_eq!(
            got,
            vec![
                Moved::Renamed {
                    from: "a/x.rs".into(),
                    to: "b/x.rs".into()
                },
                Moved::Deleted("gone.txt".into()),
                Moved::Renamed {
                    from: "m".into(),
                    to: "n".into()
                },
            ]
        );
        assert_eq!(got[0].old(), "a/x.rs");
        assert_eq!(got[1].old(), "gone.txt");
        assert!(parse_moves(b"").unwrap().is_empty());
        assert!(parse_moves(b"M\0x\0").unwrap_err().contains("neither D nor R"));
        assert!(parse_moves(b"R100\0only-one\0").unwrap_err().contains("no path"));
        assert!(parse_moves(b"D\0\xff\0").unwrap_err().contains("UTF-8"));
    }

    #[test]
    fn a_read_longer_than_the_cap_is_an_error_never_a_cut_answer() {
        let here = Path::new(".");
        let err = Git::new(here).with_max_output(4).text(&["--version"]).unwrap_err();
        assert!(err.stderr.contains("wrote more than 4 bytes"), "{err}");
        assert!(Git::new(here).text(&["--version"]).unwrap().starts_with("git version"));
    }
}
