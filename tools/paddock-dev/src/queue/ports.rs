//! The ports of the merge queue's tick (docs/plans/merge-queue.md): the ledger (Gitea pull requests and labels), the train
//! repository (git) and CI. The tick runs no code of any pull request: the gates are the first jobs of the dispatched workflow. The tick in [`super::tick`] is pure over them, so the tests run against an
//! in-memory Gitea, temporary git repositories and a scripted CI; [`super::real`] is the adapter over the real tools.

use crate::ci_runs::verdict::Verdict;
use crate::pr::gitea::Pull;
use std::collections::BTreeSet;

/// The branch the queue builds the train on (one train at a time, so one fixed name; the tick finds it again from it).
pub const TRAIN_BRANCH: &str = "train/queue";

/// Labels the queue reads and writes (`.gitea/labels.toml`).
pub mod label {
    /// Approved: the queue may take the pull request.
    pub const APPROVED: &str = "state/approved";
    /// In the train branch now.
    pub const IN_TRAIN: &str = "state/in-train";
    /// Dropped or sent back: fixes needed.
    pub const FIXES: &str = "state/fixes";
    /// On main.
    pub const LANDED: &str = "state/landed";
    /// Built and waiting for review (a pull request whose head moved after approval goes back here).
    pub const BUILT: &str = "state/built";
    /// On any pull request: the queue does nothing after reading the ledger.
    pub const HOLD: &str = "queue/hold";
    /// On the pull requests of a train whose run failed only in tests that also fail on main: the queue does not triage it
    /// again until main moves or another pull request is approved.
    pub const BLOCKED: &str = "queue/blocked";
}

/// What the commit at the tip of the train branch says about the train (trailers of an empty marker commit), so a tick can
/// resume from the branch alone: no local state file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// The pull requests merged into the train, ascending.
    pub prs: Vec<u64>,
    /// The round: 1 for a fresh train, plus one for each rebuild after a drop.
    pub round: u32,
    /// The failed run whose failures this round reruns (`since_run`), when the round follows a drop.
    pub since_run: Option<u64>,
    /// The workflow of that run (`ci.yml`, `debrief.yml`): only that workflow reruns with `since_run`.
    pub since_workflow: Option<String>,
}

/// The result of building the train.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    /// The train's head: the marker commit, checked out in the queue's working copy.
    pub head: String,
    /// The pull requests merged, in order.
    pub merged: Vec<u64>,
    /// Dropped before any gate: `(number, why)` (a conflict, a merge-check finding).
    pub dropped: Vec<(u64, String)>,
}

/// The ledger: Gitea pull requests, their labels and comments.
pub trait Ledger {
    /// The open pull requests.
    fn open_pulls(&mut self) -> Result<Vec<Pull>, String>;
    /// Applies `+label`/`-label` words to the pull request's labels (an exclusive scope keeps one).
    fn relabel(&mut self, number: u64, changes: &[&str]) -> Result<(), String>;
    /// Adds a comment.
    fn comment(&mut self, number: u64, text: &str) -> Result<(), String>;
    /// The numbers of closed pull requests that still carry `queue/hold` (a hold must not outlive its pull request).
    fn closed_held(&mut self) -> Result<Vec<u64>, String>;
    /// Whether an approving review of the pull request exists for exactly this head commit (a Gitea review in the APPROVED
    /// state, not dismissed, whose `commit_id` is `head`). A label alone is not an approval: anyone with write access can set it.
    fn approved_head(&mut self, number: u64, head: &str) -> Result<bool, String>;
}

/// The queue's working copy of the repository.
pub trait Train {
    /// Fetches `origin/main`, the train branch (absent is fine) and these branches.
    fn fetch(&mut self, branches: &[String]) -> Result<(), String>;
    /// The commit of `origin/main`.
    fn main_head(&mut self) -> Result<String, String>;
    /// The commit of `origin/train/queue`, `None` when there is none.
    fn train_head(&mut self) -> Result<Option<String>, String>;
    /// The marker of a train commit, `None` when it has none.
    fn marker(&mut self, head: &str) -> Result<Option<Marker>, String>;
    /// Whether `ancestor` is reachable from `descendant` (equal counts).
    fn contains(&mut self, ancestor: &str, descendant: &str) -> Result<bool, String>;
    /// Builds the train from `main`: merges each pull request with `--no-ff` in order, checks every merge (`./dev merge check`)
    /// and drops a conflict or a finding at once, then adds the marker commit. The result is checked out.
    fn build(&mut self, main: &str, prs: &[Pull], marker: &Marker) -> Result<Built, String>;
    /// The projects a pull request reaches (the planner's answer for `main`..`head`, run from a checkout of `main`, never of the
    /// pull request), normalised.
    fn affected(&mut self, main: &str, head: &str) -> Result<BTreeSet<String>, String>;
    /// The workflows the train's change reaches, `ci.yml` first, then each app workflow (`paddock.ci.json`'s `workflow`, read from
    /// `main`, never from the train) of an app the plan reaches.
    fn reached_workflows(&mut self, main: &str, head: &str) -> Result<Vec<String>, String>;
    /// Pushes the train branch; `old` is the head it is expected to replace (a lease, never a blind force).
    fn push_train(&mut self, head: &str, old: Option<&str>) -> Result<(), String>;
    /// Fast-forwards `origin/main` to `head` (never a force) and checks that `origin/main` is `head`.
    fn land(&mut self, head: &str) -> Result<(), String>;
}

/// What a run of the train's workflow is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    /// Queued or running.
    Running,
    /// Finished and succeeded.
    Success,
    /// Finished and did not succeed (`failure`, `cancelled`).
    Failed(String),
}

/// One run of `ci.yml` on the train head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiRun {
    /// The run's id.
    pub id: u64,
    /// What it is doing.
    pub state: RunState,
}

/// What triage says about one failed test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blame {
    /// Not on main and not known flaky: the train's own.
    New,
    /// Flaky by the history: still a red run (ER-12), so still blamed.
    Flaky,
    /// Also failing on main: not blamed on any pull request.
    OnMain,
    /// Nothing proves either way.
    Unknown,
}

/// One failure of a run, with the projects it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// What failed, for the comment (`paddock-service per_machine::tests::x`).
    pub what: String,
    /// The projects it belongs to, normalised like [`Train::affected`].
    pub projects: BTreeSet<String>,
    /// The triage answer.
    pub blame: Blame,
}

/// How a dispatch ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    /// Dispatched.
    Yes,
    /// A shared runner stayed busy: nothing was dispatched (`./dev ci dispatch`'s wait).
    Busy(String),
}

/// CI: the train's runs.
pub trait Ci {
    /// The runs of `workflow` on `head`, newest first.
    fn runs(&mut self, workflow: &str, head: &str) -> Result<Vec<CiRun>, String>;
    /// Whether there is a full green run of `workflow` on `head` (`./dev ci verdict`).
    fn verdict(&mut self, workflow: &str, head: &str) -> Verdict;
    /// Dispatches `workflow` on the train branch with these inputs.
    fn dispatch(&mut self, workflow: &str, inputs: &[(String, String)]) -> Result<Sent, String>;
    /// The failures of a finished run.
    fn failures(&mut self, run: u64) -> Result<Vec<Failure>, String>;
}
