use crate::{
    audit, check, check_ids, ci_build_env, ci_build_jobs, ci_dev_tool_store, ci_dyno_linux_units, ci_host, ci_isolation, ci_job_model,
    ci_run, ci_runs, ci_rust_pin, ci_s3_guard, ci_setup_rust, deps_cache, disk, gates, generators, heavy_cmd, heavy_install, hook,
    input_hash, ipc_endpoints, lessons_driver, load, local_sccache, locks, locks_guard, merge_check, mutants_gate, places, plan_cmd, pr,
    pre_push, queue, release_isolation, slots, test_cache, test_features, wait, worktree,
};
use clap::{Args, Parser, Subcommand};
use paddock_cli::{CliError, Exit, Format, Report, finish};
use std::io::Write;
use std::path::PathBuf;

/// `./dev`: the Paddock developer and CI tool.
#[derive(Parser)]
#[command(name = "dev", version, disable_help_subcommand = true)]
pub(crate) struct Cli {
    /// Write exactly one JSON object to standard output (errors included).
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// The tool itself.
    #[command(name = "self")]
    SelfCmd {
        #[command(subcommand)]
        command: SelfCommand,
    },
    /// CI helpers.
    Ci {
        #[command(subcommand)]
        command: CiCommand,
    },
    /// Wait, with a limit, for a plock job, a process, a line in a file or a CI run. Use it instead of `pgrep -f` loops.
    Wait(wait::WaitArgs),
    /// Run a command through the machine-wide throttle slot (ER-24), found the way `check --pre-push` finds it; git hooks use it.
    /// `./dev heavy` is the old name: it still works and prints a one-line deprecation.
    #[command(alias = "heavy")]
    Throttle(heavy_cmd::ThrottleArgs),
    /// Install the `throttle` binary of a release into `<dev-targets>/throttle/bin`: downloads the asset for this OS, checks its SHA256SUMS entry, renames it into place, records the version.
    #[command(alias = "heavy-install")]
    ThrottleInstall(heavy_install::InstallArgs),
    /// One line for the coordinator: start, hold or stop for new agents, from the load, the free disk of the dev-targets folder and the CI queue (ER-24).
    Load(load::LoadArgs),
    /// The machine's dev folders, derived from `git config paddock.worktreeRoot` (docs/dev-setup.md): dev-targets, dev-disk,
    /// vm-data, lock-root and main-checkout; `--get <name>` prints one, `--pinned` ignores every environment variable.
    Places(places::PlacesArgs),
    /// C100 and the gate register (ADR 0060): is every check run somewhere?
    Gates {
        #[command(subcommand)]
        command: GatesCommand,
    },
    /// Advisory audits (cargo deny, npm audit): `nightly` on main, `compare` judges only what a change adds.
    Audit {
        #[command(subcommand)]
        command: audit::AuditCommand,
    },
    /// Mutation-testing of the diff (cargo-mutants `--in-diff`), mutants.yml (after a landing): a function whose every mutation passes
    /// the tests is a finding. Report-only for now (warnings, exit 0).
    Mutants {
        #[command(subcommand)]
        command: MutantsCommand,
    },
    /// Lockfiles: are they all current, and regenerate them with cargo, cargo-hakari and npm (never by hand).
    Locks {
        #[command(subcommand)]
        command: LocksCommand,
    },
    /// The conformance checks on the Rust host, with the command line, output and exit codes of `paddock-conformance`
    /// (`--app`, `--root`, `--only`, `--strict`, `--json`, `--out`, `--summary`, `list`, `commands`). It runs the checks ported so far;
    /// `node conformance/bin/paddock-conformance.mjs` keeps the rest.
    #[command(disable_help_flag = true)]
    Conformance {
        /// The arguments of `paddock-conformance`.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Repository checks of the family.
    Check {
        #[command(subcommand)]
        command: CheckCommand,
    },
    /// Agent worktrees (ADR 0083): add, claim, reassign, release, drop, list and clean. The caller's agent id is self-declared
    /// until the guard hook (RT-7) passes the real one.
    Worktree {
        #[command(subcommand)]
        command: worktree::WorktreeCommand,
    },
    /// The local sccache for an agent's own cargo calls: `eval "$(./dev sccache env)"` installs the pinned release when needed,
    /// starts the one local server and prints the `export` lines (RUSTC_WRAPPER and the cache settings). Warns and prints nothing
    /// when sccache cannot be used (the build then runs without it).
    Sccache {
        #[command(subcommand)]
        command: SccacheCommand,
    },
    /// The shared cache of `node_modules` and `ui/dist`: `install` and `ui` give this worktree a copy-on-write clone of a result built
    /// once per content key under `<dev-targets>/cache/npm`, or build it once.
    Deps {
        #[command(subcommand)]
        command: deps_cache::DepsCommand,
    },
    /// Disk watch and clean-up: free-space check, usage report, scan, owner records, clean.
    Disk {
        #[command(subcommand)]
        command: disk::DiskCommand,
    },
    /// The branch ledger on Gitea pull requests: open (a draft), label, list, sync (the labels of .gitea/labels.toml) and
    /// plan-status (plan rows checked against the ledger). Reads GITEA_TOKEN; opening a pull request starts no CI run.
    Pr {
        #[command(subcommand)]
        command: pr::PrCommand,
    },
    /// The merge queue (docs/plans/merge-queue.md): `queue tick` takes one step. Reads GITEA_TOKEN (queue-bot's).
    Queue {
        #[command(subcommand)]
        command: queue::QueueCommand,
    },
    /// The CI planner: which projects a change reaches and the CI matrix (the port of the retired scripts/affected.mjs; the
    /// arguments are its: <base> [<head>], --files a,b, --all, --ci [--app x], --explain, and --since-run <run>).
    /// It prints no JSON object: `--json` is refused.
    Plan(PlanArgs),
    /// Merge checks.
    Merge {
        #[command(subcommand)]
        command: MergeCommand,
    },
    /// Generated and pinned files (ER-40): the registry docs/generators.toml and the checks it declares.
    Gen {
        #[command(subcommand)]
        command: GenCommand,
    },
    /// Claude Code hooks (ADR 0083). Run by the installed copy, never through `./dev`.
    Hook {
        #[command(subcommand)]
        command: HookCommand,
    },
    /// Put the hook binary and its shim in place.
    Hooks {
        #[command(subcommand)]
        command: HooksCommand,
    },
}

#[derive(Subcommand)]
enum HookCommand {
    /// The PreToolUse guard: reads one tool call as JSON on standard input, prints a deny decision or nothing. Exit 0 always.
    PreToolUse,
}

#[derive(Subcommand)]
enum HooksCommand {
    /// Copy this binary to <git-common-dir>/paddock/hooks/ and write the shim tools/githooks/claude-pre-tool-use.
    Install(hook::install::InstallArgs),
}

#[derive(Subcommand)]
enum CheckCommand {
    /// The repository checks ported to Rust so far (C39, C53; RT-5): findings, expiring waivers from paddock.waivers.toml, a run
    /// record per check (`not measured` is told from `clean`). The report JSON has the shape of paddock-conformance's, plus `runs`.
    Run {
        /// Only these check ids, comma separated (for example C39,C53).
        #[arg(long, value_delimiter = ',')]
        only: Option<Vec<String>>,
        /// Only these repository-relative files, comma separated (the scope of a push gate).
        #[arg(long, value_delimiter = ',')]
        files: Option<Vec<String>>,
        /// The repository root (default: the folder containing .git, found from the current folder).
        #[arg(long)]
        root: Option<PathBuf>,
        /// The app name in the report.
        #[arg(long, default_value = "paddock")]
        app: String,
        /// Also write the report JSON to this file.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// ER-9's fast gates for what this push changes (fmt, host and Windows Clippy, conformance,
    /// commit-lint, lockfiles `--locked`), the commands CI runs, in parallel and bounded by load. A gate that could not run
    /// is "not measured" (exit 6), never a pass; a failure is one line with its next step (exit 5). The pre-push hook runs it.
    #[command(name = "pre-push", long_flag = "pre-push")]
    PrePush(pre_push::PrePushArgs),
    /// The next free conformance check id, read from the working tree, every local branch, every branch of origin as the
    /// remote has it now (git ls-remote) and conformance/reserved/ (ER-21: two branches took the same id five times). `--reserve <branch>` takes the id on origin at once (a push of refs/paddock/check-ids/C<n>, which only one agent can create) and writes the reservation file.
    NewId {
        /// Reserve on origin (refs/paddock/check-ids/C<n>), then create conformance/reserved/C<n>-<branch-slug> (commit it: the record).
        #[arg(long)]
        reserve: Option<String>,
        /// The repository root (default: the folder containing .git, found from the current folder).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// C102 (ADR 0072): every listen site is in docs/ipc-endpoints.toml with an ending test or a reason, and the ratchet of
    /// connection endings is well formed. With `--tag`, also refuses a release tag that a ratchet entry blocks.
    IpcEndpoints {
        /// A release tag to check against the ratchet's `blocks_tag` patterns (for example `dyno-v1.2.0`).
        #[arg(long)]
        tag: Option<String>,
        /// The repository root (default: the folder containing .git, found from the current folder).
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// C170 (rule 99): no released binary (each app's Tauri package and every `bundle.externalBin`, and paddock-setup) is built
    /// with a test feature (`testing`, `test-support`, ...): `cargo tree -e normal` per binary. Report-only until its first green
    /// run (ER-41); a probe that cannot see is "check broken" (exit 7).
    #[command(name = "test-features")]
    TestFeatures {
        /// Only this app's binaries (default: every app and paddock-setup).
        #[arg(long)]
        app: Option<String>,
        /// The repository root (default: the folder containing .git, found from the current folder).
        #[arg(long)]
        root: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum MergeCommand {
    /// Did a merge silently drop one side's work? Flags files that go backwards against the first parent, hunks a parent added
    /// that the result lacks (reason: `Hunks-dropped: <path>: <reason>` in the merge message) and tests a parent has that the
    /// result lacks (`Tests-removed: <name>: <reason>`). Given a merge commit that is checked out, it also runs the generator
    /// checks the merge makes due (docs/generators.toml; `--no-generators` skips them). Exit 5 on findings; a check that could not
    /// look is an error, never a pass.
    Check(MergeCheckArgs),
    /// Before merging the train into a member branch: list the commands that re-apply the member's own work the train reverted
    /// (`git revert <sha>`, git's "revert a faulty merge"; run them after `git merge <train>`), and warn about reverts of other
    /// branches' merges the member holds. Run it before the merge of the train: it changes nothing. Run it again after the merge and it
    /// also runs the fast static rows on the sync tip (`check --pre-push --priority train --only fmt,conformance,gates-register,clippy,clippy-windows
    /// --base <train>`, in the train lane of the queue); a failing row is exit 5 (6 not measured) with the rows and the next step, unless `--no-static` says why not. Once
    /// they pass it dispatches the member's own CI trial: it pushes the sync tip as the new ref `sync/<member>/<short sha>` (integrator role only) and runs `ci dispatch ci.yml
    /// --trial sync/<member>/<short sha> --onto <train>` (the affected jobs of the member on the train), printing the run id, unless `--no-trial` says why not.
    SyncMember(SyncMemberArgs),
    /// Merge a member into the train in this worktree (run it in the train's worktree): `git merge --no-ff` with the fixed subject
    /// `Merge <member> into <train>` and the trailers every reader reads first (`Paddock-Member`, `Paddock-Sync-Ref`, `Paddock-Sync-Sha`,
    /// the `Reverted:` lines `merge check --write-reverted` would write, and `Paddock-Reland` for each `--reland`). A conflict stops it with
    /// the merge in progress and the message prepared; it resolves nothing. After the merge it runs `merge check` on the new commit.
    /// Take a train merge by this command, never by hand (user decision 100, 2026-11-27).
    Member(MemberArgs),
    /// Git's merge driver of docs/lessons.md (`merge=lessons`, set by `./dev hooks install`): `./dev merge lessons-driver %O %A %B`.
    /// Git's union merge of two appended sections, then the blank line it drops before a `## ` heading put back. The result is
    /// written to the second file (ours).
    LessonsDriver {
        /// The merge base (%O).
        base: PathBuf,
        /// Our version (%A); receives the result.
        ours: PathBuf,
        /// Their version (%B).
        theirs: PathBuf,
    },
}

#[derive(Args)]
struct MemberArgs {
    /// The member's branch, or its sync ref (`sync/<member>/<sha>`); the remote's `origin/` copy is used when there is no local one.
    member: String,
    /// The train's worktree (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// A revert on the train of the member's own earlier merge that the member re-applies (repeatable): named in a `Paddock-Reland`
    /// trailer, and its files get `Reverted:` lines so the merge check accepts the re-add.
    #[arg(long, value_name = "REVERT")]
    reland: Vec<String>,
    /// Emergency override of the old-base block, handed to the merge check (see `merge check --old-base-anyway`).
    #[arg(long, value_name = "REASON")]
    old_base_anyway: Option<String>,
}

#[derive(Args)]
struct SyncMemberArgs {
    /// The train to merge in (for example `origin/train/2026-11-17-14`).
    train: String,
    /// The member's worktree (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// The member's branch name (default: the branch HEAD is on).
    #[arg(long)]
    member: Option<String>,
    /// Do not run the static rows on the sync tip; the reason is printed (like `merge check --old-base-anyway`).
    #[arg(long, value_name = "REASON")]
    no_static: Option<String>,
    /// Do not dispatch the member's own CI trial (the affected jobs of the member on the train, `ci dispatch --trial`); the reason is
    /// printed. After the static rows pass the trial is dispatched from the sync tip, pushed as `sync/<member>/<short sha>` (integrator role only); `merge check --gates` reads it.
    #[arg(long, value_name = "REASON")]
    no_trial: Option<String>,
    /// The longest the trial's dispatch waits for a busy runner (default 30m).
    #[arg(long, value_name = "DURATION", default_value = merge_check::trial_dispatch::DEFAULT_MAX)]
    trial_max: String,
    /// A revert on the train of the member's own earlier merge that the sync tip re-applies (repeatable); the ones in the member's
    /// history that `git revert <revert>` already re-applied are found without it. The trial's merge carries a `Reverted:` line for each
    /// file the revert took out, so the trial's merge check accepts the re-add.
    #[arg(long, value_name = "REVERT")]
    reland: Vec<String>,
}

#[derive(Subcommand)]
enum GenCommand {
    /// Run the checks of the generators a change touches (docs/generators.toml: the lockfiles, catalogues, notices, workspace
    /// hack, baselines and bindings that are derived from other files). Each command and its duration is logged; a stale file
    /// is exit 5 with its regenerate command; a check that could not run (tool missing, file not on this tree) is exit 6, never a pass.
    Check(GenCheckArgs),
}

#[derive(Args)]
struct GenCheckArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// Run every generator, not only those the change touches.
    #[arg(long)]
    all: bool,
    /// Run only these generator ids (comma-separated), whatever the change is.
    #[arg(long, value_delimiter = ',')]
    only: Vec<String>,
    /// The changed paths (comma-separated), instead of asking git.
    #[arg(long, value_delimiter = ',')]
    files: Vec<String>,
    /// The revision the change is measured against (its merge base with HEAD, working tree included).
    #[arg(long, default_value = "origin/main")]
    base: String,
    /// `train` for an integrator's run: its heavy builds go before normal waiters in `throttle` (`throttle run --high`).
    #[arg(long, value_enum, default_value_t = crate::priority::Priority::Normal)]
    priority: crate::priority::Priority,
}

#[derive(Args)]
struct MergeCheckArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// A branch about to be merged (before the merge exists); needs --target.
    #[arg(long, requires = "target", conflicts_with = "commit")]
    branch: Option<String>,
    /// The branch it would be merged into.
    #[arg(long, requires = "branch")]
    target: Option<String>,
    /// The merge commit to check.
    commit: Option<String>,
    /// Do not run the generator checks of docs/generators.toml (by default the commit form runs those the merge makes due; the
    /// checkout must be the merge result, else they are NOT MEASURED).
    #[arg(long)]
    no_generators: bool,
    /// Also run the heavy generators the merge makes due (each builds a workspace: minutes per merge). By default they are named as
    /// deferred to the final `./dev gen check --base origin/main`, which runs the train's union of them once.
    #[arg(long, conflicts_with = "no_generators")]
    heavy_generators: bool,
    /// Also run the member gate on the merge commit: `check --pre-push --base <merge>^1`, the fast gates on the combined tree
    /// (ADR 0098). The checkout must be the merge result. Report-only until its first green run on a real train is recorded.
    #[arg(long)]
    gates: bool,
    /// A member that re-lands work the target reverted (`revert: reapply ...`, `Reapply "..."`) goes backwards by design: write
    /// the merge message with one `Reverted: <path>: <reason>` line for each backwards finding the re-land explains (its commit
    /// undoes the revert that changed the file) to this file, for `git commit --amend -F <file>` or `git merge -F <file>`.
    /// Any other finding stays a finding for a person to answer.
    #[arg(long, value_name = "FILE")]
    write_reverted: Option<PathBuf>,
    /// Emergency override of the old-base block: merge a member that lacks the train's base anyway, for this reason (required,
    /// printed with the result). The default is an error that says to merge origin/main into the member and rerun its gate.
    #[arg(long, value_name = "REASON")]
    old_base_anyway: Option<String>,
    /// With `--gates`: do not read the member's own CI trial run (`merge sync-member` dispatches it); the reason is required and printed
    /// (like `--old-base-anyway`). A red trial otherwise blocks the member, a running one is not a pass.
    #[arg(long, value_name = "REASON")]
    no_trial: Option<String>,
    /// With `--gates`: measure the member gate from this ref (`origin/main`) instead of the merge's first parent, so it plans the whole
    /// train's rows and leaves a pass for each: the push hook and the train-audits guard then reuse them on the same tree.
    #[arg(long, value_name = "REF")]
    gates_base: Option<String>,
}

#[derive(Subcommand)]
enum SelfCommand {
    /// Print the input hash this binary's sources, dependencies and compiler give, and the hash it was built from.
    Hash,
}

#[derive(Args)]
struct ConformanceArgsArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// Whose line: `js` (default, the arguments of `node conformance/bin/paddock-conformance.mjs`) or `rust` (the arguments of
    /// `./dev conformance`, the checks ported in pair P3).
    #[arg(long, default_value = "js", value_parser = ["js", "rust"])]
    host: String,
}

#[derive(Subcommand)]
enum CiCommand {
    /// Job-isolation slots held by kernel file locks.
    Slot {
        #[command(subcommand)]
        command: SlotCommand,
    },
    /// The CI kit's job isolation on a host runner: slots, kept folders, ports, process patterns, temp folders (the port of
    /// `conformance/ci/job-isolation.sh`). Values go to standard output, messages to standard error.
    Isolation(ci_isolation::IsolationArgs),
    /// The compile cache (sccache: installed, configured, a fresh server) and the Tauri Linux libraries of a CI job: the port of
    /// the setup-rust action's shell steps. The action keeps CARGO_BUILD_JOBS and the toolchain install in YAML (./dev needs
    /// rustc first). Messages and `::warning::` lines on standard output, one `::error::` line on failure.
    SetupRust(ci_setup_rust::SetupRustArgs),
    /// The localhost proxy in front of CI's S3 compile cache that cuts a request the store does not answer (user decision
    /// 2026-11-20): `ci setup-rust` starts it, `ci sccache-stats` stops it.
    S3Guard {
        #[command(subcommand)]
        command: ci_s3_guard::GuardCommand,
    },
    /// The compiler a release job builds with is the exact version `rust-toolchain.toml` pins (`rustc --version` against its `channel`, read
    /// with the TOML parser): one `::error::` line naming both versions when they differ or the channel floats.
    RustPin(ci_rust_pin::RustPinArgs),
    /// ci.yml's job model (ADR 0123 §7): `check` builds it and fails on a job that breaks the conventions or a `render` that is not injective
    /// over the legs the planner emits. Report-only (warnings, exit 0) until its first green run is recorded; the plan job and the member gate run it.
    JobModel(ci_job_model::JobModelArgs),
    /// Dyno's release step 2c in CI (ported from the shell script of chore/dyno-linux-units-on-ci, which never landed): the benchmark-unit tests (`cargo test -p
    /// dyno-agent --test bench_units -- --ignored`) in a systemd user manager. No manager is exit 6, not measured, never a pass; failing
    /// tests are exit 1.
    DynoLinuxUnits(ci_dyno_linux_units::DynoLinuxUnitsArgs),
    /// CARGO_BUILD_JOBS and NEXTEST_TEST_THREADS for the job: the runner's share of the host (the port of
    /// conformance/ci/build-jobs.sh). Assignments to $GITHUB_ENV (or standard output without it), the reason on standard error.
    BuildJobs(ci_build_jobs::BuildJobsArgs),
    /// PADDOCK_BUILD_COMMIT and PADDOCK_BUILD_DATE for the About page (the port of conformance/ci/build-env.mjs): appended to
    /// $GITHUB_ENV and printed. Never fails.
    BuildEnv(ci_build_env::BuildEnvArgs),
    /// The runner-local store of `./dev`'s own binary on a Linux runner (user decision 109): `publish` keeps the verified tool by its
    /// hash in a folder of the `/sccache` volume (the plan job's step; atomic, locked, size-limited, evicted). Never fails the job.
    DevToolStore(ci_dev_tool_store::StoreArgs),
    /// The gate executor (ADR 0038 §1): the commands of one phase of a project's `paddock.ci.json`, one per line, a `cd` holds for the
    /// lines after it, each logged with its duration and exit; the port of conformance/ci/run-commands.sh for every project (an app
    /// under apps/, or crates, ui, conformance, tools, icons, eslint). `--list` prints the lines and runs nothing.
    Run(ci_run::RunArgs),
    #[command(flatten)]
    Host(ci_host::HostStep),
    /// Release mode (ADR 0085): a tag build reads nothing kept. `setup` makes the job's folders, `record` is the only
    /// ledger writer, `verify` proves it.
    ReleaseIsolation {
        #[command(subcommand)]
        command: release_isolation::Command,
    },
    /// The arguments of CI's conformance line (`--app paddock ... --only ...`, after `node conformance/bin/paddock-conformance.mjs`... as
    /// one line), read from .gitea/workflows/ci.yml by the one reader the push gate uses (ADR 0098 WP-2). Exit 6 when the
    /// workflow has no single such line.
    ConformanceArgs(ConformanceArgsArgs),
    /// For each failed test of a finished run: NEW, ALREADY FAILING ON MAIN (the last main run of the same workflow) or FLAKY (its
    /// flake history). UNKNOWN when nothing proves either way. Read-only: it never quarantines or marks anything green (ER-12).
    Triage(ci_runs::TriageArgs),
    /// Record finished runs as test fixtures (run, jobs, failed-job logs), with people, e-mail addresses, avatars and runner names
    /// replaced by invented values on capture. Conformance C110 fails a fixture that holds a real one.
    RecordFixture(ci_runs::RecordArgs),
    /// The cost classes of paddock-dev's tests against what they took (ADR 0121): per class the longest test, and every unclassed test
    /// over a quarter of its stop, from a run's job logs or a saved nextest output. Report-only (exit 0) unless --fail-on-findings (5);
    /// 6 when no whole nextest run of paddock-dev was read.
    TestCost(ci_runs::TestCostArgs),
    /// Cancels one CI run by the id `./dev ci dispatch` printed, through that run's own cancel URL (never "the newest run", ER-51). A
    /// cancelled run reads as "cancelled: not measured". Exit 0 sent or already finished, 6 not measured.
    Cancel(ci_runs::CancelArgs),
    /// May the commit land (ER-49)? Needs the complete green CI run AND the green local slow rows (`./dev check --pre-push --member-gate
    /// --set slow`) on the same commit. Exit 0 landable, 5 refused, 6 not held (an unfinished or missing half is never a pass).
    LandingHeld(ci_runs::LandingHeldArgs),
    /// Is there a full green run of a workflow on a commit? The one reader of the runs for that verdict (the tag workflow asks
    /// it). A run counts if it is a push to main or its plan job logged `plan mode: full=true`; the newest full run decides.
    /// Exit 0 full green (with `--landing` also green with fix items: FLAKY tests in untouched code), 5 red, only partial runs, or
    /// fix items without `--landing` (a tag needs a run without them), 6 not measured (never a pass).
    Verdict(ci_runs::VerdictArgs),
    /// What the member's own trial run says (the run `./dev merge sync-member` dispatched with `ci dispatch --trial`): `green` (exit 0),
    /// `red` (5), `running`, `no-run` or `not-measured` (6, never a pass). Read by `merge check`.
    TrialVerdict(ci_runs::TrialVerdictArgs),
    /// Records the fix items of a run that is green with fix items (FLAKY tests of crates and apps the change did not reach, ER-12) in
    /// docs/lessons.md, once whatever the number of calls: the call a landing makes on the landed commit, the queue's or an
    /// integrator's by hand. Exit 0 recorded or already recorded (or nothing to record on a plain green), 5 red or partial, 6 not measured.
    FixItems(ci_runs::FixItemsArgs),
    /// The cache keys of the last runs that no job ever saved, read from the `cache-report:` lines of the job logs (ADR 0115).
    /// Report-only (exit 0) unless `--fail-on-never-saved` (5); no report line in the window exits 6, check broken.
    CacheAudit(ci_runs::CacheAuditArgs),
    /// The test-result cache (ADR 0116): `run` the nextest of a Cargo workspace with each unchanged package's pass reused, `lookup` what
    /// would be reused, or `audit` the local store. Shadow mode (report-only) until docs/test-cache.toml records two green train runs.
    TestCache(test_cache::cmd::TestCacheArgs),
    /// Whether a scheduled run ends early because a train's dispatch run is queued or running (skip=true|false to
    /// GITHUB_OUTPUT). Fails open: an unreadable API lets the run proceed, with a warning.
    ScheduleSkip(ci_runs::ScheduleSkipArgs),
    /// The scope line of an app's own workflow (`plan scope: {json}`, ADR 0113): what the run did and what a push to main does, which
    /// `ci verdict` compares. ci.yml's plan prints its own (`dev plan --ci`).
    Scope(ci_runs::ScopeArgs),
    /// Records the first green run of a scope producer in docs/ci-scope-first-runs.toml, which switches `ci verdict` from the producer's
    /// old line to its declared scope (ER-41, ADR 0113). Refuses a run that is not green, complete and run.
    ScopeRecord(ci_runs::ScopeRecordArgs),
    /// Whether a push to main ends early because a full green run of the workflow, with no FLAKY test, already tested this exact
    /// commit (`reuse=true|false` and `run=<id>` to GITHUB_OUTPUT; ER-49). Fails open: anything unreadable lets the run proceed.
    Reuse(ci_runs::ReuseArgs),
    /// Dispatch a workflow once no other run is on its single-machine runner (macOS, Windows): waits like `wait --ci-run`, at
    /// most --max, then ends with an `::error::` naming the busy run. Linux-only workflows dispatch at once, and so does a train
    /// (`train/*`) whose workflow waits in its own first job (the runner-wait action), so scheduled runs see it queued.
    Dispatch(ci_runs::DispatchArgs),
    /// The first job of a train run (the runner-wait action): wait, at most --max, until no older run has an unfinished job on this run's
    /// single-machine runner (macOS, Windows). Fails open when the runs cannot be read; ends with an `::error::` at --max.
    RunnerWait(ci_runs::RunnerWaitArgs),
    /// Delete the `trial/*` refs (`ci dispatch --trial`) of branches that are gone or landed on main (ADR 0098 WP-4).
    TrialPrune(ci_runs::TrialPruneArgs),
    /// In a failed-only round's test step: every failed test the rerun filter names must select a test at this commit (a renamed or
    /// removed one fails with an `::error::` naming it, never a warning; ER-10, ER-33). Exit 5 with the names.
    RerunCheck(ci_runs::RerunCheckArgs),
    /// Records a dispatch guard's first green use (failed-first, green-main) in docs/ci-dispatch-guards.toml, which switches it from
    /// report-only to blocking (ER-41). Refuses a run that is not a green train run dispatched through `ci dispatch`.
    GuardRecord(ci_runs::GuardRecordArgs),
}

#[derive(Subcommand)]
enum SlotCommand {
    /// Take the first free slot of 1..=N and keep it until the owner folder is gone or `release` is called. Prints sN.
    Acquire(slots::AcquireArgs),
    /// Let a slot go and wait until it is free.
    Release(slots::ReleaseArgs),
    /// Show which slots are held.
    Status(slots::StatusArgs),
    /// Check that this job still holds its slot (the lock file is the one it locked, its holder lives; a lost lane is an error).
    Check(slots::CheckArgs),
    /// Run a command only while a slot is idle, holding its lock until the command ends (skips a busy slot).
    IdleRun(slots::IdleRunArgs),
    /// The detached holder that `acquire` starts. Not for people.
    #[command(hide = true)]
    Hold(slots::HoldArgs),
}

#[derive(Subcommand)]
enum MutantsCommand {
    /// Run cargo-mutants over the Rust functions changed in `tools/paddock-dev` and `crates` since `--base`, after its positive control.
    Gate(MutantsArgs),
    /// Count the mutants of the diff since `--base` and plan the shards for them (`run`, `count`, `shards` to GITHUB_OUTPUT): mutants.yml's plan job.
    Plan(MutantsPlanArgs),
    /// Print the pinned cargo-mutants version (`mutants_gate::PIN`): the one definition mutants.yml installs it from.
    Pin,
    /// The record job of mutants.yml: per planned shard, whether it measured all of its slice, part or nothing (read from each shard job's
    /// result line through the Actions API; token PLAN_TOKEN or GITEA_TOKEN). Exit 0 whatever it finds (report-only).
    Record(MutantsRecordArgs),
}

#[derive(Args)]
struct MutantsRecordArgs {
    /// The run whose shard jobs are read (default: GITHUB_RUN_ID).
    #[arg(long)]
    run: Option<u64>,
    /// The base of the run's diff.
    #[arg(long)]
    base: String,
    /// The run's head commit.
    #[arg(long)]
    head: String,
    /// The plan's mutant count.
    #[arg(long)]
    count: usize,
    /// The plan's slice count (`n` of every `k/n`).
    #[arg(long)]
    slices: usize,
    /// The plan's shards, its JSON list of `k/n` (the matrix of mutants.yml).
    #[arg(long)]
    shards: String,
}

#[derive(Args)]
struct MutantsPlanArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// The commit the diff is measured from (the previous main tip).
    #[arg(long)]
    base: String,
}

#[derive(Args)]
struct MutantsArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// The commit the change is measured from (the merge base).
    #[arg(long)]
    base: String,
    /// Test only this slice of the diff's mutants, `k/n` with k below n (cargo-mutants `--shard`): one leg of mutants.yml's matrix.
    #[arg(long)]
    shard: Option<String>,
}

#[derive(Subcommand)]
enum LocksCommand {
    /// Regenerate them (ER-40: never by hand): cargo hakari (manage-deps, generate), `cargo update --workspace`, `npm install --package-lock-only`.
    Sync(LocksArgs),
    /// The shared-crate dependency guard: the workspace hack, the resolved features of the watched crates (docs/shared-crate-features.toml)
    /// and every Cargo.lock agree with the manifests. Report-only for now (warnings, exit 0). `--write` regenerates the pins file.
    Guard(GuardArgs),
}

#[derive(Args)]
struct GuardArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// Rewrite the pins file from what Cargo resolves now (keeps the watch list).
    #[arg(long)]
    write: bool,
}

#[derive(Args)]
struct LocksArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Subcommand)]
enum SccacheCommand {
    /// Print `export` lines for the shell: `eval "$(./dev sccache env)"`.
    Env,
}

#[derive(Subcommand)]
enum GatesCommand {
    /// Print the CI model: every command CI can run, with its folder, levels and status (for debugging C100).
    Model(GatesRootArgs),
    /// C100: every check the repository defines is gated by a reachable CI line, or registered with a reason.
    Check(GatesCheckArgs),
    /// Every check with its gating (use --json).
    List(GatesListArgs),
    /// The globs whose files change C100's answer (the planner's inputs).
    Inputs,
    /// Refuse a release tag while the lint-adoption ratchets of its app are not empty (nothing else of C100 can refuse it).
    Ratchet(GatesRatchetArgs),
}

#[derive(Args)]
struct GatesRatchetArgs {
    /// The repository root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// The release tag about to be made, `<app>-v<major>.<minor>.<patch>[-pre]`.
    #[arg(long)]
    release_tag: String,
    /// The revision the register may only shrink against.
    #[arg(long, default_value = "origin/main")]
    base: String,
}

#[derive(Args)]
struct GatesCheckArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// A release tag about to be made (debrief-v1.3.0): refused while a ratchet's target release is not met.
    #[arg(long)]
    release_tag: Option<String>,
    /// Print every finding but exit 0 (the rollback of ADR 0060: the model counts a --warn line as report-only).
    #[arg(long)]
    warn: bool,
    /// The revision the register may only shrink against.
    #[arg(long, default_value = "origin/main")]
    base: String,
}

#[derive(Args)]
struct GatesListArgs {
    /// The repository root.
    #[arg(long)]
    root: Option<PathBuf>,
    /// The revision the register is compared with.
    #[arg(long, default_value = "origin/main")]
    base: String,
}

#[derive(Args)]
struct GatesRootArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Args)]
struct PlanArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// The planner's own arguments, as the retired scripts/affected.mjs took them.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

/// Runs the tool: `args` are the command line without the program name. Returns the exit code.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    // What `./dev --print-binary` answers (the wrapper prints its cached binary): this binary's own path, so the CI kit can pin
    // whichever of the two `PADDOCK_DEV` names.
    if args == ["--print-binary"] {
        return match std::env::current_exe() {
            Ok(p) => {
                let _ = writeln!(out, "{}", p.display());
                Exit::Ok.code()
            }
            Err(e) => finish(
                Format::Text,
                Err(CliError::failure(format!("the program's own path is unknown: {e}."))),
                out,
                err,
            )
            .code(),
        };
    }
    let format = if args.iter().any(|a| a == "--json") {
        Format::Json
    } else {
        Format::Text
    };
    let argv = std::iter::once("dev".to_string()).chain(args.iter().cloned());
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(e) if !e.use_stderr() => {
            let _ = write!(out, "{e}");
            return Exit::Ok.code();
        }
        Err(e) => {
            let message = e.render().to_string();
            let first = message.lines().next().unwrap_or("").trim_start_matches("error: ").to_string();
            let hint = "Run `./dev --help` for the commands.";
            return finish(format, Err(CliError::usage(first).with_hint(hint)), out, err).code();
        }
    };
    if cli.json && matches!(cli.command, Command::Plan(_)) {
        let e = CliError::usage("`dev plan` has no --json mode.")
            .with_hint("Its output is the planner's own: JSON for a plan, true or false, a range.");
        return finish(Format::Json, Err(e), out, err).code();
    }
    if let Command::Conformance { args } = &cli.command {
        // The conformance host's contract: its own text or JSON on standard output, `::error::` lines, exit 0, 1, 2, 6 or 7.
        return check::conformance::run(args, out, err, &|k| std::env::var(k).ok());
    }
    if let Command::Ci {
        command: CiCommand::Isolation(a),
    } = &cli.command
    {
        // The script's contract: plain values on standard output, `::error::` lines on standard error, no `error:` prefix.
        return ci_isolation::run(a, out, err);
    }
    if let Command::Ci {
        command: CiCommand::SetupRust(a),
    } = &cli.command
    {
        // A CI step's contract: plain lines (the shell's), one `::error::` line on failure, no report.
        return ci_setup_rust::run(a, out, err);
    }
    if let Command::Ci {
        command: CiCommand::S3Guard { command },
    } = &cli.command
    {
        // The guard is a long-running process with its own contract: one `PORT <n>` line, then silence.
        return ci_s3_guard::run(command, out, err);
    }
    if let Command::Ci {
        command: CiCommand::BuildJobs(a),
    } = &cli.command
    {
        // A CI step's contract: plain lines (the shell's), one `::error::` line on failure, no report.
        return ci_build_jobs::run(a, out, err);
    }
    if let Command::Ci {
        command: CiCommand::BuildEnv(a),
    } = &cli.command
    {
        return ci_build_env::run(a, out, err);
    }
    if let Command::Ci {
        command: CiCommand::DevToolStore(a),
    } = &cli.command
    {
        // A CI step's contract: plain lines, one `::warning::` on a problem, exit 0.
        return ci_dev_tool_store::run(a, &|k| std::env::var(k).ok(), out, err);
    }
    if let Command::Ci {
        command: CiCommand::Run(a),
    } = &cli.command
    {
        // A CI step's contract: the commands' own output, `::group::` and one `::error::` line on failure, the failing line's exit code.
        return ci_run::run(a, out, err);
    }
    if let Command::Disk {
        command: disk::DiskCommand::Check(a),
    } = &cli.command
    {
        // CI annotations (`::warning::`, `::error::`) on standard error and the exit code: like `ci isolation`, no report.
        return disk::check::run(a, &|k| std::env::var(k).ok(), out, err);
    }
    if let Command::Ci {
        command: CiCommand::Host(step),
    } = &cli.command
    {
        // A CI step's contract: plain lines (the shell's), one `::error::` line on failure, no report.
        return ci_host::run(step, out, err);
    }
    if matches!(cli.command, Command::Hook { .. }) {
        // The hook's answer is its own JSON on standard output, not a report, and its exit code is always 0.
        return hook::pre_tool_use(&mut std::io::stdin(), out, err);
    }
    let command = match cli.command {
        Command::Merge {
            command: MergeCommand::Check(a),
        } if cli.json => {
            return merge_check::command_json(
                &merge_check::Request {
                    root: a.root,
                    commit: a.commit,
                    branch: a.branch,
                    target: a.target,
                    generators: !a.no_generators,
                    heavy_generators: a.heavy_generators,
                    gates: a.gates,
                    write_reverted: a.write_reverted,
                    old_base_anyway: a.old_base_anyway,
                    no_trial: a.no_trial,
                    gates_base: a.gates_base,
                },
                out,
                err,
            );
        }
        other => other,
    };
    let result = dispatch(command);
    finish(if cli.json { Format::Json } else { Format::Text }, result, out, err).code()
}

fn dispatch(command: Command) -> Result<Report, CliError> {
    match command {
        Command::SelfCmd {
            command: SelfCommand::Hash,
        } => input_hash::command(),
        Command::Ci {
            command: CiCommand::Slot { command },
        } => match command {
            SlotCommand::Acquire(a) => slots::acquire(&a),
            SlotCommand::Release(a) => slots::release(&a),
            SlotCommand::Status(a) => slots::status(&a),
            SlotCommand::Check(a) => slots::check(&a),
            SlotCommand::IdleRun(a) => slots::idle_run(&a),
            SlotCommand::Hold(a) => slots::hold(&a),
        },
        Command::Ci {
            command: CiCommand::Isolation(_),
        } => Err(CliError::failure("`ci isolation` was dispatched through the report path.")),
        Command::Ci {
            command: CiCommand::SetupRust(_),
        } => Err(CliError::failure("`ci setup-rust` was dispatched through the report path.")),
        Command::Ci {
            command: CiCommand::S3Guard { .. },
        } => Err(CliError::failure("`ci s3-guard` was dispatched through the report path.")),
        Command::Ci {
            command: CiCommand::BuildJobs(_),
        } => Err(CliError::failure("`ci build-jobs` was dispatched through the report path.")),
        Command::Ci {
            command: CiCommand::BuildEnv(_),
        } => Err(CliError::failure("`ci build-env` was dispatched through the report path.")),
        Command::Ci {
            command: CiCommand::DevToolStore(_),
        } => Err(CliError::failure("`ci dev-tool-store` was dispatched through the report path.")),
        Command::Ci {
            command: CiCommand::Run(_),
        } => Err(CliError::failure("`ci run` was dispatched through the report path.")),
        Command::Conformance { .. } => Err(CliError::failure("`conformance` was dispatched through the report path.")),
        Command::Ci {
            command: CiCommand::Host(_),
        } => Err(CliError::failure("a `ci` host step was dispatched through the report path.")),
        Command::Ci {
            command: CiCommand::ReleaseIsolation { command },
        } => release_isolation::command(command),
        Command::Ci {
            command: CiCommand::RecordFixture(a),
        } => ci_runs::record::command(&a),
        Command::Ci {
            command: CiCommand::ConformanceArgs(a),
        } => crate::repo_root::repo_root(a.root.as_deref()).and_then(|root| {
            // The one reader is the contract, `conformance/paddock.ci.json` (ADR 0038 section 1), which replaced the line in ci.yml.
            let read = if a.host == "rust" {
                crate::ci_run::contract::conformance_rust_args_at(&root)
            } else {
                crate::ci_run::contract::conformance_args_at(&root)
            };
            let args = read.map_err(|why| {
                paddock_cli::CliError::new(
                    paddock_cli::Exit::NotMeasured,
                    format!(
                        "::error::conformance args NOT MEASURED: {why}. Next step: fix the `conformance` field of conformance/paddock.ci.json, then run it again."
                    ),
                )
            })?;
            Ok(paddock_cli::Report::new(serde_json::json!({ "args": args }), args.join(" ")))
        }),
        Command::Ci {
            command: CiCommand::Triage(a),
        } => ci_runs::triage(&a),
        Command::Ci {
            command: CiCommand::TestCost(a),
        } => ci_runs::test_cost(&a),
        Command::Ci {
            command: CiCommand::ScheduleSkip(a),
        } => ci_runs::schedule_skip(&a),
        Command::Ci {
            command: CiCommand::Reuse(a),
        } => ci_runs::reuse(&a),
        Command::Ci {
            command: CiCommand::Scope(a),
        } => ci_runs::scope(&a),
        Command::Ci {
            command: CiCommand::ScopeRecord(a),
        } => ci_runs::scope_record(&a),
        Command::Ci {
            command: CiCommand::RustPin(a),
        } => ci_rust_pin::command(&a),
        Command::Ci {
            command: CiCommand::JobModel(a),
        } => ci_job_model::command(&a),
        Command::Ci {
            command: CiCommand::DynoLinuxUnits(a),
        } => ci_dyno_linux_units::command(&a),
        Command::Ci {
            command: CiCommand::TrialVerdict(a),
        } => ci_runs::trial_verdict(&a),
        Command::Ci {
            command: CiCommand::Cancel(a),
        } => ci_runs::cancel_cmd(&a),
        Command::Ci {
            command: CiCommand::LandingHeld(a),
        } => ci_runs::landing_held(&a),
        Command::Ci {
            command: CiCommand::Verdict(a),
        } => ci_runs::verdict(&a),
        Command::Ci {
            command: CiCommand::FixItems(a),
        } => ci_runs::fix_items_cmd(&a),
        Command::Ci {
            command: CiCommand::CacheAudit(a),
        } => ci_runs::cache_audit(&a),
        Command::Ci {
            command: CiCommand::TestCache(a),
        } => test_cache::cmd::command(&a),
        Command::Ci {
            command: CiCommand::Dispatch(a),
        } => ci_runs::dispatch(&a),
        Command::Ci {
            command: CiCommand::RunnerWait(a),
        } => ci_runs::runner_wait(&a),
        Command::Ci {
            command: CiCommand::TrialPrune(a),
        } => ci_runs::trial_prune(&a),
        Command::Ci {
            command: CiCommand::RerunCheck(a),
        } => ci_runs::rerun_check(&a),
        Command::Ci {
            command: CiCommand::GuardRecord(a),
        } => ci_runs::guard_record(&a),
        Command::Wait(a) => wait::command(&a),
        Command::Throttle(a) => heavy_cmd::command(&a),
        Command::ThrottleInstall(a) => heavy_install::command(&a),
        Command::Load(a) => load::command(&a),
        Command::Places(a) => places::command(&a),
        Command::Audit { command } => audit::command(command),
        Command::Mutants {
            command: MutantsCommand::Gate(a),
        } => mutants_gate::command_gate(a.root, &a.base, a.shard.as_deref()),
        Command::Mutants {
            command: MutantsCommand::Plan(a),
        } => mutants_gate::command_plan(a.root, &a.base),
        Command::Mutants {
            command: MutantsCommand::Record(a),
        } => mutants_gate::record::command_record(a.run, &a.base, &a.head, a.count, a.slices, &a.shards),
        Command::Mutants {
            command: MutantsCommand::Pin,
        } => Ok(Report::new(serde_json::json!({"pin": mutants_gate::PIN}), mutants_gate::PIN)),
        Command::Locks {
            command: LocksCommand::Sync(a),
        } => locks::sync(a.root),
        Command::Locks {
            command: LocksCommand::Guard(a),
        } => locks_guard::command_guard(a.root, a.write),
        Command::Gates {
            command: GatesCommand::Model(a),
        } => gates::command::model(a.root.as_deref()),
        Command::Gates {
            command: GatesCommand::Check(a),
        } => gates::command::check(
            &gates::command::CheckArgs {
                root: a.root,
                release_tag: a.release_tag,
                warn: a.warn,
                base: a.base,
                stage: crate::check::growth_approval::stage_from_env(&|k| std::env::var(k).ok()).map_err(CliError::usage)?,
            },
            dev_command_paths(),
        ),
        Command::Gates {
            command: GatesCommand::List(a),
        } => gates::command::list(a.root.as_deref(), &a.base, dev_command_paths()),
        Command::Gates {
            command: GatesCommand::Inputs,
        } => gates::command::inputs(),
        Command::Gates {
            command: GatesCommand::Ratchet(a),
        } => gates::command::ratchet(a.root.as_deref(), &a.base, &a.release_tag),
        Command::Check {
            command:
                CheckCommand::Run {
                    only,
                    files,
                    root,
                    app,
                    out,
                },
        } => check::command(check::Args {
            only,
            files,
            root,
            app,
            out,
        }),
        Command::Check {
            command: CheckCommand::PrePush(a),
        } => pre_push::command(&a),
        Command::Check {
            command: CheckCommand::NewId { reserve, root },
        } => check_ids::command(root, reserve),
        Command::Check {
            command: CheckCommand::IpcEndpoints { tag, root },
        } => ipc_endpoints::command(root, tag),
        Command::Check {
            command: CheckCommand::TestFeatures { app, root },
        } => test_features::command(root, app),
        Command::Worktree { command } => worktree::command(command),
        Command::Deps { command } => deps_cache::command(command),
        Command::Sccache {
            command: SccacheCommand::Env,
        } => {
            let env: std::collections::HashMap<String, String> = crate::process_env::vars().collect();
            for w in local_sccache::shell_warnings(&env) {
                eprintln!("{w}");
            }
            let ready = local_sccache::prepare_and_say(&env, local_sccache::Install::OnDemand);
            if let Some(r) = &ready {
                local_sccache::say_machine_config(&env, r);
            }
            let lines = ready.map(|r| local_sccache::shell_lines(&r)).unwrap_or_default();
            Ok(Report::new(serde_json::json!({ "lines": lines }), lines.join("\n")))
        }
        Command::Disk { command } => disk::command(command),
        Command::Pr { command } => pr::command(command),
        Command::Queue { command } => queue::command(command),
        Command::Merge {
            command: MergeCommand::Check(a),
        } => merge_check::command(&merge_check::Request {
            root: a.root,
            commit: a.commit,
            branch: a.branch,
            target: a.target,
            generators: !a.no_generators,
            heavy_generators: a.heavy_generators,
            gates: a.gates,
            write_reverted: a.write_reverted,
            old_base_anyway: a.old_base_anyway,
            no_trial: a.no_trial,
            gates_base: a.gates_base,
        }),
        Command::Merge {
            command: MergeCommand::SyncMember(a),
        } => merge_check::sync_member::command(
            a.root.as_deref(),
            &a.train,
            a.member.as_deref(),
            &merge_check::sync_member::Options {
                no_static: a.no_static.as_deref(),
                no_trial: a.no_trial.as_deref(),
                trial_max: &a.trial_max,
                relands: &a.reland,
            },
        ),
        Command::Merge {
            command: MergeCommand::Member(a),
        } => merge_check::member::command(
            a.root.as_deref(),
            &a.member,
            &merge_check::member::Options {
                relands: &a.reland,
                old_base_anyway: a.old_base_anyway.as_deref(),
            },
        ),
        Command::Merge {
            command: MergeCommand::LessonsDriver { base, ours, theirs },
        } => lessons_driver::command(&base, &ours, &theirs),
        Command::Gen {
            command: GenCommand::Check(a),
        } => generators::command(&generators::CheckArgs {
            root: a.root,
            all: a.all,
            only: a.only,
            files: a.files,
            base: a.base,
            priority: a.priority,
        }),
        Command::Plan(a) => plan_cmd::command(&a.args, a.root),
        Command::Hook { .. } => Ok(Report::new(serde_json::json!({}), String::new())),
        Command::Hooks {
            command: HooksCommand::Install(a),
        } => hook::install::command(&a),
    }
}

/// Every visible subcommand path of `./dev` (`ci run`, `gates check`, ...), read from the command tree this module defines: what the gate
/// register's `dev` rows are checked against. It lives here because only `cli` may name `Cli` without importing every command
/// the other way round (`gates::command` is handed the set, so conformance C155 sees no edge from it to `cli`).
fn dev_command_paths() -> std::collections::BTreeSet<String> {
    use clap::CommandFactory;
    fn walk(c: &clap::Command, prefix: &str, out: &mut std::collections::BTreeSet<String>) {
        for s in c.get_subcommands().filter(|s| !s.is_hide_set()) {
            let path = if prefix.is_empty() {
                s.get_name().to_string()
            } else {
                format!("{prefix} {}", s.get_name())
            };
            out.insert(path.clone());
            walk(s, &path, out);
        }
    }
    let mut out = std::collections::BTreeSet::new();
    walk(&Cli::command(), "", &mut out);
    out
}
