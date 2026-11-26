//! The declaration of every `PADDOCK_*` variable and secret the tools read (ADR 0123 section 5): one row each, with its type, its
//! parser, its default, whether it is a secret and who sets it. The table is data: a test reads it through [`declaration`], and the
//! check that flags a direct read of a `PADDOCK_*` name outside this crate reads its names from it.

/// What a variable holds, and so how [`super::Settings`] parses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Free text.
    Text,
    /// A file or folder path (read as it is, never trimmed).
    Path,
    /// A list of paths in the OS's list separator.
    PathList,
    /// A number (`u64`).
    Number,
    /// A switch: on when set, not empty and not `0` or `false`.
    Switch,
    /// A switch that is on for exactly this value (`1`, `true`, `off` as each reader has always spelled it).
    Exact(&'static str),
    /// `http://host:port`, `https://host:port` or `host:port`, read by [`super::sccache::parse_endpoint`].
    Endpoint,
    /// A credential: read as a [`paddock_secrets::Secret`], never printed.
    Secret,
}

impl Kind {
    /// Whether the value is a credential.
    pub const fn is_secret(self) -> bool {
        matches!(self, Kind::Secret)
    }
}

/// Who sets a variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// A Gitea repository variable.
    Repository,
    /// A Gitea repository secret (passed to the one step that needs it).
    Secret,
    /// The machine's own environment (a runner's service or an agent's profile).
    Machine,
    /// Set by one tool or workflow step for the tools it starts, not by a person.
    Handoff,
    /// A switch or a hook for tests only; never set outside a test.
    TestOnly,
}

/// One declared variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decl {
    /// The variable's name.
    pub name: &'static str,
    /// What it holds.
    pub kind: Kind,
    /// Who sets it.
    pub scope: Scope,
    /// The value when it is unset (`None`: the reader decides, or the variable has no default).
    pub default: Option<&'static str>,
    /// One sentence: what it controls.
    pub what: &'static str,
}

const fn d(
    name: &'static str,
    kind: Kind,
    scope: Scope,
    default: Option<&'static str>,
    what: &'static str,
) -> Decl {
    Decl {
        name,
        kind,
        scope,
        default,
        what,
    }
}

use Kind::{Endpoint, Exact, Number, Path, PathList, Secret as Sec, Switch, Text};
use Scope::{Handoff, Machine, Repository, Secret as Sc, TestOnly};

/// The name of the endpoint variable, for the parser's messages.
pub const S3_ENDPOINT_NAME: &str = "PADDOCK_SCCACHE_S3_ENDPOINT";

/// Every declared variable, sorted by name (a test checks it). One row each: rustfmt would spread them over six lines.
#[rustfmt::skip]
pub const DECLARED: &[Decl] = &[
    d("PADDOCK_ACCEPT_NOT_MEASURED", Exact("1"), Machine, None, "A gate that could not be measured passes anyway (the push gate's escape, logged)."),
    d("PADDOCK_AGENT_ID", Text, Handoff, None, "The agent's id, written into a new worktree's owner note."),
    d("PADDOCK_AGENT_TYPE", Text, Handoff, None, "The agent's role, written into a new worktree's owner note."),
    d("PADDOCK_ALLOW_SPARSE", Switch, Machine, None, "Lets a gate run in a sparse checkout (a detached review copy)."),
    d("PADDOCK_ARCHIVE", Path, Handoff, None, "The archive a host tool unpacks (set by setup-rust for its child)."),
    d("PADDOCK_ARCHIVE_TO", Path, Handoff, None, "The folder a host tool unpacks into (set by setup-rust for its child)."),
    d("PADDOCK_AUDIT_ENFORCE", Switch, Handoff, None, "The member audits fail instead of warn."),
    d("PADDOCK_BASE_REF", Text, Handoff, None, "The base a CI run compares with; part of the scope a verdict is reused under."),
    d("PADDOCK_BROWSER_NO_SANDBOX", Switch, TestOnly, None, "Passed to the orphan guardian test's child: a browser without its sandbox."),
    d("PADDOCK_BUILD_COMMIT", Text, Handoff, None, "The commit a build was made from (set for the build, shown by About)."),
    d("PADDOCK_BUILD_DATE", Text, Handoff, None, "The UTC date of a build."),
    d("PADDOCK_BUILD_FOLDER", Path, Handoff, None, "The job's build folder; a base folder of the compile cache."),
    d("PADDOCK_CI_ASSUME_WINDOWS", Exact("1"), TestOnly, None, "Makes the build-jobs sizing read as Windows on another OS."),
    d("PADDOCK_CI_BUILD_JOBS", Number, Machine, None, "Overrides CARGO_BUILD_JOBS for a job."),
    d("PADDOCK_CI_CORES", Number, Machine, None, "The host's core count when the OS cannot say."),
    d("PADDOCK_CI_HOME", Path, Machine, None, "The folder a runner keeps its job homes under."),
    d("PADDOCK_CI_IDLE_SLOT", Text, Handoff, None, "Names the slot a job found idle."),
    d("PADDOCK_CI_JOBS_PER_HOST", Number, Machine, None, "How many jobs share a host's cores and memory."),
    d("PADDOCK_CI_KEEP_SLOTS", Text, Machine, None, "How many job slot folders a runner keeps."),
    d("PADDOCK_CI_LANE_LOCK", Path, Handoff, None, "The lane's lock file in a container job."),
    d("PADDOCK_CI_LANE_ROOT", Path, Handoff, None, "The lane's folder in a container job."),
    d("PADDOCK_CI_MIN_FREE_GB", Number, Machine, Some("30"), "The free disk a job slot needs before it starts."),
    d("PADDOCK_CI_MOUNT", Path, Handoff, None, "The kept mount of a container job."),
    d("PADDOCK_CI_ORPHAN_DAYS", Number, Machine, Some("14"), "Days after which an unclaimed job folder counts as an orphan."),
    d("PADDOCK_CI_RAM_MB", Number, Machine, None, "The host's memory when the OS cannot say."),
    d("PADDOCK_CI_RELEASE", Switch, Handoff, None, "The run is a release or rule 54 run: it never reads the shared S3 cache."),
    d("PADDOCK_CI_RESERVED_CORES", Number, Machine, None, "Cores kept free for the host."),
    d("PADDOCK_CI_RESERVED_RAM_MB", Number, Machine, None, "Memory kept free for the host."),
    d("PADDOCK_CI_RUSTC_MB", Number, Machine, None, "Memory one rustc is expected to use."),
    d("PADDOCK_CI_SCCACHE_GB", Number, Machine, Some("40"), "The host's compile-cache budget in GB, divided between its slots."),
    d("PADDOCK_CI_SLOT", Text, Handoff, None, "The job's slot name."),
    d("PADDOCK_CI_SLOTS", Number, Machine, None, "How many job slots a host has."),
    d("PADDOCK_CI_SLOT_TOKEN", Text, Handoff, None, "The holder token of the job's slot lock."),
    d("PADDOCK_CI_TABLES", Path, Handoff, None, "Where the slot tables are, for a child tool."),
    d("PADDOCK_CI_TEST_THREADS", Number, Machine, None, "Test threads per job."),
    d("PADDOCK_CI_TEST_WORKERS", Number, Machine, None, "Browser test workers per job."),
    d("PADDOCK_CONFORMANCE_ARGS", Text, Handoff, None, "The conformance line of ci.yml, handed to a gate."),
    d("PADDOCK_CONFORMANCE_RUST_ARGS", Text, Handoff, None, "The Rust conformance line of ci.yml, handed to a gate."),
    d("PADDOCK_DEPS_CACHE_MAX_GIB", Number, Machine, None, "The size limit of the shared dependency cache."),
    d("PADDOCK_DEV", Path, Handoff, None, "The `./dev` binary a child tool calls."),
    d("PADDOCK_DEV_CONTRACT", Text, Handoff, None, "The version of the wrapper contract `./dev` was started under."),
    d("PADDOCK_DEV_DOWNLOAD", Text, Handoff, None, "Whether `./dev` may download its binary (setup-rust sets it from the read token)."),
    d("PADDOCK_DEV_EMBEDDED_HASH", Text, Handoff, None, "The input hash baked into the binary at build time."),
    d("PADDOCK_DEV_FRESH", Switch, Handoff, None, "`./dev` was just rebuilt."),
    d("PADDOCK_DEV_HASH", Text, Handoff, None, "The input hash the wrapper computed."),
    d("PADDOCK_DEV_HOME", Path, Machine, None, "Where `./dev` keeps its tool cache."),
    d("PADDOCK_DEV_TARGETS", Path, Machine, None, "The folder holding worktrees, build folders and run logs."),
    d("PADDOCK_DEV_TARGET_DIR", Path, Machine, None, "A build folder for `./dev`'s own build."),
    d("PADDOCK_DEV_TOKEN", Sec, Sc, None, "The read token `./dev` downloads its binary with."),
    d("PADDOCK_DEV_WRAPPER_HASH", Text, Handoff, None, "The hash of the wrapper script."),
    d("PADDOCK_DF", Path, TestOnly, None, "A stand-in for `df`, so a test never reads the real disk."),
    d("PADDOCK_DISK_FOLDER_TIMEOUT_SECS", Number, Machine, Some("60"), "Seconds one folder may take to measure."),
    d("PADDOCK_DISK_INDEX", Path, Machine, None, "The disk report's index file."),
    d("PADDOCK_DISK_JOBS", Number, Machine, None, "Parallel folder measurements of the disk report."),
    d("PADDOCK_DISK_LIVE_HOURS", Number, Machine, Some("48"), "Hours a folder counts as live."),
    d("PADDOCK_DISK_REFUSE_GIB", Number, Machine, Some("50"), "Free GiB under which a job is refused."),
    d("PADDOCK_DISK_ROOTS", PathList, Machine, None, "The folders the disk report measures."),
    d("PADDOCK_DISK_WARN_GIB", Number, Machine, Some("200"), "Free GiB under which the disk report warns."),
    d("PADDOCK_DISPATCH_AUDITS", Path, TestOnly, None, "A stand-in for the dispatch audit program."),
    d("PADDOCK_E2E_WORKERS", Number, Handoff, None, "Browser workers, set by the build-jobs step for later steps."),
    d("PADDOCK_GATE_TARGET_DIR", Path, Handoff, None, "The build folder a gate uses."),
    d("PADDOCK_GATE_WATCHDOG", Text, Machine, None, "The push gate's hang detector mode."),
    d("PADDOCK_GUARDIAN_BUILD_TIMEOUT", Number, Machine, Some("600"), "Seconds the orphan guardian's build may take."),
    d("PADDOCK_GUARDIAN_TEST_TIMEOUT", Number, Machine, Some("300"), "Seconds the orphan guardian's test may take."),
    d("PADDOCK_HEAVY", Path, Machine, None, "The heavy runner's binary (the older name of PADDOCK_THROTTLE)."),
    d("PADDOCK_HEAVY_AT_ONCE", Number, Machine, None, "How many heavy commands run at once."),
    d("PADDOCK_HEAVY_BIN", Path, Machine, None, "The heavy runner's binary for a CI step."),
    d("PADDOCK_INTERNAL_TMP_ROOT", Path, Handoff, None, "The job's internal temp root."),
    d("PADDOCK_JOB_TEMP", Path, Handoff, None, "The job's temp folder."),
    d("PADDOCK_KEPT_CLONE_CLOCK_MARK", Text, TestOnly, None, "Marks the clock a kept-clone test uses."),
    d("PADDOCK_KEPT_CLONE_MAX_MIB", Number, Machine, Some("3072"), "The size a kept clone may grow to."),
    d("PADDOCK_KEPT_CLONE_TIMEOUT", Number, Machine, None, "Seconds any kept-clone phase may take."),
    d("PADDOCK_KEPT_CLONE_TIMEOUT_CHECKOUT", Number, Machine, None, "Seconds the checkout phase may take."),
    d("PADDOCK_KEPT_CLONE_TIMEOUT_CLEAN", Number, Machine, None, "Seconds the clean phase may take."),
    d("PADDOCK_KEPT_CLONE_TIMEOUT_CREATE", Number, Machine, None, "Seconds the create phase may take."),
    d("PADDOCK_KEPT_CLONE_TIMEOUT_FETCH", Number, Machine, None, "Seconds the fetch phase may take."),
    d("PADDOCK_KEPT_CLONE_TIMEOUT_LINK", Number, Machine, None, "Seconds the link phase may take."),
    d("PADDOCK_KEPT_CLONE_TIMEOUT_RESOLVE", Number, Machine, None, "Seconds the resolve phase may take."),
    d("PADDOCK_KEPT_CLONE_TIMEOUT_VERIFY", Number, Machine, None, "Seconds the verify phase may take."),
    d("PADDOCK_MERGE_CHECK_CARGO", Path, TestOnly, None, "A stand-in for cargo in the merge check's tests."),
    d("PADDOCK_MERGE_CHECK_GATES", Path, TestOnly, None, "A stand-in for the member gate in the merge check's tests."),
    d("PADDOCK_MERGE_CHECK_TRIAL", Path, TestOnly, None, "A stand-in for the trial run in the merge check's tests."),
    d("PADDOCK_NO_REUSE", Exact("1"), Machine, None, "The push gate re-runs rows whose verdict it could reuse."),
    d("PADDOCK_OUT", Path, Handoff, None, "The folder a host tool writes to (set by setup-rust for its child)."),
    d("PADDOCK_PLAN_SELECTION", Text, Handoff, None, "The plan's selection a run step reads."),
    d("PADDOCK_PREPUSH_WORKTREES", Path, Machine, None, "Where the pre-push gate makes its scratch worktrees."),
    d("PADDOCK_PROC_GUARDIAN_LOG", Path, Handoff, None, "The orphan guardian's log file."),
    d("PADDOCK_PRUNE_UNIT", Text, Handoff, None, "The unit a kept-folder prune reports in."),
    d("PADDOCK_PS", Path, TestOnly, None, "A stand-in for `ps`."),
    d("PADDOCK_QUEUE_PUSH_TOKEN", Sec, Sc, None, "The token the queue pushes with."),
    d("PADDOCK_QUEUE_STALL", Number, Machine, None, "Seconds without output before a queue step counts as stalled."),
    d("PADDOCK_RELEASE_CLONE", Path, Handoff, None, "The release job's clone."),
    d("PADDOCK_RELEASE_LEDGER", Path, Handoff, None, "The release job's ledger of folders it made."),
    d("PADDOCK_RELEASE_MODE", Exact("1"), Handoff, None, "The job is a release job."),
    d("PADDOCK_RELEASE_NODE_MODULES", Path, Handoff, None, "The release job's node_modules folder."),
    d("PADDOCK_RELEASE_TARGET", Path, Handoff, None, "The release job's build folder."),
    d("PADDOCK_RELEASE_TAURI_TOOLS", Path, Handoff, None, "The release job's Tauri tools folder."),
    d("PADDOCK_S3_GUARD_LOG", Path, Handoff, None, "The S3 timeout guard's log file."),
    d("PADDOCK_S3_GUARD_PORT", Number, Handoff, None, "The port of the S3 timeout guard on 127.0.0.1."),
    d("PADDOCK_SCCACHE", Exact("off"), Machine, None, "`off` turns the local compile cache off."),
    d("PADDOCK_SCCACHE_BACKEND", Text, Machine, None, "The local server's backend: auto, local or s3."),
    d("PADDOCK_SCCACHE_ENV_FILE", Path, Machine, None, "The credentials file of the local compile cache, outside the repository."),
    d("PADDOCK_SCCACHE_MOUNT", Path, TestOnly, None, "The kept mount of a container job (a test hook)."),
    d("PADDOCK_SCCACHE_REDIS_ENDPOINT", Text, Repository, None, "The Redis compile cache's endpoint (may hold a password)."),
    d("PADDOCK_SCCACHE_RELEASES", Text, TestOnly, None, "The download base for sccache, so a test never reaches the network."),
    d("PADDOCK_SCCACHE_S3_ACCESS_KEY_ID", Sec, Sc, None, "The compile cache's S3 access key id."),
    d("PADDOCK_SCCACHE_S3_BUCKET", Text, Repository, Some("sccache-paddock"), "The compile cache's S3 bucket."),
    d("PADDOCK_SCCACHE_S3_ENDPOINT", Endpoint, Repository, Some("172.16.1.220:15000"), "The S3 store's endpoint: http://host:port."),
    d("PADDOCK_SCCACHE_S3_PREFIX", Text, Machine, None, "The key prefix of the local server's S3 level (default: CI's for the same OS and arch)."),
    d("PADDOCK_SCCACHE_S3_REGION", Text, Repository, Some("auto"), "The S3 region the test cache signs requests for."),
    d("PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY", Sec, Sc, None, "The compile cache's S3 secret access key."),
    d("PADDOCK_SCCACHE_S3_SSL", Exact("true"), Repository, None, "`true` makes the test cache talk TLS to a bare host:port endpoint."),
    d("PADDOCK_SETUP_PRODUCT", Text, Handoff, None, "The product a setup step prepares for."),
    d("PADDOCK_SETUP_VM", Text, Handoff, None, "The test machine a setup step prepares."),
    d("PADDOCK_SLOT_WAIT_SECONDS", Number, Machine, None, "Seconds between two tries to take a job slot."),
    d("PADDOCK_SLOT_WAIT_TRIES", Number, Machine, None, "How many times a job tries to take a slot."),
    d("PADDOCK_SOCKET_BASE", Path, Handoff, None, "The base of the job's socket folder."),
    d("PADDOCK_SOCKET_ROOT", Path, Machine, None, "The root of socket folders (default /tmp)."),
    d("PADDOCK_STABLE_WORKSPACE_TIMEOUT", Number, Machine, None, "Seconds the stable-workspace step may take."),
    d("PADDOCK_STAND_IN_BIN", Path, TestOnly, None, "A stand-in binary the orphan guardian test starts."),
    d("PADDOCK_SYNC_TRIAL_DISPATCH", Path, TestOnly, None, "A stand-in for the trial dispatch in the merge check's tests."),
    d("PADDOCK_TAG_REHEARSAL", Exact("1"), Handoff, None, "The run is a tag rehearsal: it never reads the shared S3 cache."),
    d("PADDOCK_TEST_CACHE_KEY_ID", Sec, Sc, None, "The test-result cache's S3 access key id (its own key)."),
    d("PADDOCK_TEST_CACHE_SECRET", Sec, Sc, None, "The test-result cache's S3 secret key."),
    d("PADDOCK_TEST_CONCURRENCY", Number, Handoff, None, "Conformance test concurrency, set by the push gate."),
    d("PADDOCK_TEST_COST_REPORT", Path, Handoff, None, "Where a test run writes its cost report."),
    d("PADDOCK_TEST_SLEEPER_FILE", Path, TestOnly, None, "The file a test's sleeper child reads."),
    d("PADDOCK_TEST_THROTTLE_OFF", Exact("1"), TestOnly, None, "Tests run without the heavy runner."),
    d("PADDOCK_TEST_UID", Number, TestOnly, None, "The user id the orphan guardian's acting test runs as."),
    d("PADDOCK_THROTTLE", Path, Machine, None, "The heavy runner's binary."),
    d("PADDOCK_THROTTLE_BIN", Path, Machine, None, "The heavy runner's binary for a CI step."),
    d("PADDOCK_THROTTLE_RELEASES", Text, TestOnly, None, "The download base for the heavy runner, so a test never reaches the network."),
    d("PADDOCK_TRAIN_CI_LOCK_ROOT", Path, Machine, None, "Where the train CI reservation lock lives."),
    d("PADDOCK_TRIAL_DIR", Path, TestOnly, None, "The scratch folder of a trial run (not configuration)."),
    d("PADDOCK_UPDATE_GOLDEN", Switch, TestOnly, None, "A golden-file test rewrites its golden instead of comparing."),
    d("PADDOCK_VM", Text, Machine, None, "Names a test machine for a VM gate."),
    d("PADDOCK_VM_DATA", Path, Machine, None, "Where test machines keep their data."),
    d("PADDOCK_VM_LOCK_ROOT", Path, Machine, None, "Where test machines' locks live."),
    d("PADDOCK_WORKSPACE", Path, Handoff, None, "The job's checkout; a base folder of the compile cache."),
    d("PADDOCK_WORKTREE_ROOT", Path, Machine, None, "The folder worktrees are made in (git config paddock.worktreeRoot is the other source)."),
    d("PADDOCK_ZIP", Path, Handoff, None, "The archive a host tool packs (set by setup-rust for its child)."),
];

impl Decl {
    /// The default as a number (`None`: no default, or not a number). `const`, so a reader's default constant is this declaration
    /// and not a second copy of the literal.
    pub const fn default_u64(&self) -> Option<u64> {
        let Some(text) = self.default else {
            return None;
        };
        let b = text.as_bytes();
        if b.is_empty() {
            return None;
        }
        let mut n: u64 = 0;
        let mut i = 0;
        while i < b.len() {
            if !b[i].is_ascii_digit() {
                return None;
            }
            n = n * 10 + (b[i] - b'0') as u64;
            i += 1;
        }
        Some(n)
    }
}

/// The declaration of `name`, or `None` when the name is not declared.
pub fn declaration(name: &str) -> Option<&'static Decl> {
    DECLARED
        .binary_search_by(|d| d.name.cmp(name))
        .ok()
        .map(|i| &DECLARED[i])
}

/// The declared rows, as `const` items the readers name (`settings::PADDOCK_SCCACHE_S3_BUCKET`).
macro_rules! named {
    ($($id:ident),* $(,)?) => {
        $(
            #[doc = concat!("The declaration of `", stringify!($id), "`.")]
            pub const $id: &Decl = match find(stringify!($id)) {
                Some(d) => d,
                None => panic!(concat!(stringify!($id), " is not declared")),
            };
        )*
    };
}

const fn find(name: &str) -> Option<&'static Decl> {
    let mut i = 0;
    while i < DECLARED.len() {
        if str_eq(DECLARED[i].name, name) {
            return Some(&DECLARED[i]);
        }
        i += 1;
    }
    None
}

const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

named!(
    PADDOCK_CI_RELEASE,
    PADDOCK_DISK_FOLDER_TIMEOUT_SECS,
    PADDOCK_DISK_INDEX,
    PADDOCK_DISK_JOBS,
    PADDOCK_DISK_LIVE_HOURS,
    PADDOCK_DISK_REFUSE_GIB,
    PADDOCK_DISK_ROOTS,
    PADDOCK_DISK_WARN_GIB,
    PADDOCK_SCCACHE_S3_ACCESS_KEY_ID,
    PADDOCK_SCCACHE_S3_BUCKET,
    PADDOCK_SCCACHE_S3_ENDPOINT,
    PADDOCK_SCCACHE_S3_PREFIX,
    PADDOCK_SCCACHE_S3_REGION,
    PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY,
    PADDOCK_SCCACHE_S3_SSL,
    PADDOCK_TAG_REHEARSAL,
    PADDOCK_TEST_CACHE_KEY_ID,
    PADDOCK_TEST_CACHE_SECRET,
);
