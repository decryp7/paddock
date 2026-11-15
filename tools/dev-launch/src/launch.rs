//! The launcher: finds the `paddock-dev` binary for the checkout's input hash and runs it with the caller's arguments.
//!
//! Order: the per-user cache (on a CI runner the kept mount), the prebuilt download (on a CI runner by default, elsewhere with
//! `PADDOCK_DEV_DOWNLOAD=1`), then a local build. A build also happens whenever an input has uncommitted changes, because then
//! HEAD's hash cannot describe it. Environment: `PADDOCK_DEV_TARGET_DIR` (build folder, default `tools/paddock-dev/target`),
//! `PADDOCK_DEV_HOME` (cache, default `~/.paddock-dev`), `PADDOCK_CI_MOUNT` (a CI runner's kept mount, default `/sccache`),
//! `PADDOCK_DEV_FRESH=1` (build from source in a fresh folder without a compiler wrapper, as release jobs do), and the download
//! settings of [`crate::download`]. `PADDOCK_DEV_DOWNLOAD=0` (release and tag jobs) keeps its tools in `bin-source`, which the
//! download path never writes.

use crate::cache;
use crate::download::{self, Fetch};
use crate::hash::{self, HashError};
use crate::proc;
use crate::toolchain::{self, Ensure};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

fn env_var(k: &str) -> Option<String> {
    std::env::var_os(k).map(|v| v.to_string_lossy().into_owned())
}

/// A line on standard output. A reader that went away (`./dev --print-binary | head -1`) is no error: the shell's `echo` ended
/// quietly on SIGPIPE, and `println!` would panic.
fn say(line: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stdout(), "{line}");
}

fn env_non_empty(k: &str) -> Option<String> {
    env_var(k).filter(|v| !v.is_empty())
}

fn home_dir() -> String {
    env_non_empty("HOME")
        .or_else(|| env_non_empty("USERPROFILE"))
        .unwrap_or_else(|| "/nonexistent".to_string())
}

/// Runs a program for the hash: a bounded run whose failure is the error.
fn run_for_hash(program: &str, args: &[&str], cwd: &Path, stdin: Option<&[u8]>) -> Result<String, HashError> {
    let mut c = proc::command(program);
    c.args(args).current_dir(cwd);
    let out = proc::capture(c, Duration::from_secs(60), stdin);
    if out.ok() {
        return Ok(out.stdout);
    }
    let why = if out.code == proc::TIMED_OUT {
        "it did not finish within 60 seconds".to_string()
    } else {
        out.stderr.trim().to_string()
    };
    Err(HashError::new(format!("{program} {}: {why}", args.first().unwrap_or(&"")))
        .with_hint("Check that git and rustc work in this repository."))
}

/// `rustc -vV` of the compiler cargo will use, run in the tool's folder, with carriage returns dropped; `None` when it does not
/// run or prints nothing.
fn rustc_vv(rustc: &str, tool: &Path) -> Option<String> {
    let mut c = proc::probe(rustc);
    c.arg("-vV").current_dir(tool);
    let out = proc::capture(c, Duration::from_secs(60), None);
    let text = out.stdout.replace('\r', "");
    (out.ok() && !text.trim().is_empty()).then_some(text)
}

/// Where this launch is: the repository, the tool, the cache.
struct Ctx {
    root: PathBuf,
    tool: PathBuf,
    home: PathBuf,
    kept_target: bool,
    ci_mount: PathBuf,
    binroot: &'static str,
    restarted: bool,
    args: Vec<String>,
    /// The effective `PADDOCK_DEV_TARGET_DIR` (the CI home's on a host runner), else none.
    target_dir: Option<PathBuf>,
    /// Whether the tool's home was named (by the caller or the CI home), so nothing says "nothing is kept".
    home_set: bool,
    /// This run never downloads (`PADDOCK_DEV_DOWNLOAD=0`, or a CI job with no read token).
    no_download: bool,
    /// Variables the tool and what it starts see besides the caller's: the CI home's folders and a download switched off.
    extra_env: Vec<(&'static str, String)>,
}

/// The compiler as it was found and what was hashed with it.
struct Found {
    rustc: String,
    compiler: String,
    host: String,
    ckey: String,
    hash: String,
    flags: String,
    configs: Vec<PathBuf>,
    dirty: bool,
}

/// `explain_hash`: the inputs of the hash that are not the tracked files, one line, so a miss (a binary published under another
/// hash) can be explained from the logs alone (ER-27): the compiler release, the names (never the values) of the hashed
/// environment variables, and the digests of the cargo config files.
fn explain(f: &Found) {
    let flags = f.flags.trim_end_matches('\n');
    let mut names = String::new();
    for l in if flags.is_empty() { vec![""] } else { flags.split('\n').collect() } {
        if !l.starts_with("config-file ") {
            names.push_str(l.split('=').next().unwrap_or(l));
            names.push(' ');
        }
    }
    let configs: String = if f.configs.is_empty() {
        " ".to_string()
    } else {
        f.configs.iter().map(|c| format!("{} ", c.display())).collect()
    };
    let digests: String = flags
        .lines()
        .filter_map(|l| l.strip_prefix("config-file "))
        .map(|d| format!("{} ", &d[..d.len().min(8)]))
        .collect();
    eprintln!(
        "dev: hash {} inputs: {}| env: {names}| configs: {configs}| digests: {digests}",
        f.hash,
        f.compiler.trim_end_matches('\n').replace('\n', " ")
    );
}

/// Whether the executable file answers `--version` with some text (an empty or truncated file is not a tool).
fn runs(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0) {
            return false;
        }
    }
    #[cfg(not(unix))]
    if !path.is_file() {
        return false;
    }
    let mut c = proc::command(path);
    c.arg("--version");
    let out = proc::capture(c, Duration::from_secs(60), None);
    out.ok() && !out.stdout.trim().is_empty()
}

/// Cargo puts a binary built for an explicit target under `<target dir>/<triple>/`, not `release/`.
fn refuse_build_target() -> Result<(), i32> {
    if env_non_empty("CARGO_BUILD_TARGET").is_some() {
        eprintln!("dev: CARGO_BUILD_TARGET is set, so cargo would put the tool outside target/release. Unset it for ./dev.");
        return Err(2);
    }
    Ok(())
}

fn built_check(dir: &Path) -> Result<(), i32> {
    let bin = dir.join("release").join(format!("paddock-dev{EXE}"));
    if bin.is_file() {
        return Ok(());
    }
    eprintln!(
        "dev: cargo did not leave the tool at {}. A cargo config probably sets build.target; remove it for ./dev.",
        bin.display()
    );
    Err(2)
}

/// The `compiler` lines of the compiler as it is now (empty when it does not run), asked the way it was asked first.
fn compiler_now(f: &Found, tool: &Path) -> String {
    rustc_vv(&f.rustc, tool).map(|v| hash::compiler_lines(&v)).unwrap_or_default()
}

/// Build outcomes besides success: the compiler changed under the build (the caller starts over once) or the build failed (said aloud).
enum BuildEnd {
    Changed,
    Failed(i32),
}

impl Ctx {
    /// `build <target dir> [hash]`: the binary ends up in `<target dir>/release/paddock-dev`.
    fn build(&self, f: &Found, target: &Path, hash: &str) -> Result<(), BuildEnd> {
        refuse_build_target().map_err(BuildEnd::Failed)?;
        let mut c = proc::command("cargo");
        c.args(["build", "--release", "--locked", "--quiet"])
            .current_dir(&self.tool)
            .env("CARGO_TARGET_DIR", target)
            .env("PADDOCK_DEV_HASH", hash);
        // Never through the job's compiler wrapper: at release the job's sccache server is stopped and its temp folder removed, and a
        // wrapper that cannot start fails the build of the tool itself (runs 7773 to 7775). The tool is a content-addressed binary, not
        // cached by sccache. The tool's own first build needs a core limit before the job's CARGO_BUILD_JOBS exists (`ci build-jobs`
        // writes it): 4, the share of a runner on the shared Mac (docs/benchmarks.md, "Kit shim default"). Only for this build: the
        // tool and what it starts keep the job's own variables.
        for k in [
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "CARGO_BUILD_RUSTC_WRAPPER",
            "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
        ] {
            c.env_remove(k);
        }
        if env_var("GITHUB_ACTIONS").as_deref() == Some("true") && env_non_empty("CARGO_BUILD_JOBS").is_none() {
            c.env("CARGO_BUILD_JOBS", "4");
        }
        let code = proc::run_inherit(c);
        if code != 0 {
            self.toolchain_changed(f, code)?;
            eprintln!(
                "::error::./dev could not build paddock-dev (hash {hash}, cargo exit {code}, build folder {}). Run 'cargo build --release --locked' in tools/paddock-dev to see why; a stale tools/paddock-dev/Cargo.lock is the usual cause.",
                target.display()
            );
            return Err(BuildEnd::Failed(1));
        }
        self.toolchain_changed(f, 0)?;
        built_check(target).map_err(BuildEnd::Failed)
    }

    /// The compiler is the one the hash named, or the build is not trusted: a rustup update during the build (the toolchain folder
    /// is replaced in place) gives cargo errors that look like a broken build (E0514, a missing rustc.exe on Windows) and a
    /// binary the hash does not describe. Once per run the launcher starts over with the new compiler (a new hash, a new build
    /// folder); the second time it says so and fails.
    fn toolchain_changed(&self, f: &Found, code: i32) -> Result<(), BuildEnd> {
        let now = compiler_now(f, &self.tool);
        if now == f.compiler {
            return Ok(());
        }
        let flat = |s: &str| s.trim_end_matches('\n').replace('\n', " ");
        eprintln!(
            "::warning::./dev: the Rust compiler changed while paddock-dev was building (it was: {}; now: {}; cargo exit {code}), probably a rustup update on this host.",
            flat(&f.compiler),
            if now.is_empty() { "not found".to_string() } else { flat(&now) }
        );
        if !self.restarted {
            return Err(BuildEnd::Changed);
        }
        eprintln!(
            "::error::./dev: the Rust compiler changed again during the second build, so paddock-dev was not built. Wait for the toolchain update on this host to finish, then run ./dev again (see 'rustup show')."
        );
        Err(BuildEnd::Failed(1))
    }

    /// 75 starts the launcher over once (the compiler changed under the build), anything else ends it (the failure was said aloud).
    fn restart_or_fail(&self, end: BuildEnd) -> i32 {
        match end {
            BuildEnd::Changed => {
                eprintln!("dev: starting over once with the compiler that is installed now.");
                match std::env::current_exe() {
                    Ok(me) => {
                        let mut c = proc::command(me);
                        c.args(&self.args).env("PADDOCK_DEV_RESTARTED", "1");
                        proc::replace_with(c)
                    }
                    Err(e) => {
                        eprintln!("::error::./dev cannot find itself to start over: {e}. Run ./dev again.");
                        1
                    }
                }
            }
            BuildEnd::Failed(code) => code,
        }
    }

    fn start(&self, binary: &Path, hash: &str) -> i32 {
        let mut c = proc::command(binary);
        c.args(&self.args)
            .env("PADDOCK_DEV_WRAPPER_HASH", hash)
            .envs(self.extra_env.iter().map(|(k, v)| (*k, v)));
        proc::replace_with(c)
    }
}

/// `--install-toolchain <toolchain> [<components>] [<targets>]` (space-separated lists): the setup-rust action's install, before
/// anything else of the job (no repository or compiler needed). One install of a toolchain at a time per rustup home, checked
/// and never uninstalled, bounded by `PADDOCK_DEV_TOOLCHAIN_TIMEOUT` (default 900 s). On failure one `::error::` line with the
/// cause and the next step (ER-27), exit 1.
fn install_toolchain(args: &[String]) -> i32 {
    let name = args.get(1).cloned().unwrap_or_default();
    if name.is_empty() {
        eprintln!("::error::usage: ./dev --install-toolchain <toolchain> [<components>] [<targets>]");
        return 2;
    }
    let words = |i: usize| -> Vec<String> {
        args.get(i)
            .map(|s| s.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default()
    };
    let (comps, targets) = (words(2), words(3));
    let _ = std::fs::create_dir_all(toolchain::rustup_home());
    let held = match toolchain::lock(&name) {
        Ok(h) => h,
        Err(m) => {
            eprintln!("{m}");
            return 1;
        }
    };
    let result = toolchain::install_checked(&name, &comps, &|| toolchain::install_named(&name, &comps, &targets));
    drop(held);
    // Once per job, which rustup and which compiler it got: drift (a rustup self-update, a moved channel) shows in the log (ER-26).
    let first_line = |c: proc::Captured| c.stdout.lines().next().unwrap_or_default().to_string();
    let rv = {
        let mut c = proc::probe("rustup");
        c.arg("--version");
        first_line(proc::capture(c, Duration::from_secs(30), None))
    };
    let cv = {
        let mut c = proc::probe("rustc");
        c.arg("-V").env("RUSTUP_TOOLCHAIN", &name);
        first_line(proc::capture(c, Duration::from_secs(30), None))
    };
    eprintln!(
        "dev: {}; toolchain {name}: {}",
        if rv.is_empty() { "rustup --version printed nothing" } else { &rv },
        if cv.is_empty() { "rustc -V printed nothing" } else { &cv }
    );
    let Err((rc, why)) = result else { return 0 };
    let mut why = why;
    // Git Bash reports 127 for a Windows program that ended with an NTSTATUS crash code, not only for a missing one.
    if rc == 127 && env_var("RUNNER_OS").as_deref() == Some("Windows") {
        why.push_str(": rustup was found but Windows ended it with an NTSTATUS crash code, which Git Bash reports as 127");
    }
    eprintln!(
        "::error::rustup could not install the {name} toolchain ({why}). CI never uninstalls a toolchain, since other jobs may be using it. Next step: {}.",
        toolchain::repair_step(&name, &comps)
    );
    toolchain::toolchain_failed();
    1
}

/// The configs a cargo run for the tool reads (closest first), their texts, and the compiler named by the environment or a config.
fn read_configs(root: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let get = |k: &str| env_var(k);
    let files = hash::config_files(root, &get);
    let texts = files.iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect();
    (files, texts)
}

/// Runs the launcher with the command line's arguments (without the program name); returns the exit code.
pub fn run(args: Vec<String>) -> i32 {
    let restarted = env_var("PADDOCK_DEV_RESTARTED").is_some_and(|v| !v.is_empty());
    if args.first().map(String::as_str) == Some("--install-toolchain") {
        return install_toolchain(&args);
    }
    // The check setup-rust's toolchain step starts with (through the stage 0's `--job-setup`): no repository or compiler needed.
    if args.first().map(String::as_str) == Some("--require-rustup") {
        if toolchain::on_path("rustup") {
            return 0;
        }
        eprintln!(
            "::error::rustup is not installed on this runner. Next step: install it (https://rustup.rs) on the runner, then rerun the job."
        );
        return 1;
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let root = {
        let mut c = proc::command("git");
        c.args(["rev-parse", "--show-toplevel"]).current_dir(&cwd);
        let out = proc::capture(c, Duration::from_secs(60), None);
        let top = out.stdout.trim_end_matches(['\n', '\r']).to_string();
        if !out.ok() || top.is_empty() {
            eprintln!("dev: run this inside the Paddock repository.");
            return 2;
        }
        PathBuf::from(top)
    };
    if let Some(code) = run_pinned(&root, &args) {
        return code;
    }
    let tool = root.join("tools/paddock-dev");
    if !tool.join("inputs.txt").is_file() {
        eprintln!("dev: tools/paddock-dev is not in this checkout (a sparse checkout without it). Run: git sparse-checkout add tools");
        return 2;
    }
    let inputs = match std::fs::read_to_string(tool.join("inputs.txt"))
        .map_err(|e| HashError::new(e.to_string()))
        .and_then(|t| hash::parse_inputs(&t))
    {
        Ok(i) => i,
        Err(e) => {
            eprintln!("dev: {}", e.text());
            return 2;
        }
    };

    // On a CI runner with a kept mount (a Linux container's /sccache, which the node-caches action also uses) the tool and its build
    // folder live there, so a plan job compiles the tool once per hash and not on every run. An explicit PADDOCK_DEV_HOME or
    // PADDOCK_DEV_TARGET_DIR wins.
    let ci_mount = PathBuf::from(env_non_empty("PADDOCK_CI_MOUNT").unwrap_or_else(|| "/sccache".to_string()));
    let mut kept_target = false;
    let mut home = env_non_empty("PADDOCK_DEV_HOME").map(PathBuf::from);
    let mut target_dir = env_non_empty("PADDOCK_DEV_TARGET_DIR").map(PathBuf::from);
    let mut extra_env: Vec<(&'static str, String)> = Vec::new();
    let in_ci = env_var("GITHUB_ACTIONS").as_deref() == Some("true");
    if in_ci && writable_dir(&ci_mount) && home.is_none() {
        home = Some(ci_mount.join("paddock-dev"));
        // One build folder per hash: jobs of other hashes on the same mount never replace the binary this job just built.
        kept_target = target_dir.is_none();
    }
    // On a host runner (macOS, Windows) a CI job keeps the tool and its build folder in the CI home's `tools` folder, never in the
    // runner's own home: a Windows job folder must stay deletable while the slot's holder runs, and the release steps of the job must
    // find the tool after the job's temp folder is gone (runs 7773 to 7775). The CI home is the one `ci isolation` uses
    // (`PADDOCK_CI_HOME`, which must exist and be absolute, else `~/.paddock-ci`; tools/paddock-dev/src/ci_isolation/home.rs,
    // `ci_home`): it is read here because the tool that knows the rule is not built yet. An explicit `PADDOCK_DEV_HOME` wins.
    let runner_os = env_non_empty("RUNNER_OS");
    if in_ci && runner_os.as_deref().is_some_and(|os| os != "Linux") && home.is_none() {
        match ci_home() {
            Ok(ci) => {
                let tools = format!("{ci}/tools");
                home = Some(PathBuf::from(format!("{tools}/paddock-dev")));
                extra_env.push(("PADDOCK_DEV_HOME", format!("{tools}/paddock-dev")));
                if target_dir.is_none() {
                    let t = format!("{tools}/paddock-dev-target");
                    target_dir = Some(PathBuf::from(&t));
                    extra_env.push(("PADDOCK_DEV_TARGET_DIR", t));
                }
            }
            Err(code) => return code,
        }
    }
    // Without a read credential a CI job builds the tool from source and never takes a binary from the store (ADR 0022 section
    // 3.3): the download could only fail, and a source build keeps its own cache folder (bin-source) below.
    let mut no_download = env_var("PADDOCK_DEV_DOWNLOAD").as_deref() == Some("0");
    if in_ci
        && env_non_empty("PADDOCK_DEV_DOWNLOAD").is_none()
        && env_non_empty("PADDOCK_DEV_TOKEN")
            .or_else(|| env_non_empty("GITHUB_TOKEN"))
            .is_none()
    {
        no_download = true;
        extra_env.push(("PADDOCK_DEV_DOWNLOAD", "0".to_string()));
    }
    let home_set = home.is_some();
    let home = home.unwrap_or_else(|| PathBuf::from(home_dir()).join(".paddock-dev"));
    // A run that never downloads (PADDOCK_DEV_DOWNLOAD=0: release and tag jobs) keeps its tools in its own folder, bin-source,
    // which the download path never writes: what it runs was built from source by a run that also did not download.
    let binroot = if no_download { "bin-source" } else { "bin" };
    let ctx = Ctx {
        root,
        tool,
        home,
        kept_target,
        ci_mount,
        binroot,
        restarted,
        args,
        target_dir,
        home_set,
        no_download,
        extra_env,
    };

    let (config_paths, configs) = read_configs(&ctx.root);
    let named = hash::named_rustc(&env_var, &configs);
    let rustc = named.clone().unwrap_or_else(|| "rustc".to_string());

    if ctx.args.first().map(String::as_str) == Some("--ensure-toolchain") {
        // For setup steps: succeeds when the compiler runs (installed if need be).
        let env_only = env_non_empty("RUSTC").or_else(|| env_non_empty("CARGO_BUILD_RUSTC"));
        let rc = env_only.clone().unwrap_or_else(|| "rustc".to_string());
        if rustc_vv(&rc, &ctx.tool).is_some() {
            return 0;
        }
        if toolchain::ensure(&Ensure {
            tool: &ctx.tool,
            named_rustc: env_only.as_deref(),
        }) && rustc_vv(&rc, &ctx.tool).is_some()
        {
            return 0;
        }
        eprintln!(
            "::error::./dev --ensure-toolchain: no Rust compiler runs here. Install Rust with rustup (https://rustup.rs) or the toolchain rust-toolchain.toml pins, then run ./dev again."
        );
        return 2;
    }

    let vv = rustc_vv(&rustc, &ctx.tool).or_else(|| {
        if toolchain::ensure(&Ensure {
            tool: &ctx.tool,
            named_rustc: named.as_deref(),
        }) {
            rustc_vv(&rustc, &ctx.tool)
        } else {
            None
        }
    });
    let Some(vv) = vv else {
        // Without a compiler the hash cannot be computed, and no cached binary is run in its place: it may be older than this
        // commit's tool and lack the subcommand asked for (run 8034: it exited 2 on `ci build-jobs`). Fail closed (ER-33).
        eprintln!(
            "::error::./dev: no Rust compiler runs here ({rustc}), so the tool's hash cannot be computed and no cached paddock-dev is run in its place (it may lack this command). Install Rust with rustup (https://rustup.rs) or the toolchain rust-toolchain.toml pins (rustup toolchain install, in tools/paddock-dev), then run ./dev again. (The toolchain rust-toolchain.toml pins is not installed, or its compiler does not run.)"
        );
        return 2;
    };
    let compiler = hash::compiler_lines(&vv);
    if !compiler.lines().any(|l| l.starts_with("release:")) {
        eprintln!("dev: {rustc} -vV printed no release line, so it is not a Rust compiler this tool can identify.");
        return 2;
    }
    let host = vv.lines().find_map(|l| l.strip_prefix("host: ")).unwrap_or_default().to_string();
    // The compiler's own key (its commit, else its release): a build folder is never shared between two compilers, because cargo
    // keeps rlibs of the old one beside the new ones and a half-replaced toolchain reads them as E0514 (run 8027, pitwall-mac).
    let ckey = match compiler.lines().find_map(|l| l.strip_prefix("commit-hash: ")) {
        Some(c) if !c.is_empty() => c.chars().take(12).collect::<String>(),
        _ => compiler
            .lines()
            .find_map(|l| l.strip_prefix("release: "))
            .unwrap_or_default()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '_' })
            .collect(),
    };
    let vars: Vec<(String, String)> = std::env::vars_os()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
        .collect();
    let hashed = match hash::hash_inputs(&run_for_hash, &ctx.root, inputs, &compiler, &configs, &vars) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("dev: {}", e.text());
            return 1;
        }
    };
    let found = Found {
        rustc,
        compiler,
        host,
        ckey,
        hash: hashed.hash,
        flags: hashed.flags,
        configs: config_paths,
        dirty: hashed.dirty,
    };

    if ctx.args.first().map(String::as_str) == Some("--wrapper-hash") {
        explain(&found);
        say(&format!("{}\n{}", found.hash, if found.dirty { "dirty" } else { "clean" }));
        return 0;
    }
    launch(&ctx, &found)
}

/// A folder of its own for a fresh build, in the job's temp folder (`mktemp -d`): a release job's build leaves nothing in the checkout.
#[expect(
    clippy::disallowed_methods,
    reason = "no Paddock scratch helper exists before the launcher has run; the folder is removed after the build"
)]
fn fresh_dir() -> PathBuf {
    std::env::temp_dir().join(download::unique("paddock-dev-fresh-"))
}

/// Whether two paths name one file (the same program reached by two names).
#[expect(
    clippy::disallowed_methods,
    reason = "std only: paddock_env::winpath does not exist before the launcher has run; both sides take the same path form"
)]
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// One tool per job: when the job's first step pinned the tool (`ci isolation acquire` writes `PADDOCK_DEV` to the job's environment),
/// a `ci` command in CI runs that binary as it is, with no hash and no build. The hash covers files the job itself writes, so at the
/// job's end (the release steps, after the job's temp folder is gone) ./dev would want another build (runs 7773 to 7775). The pin
/// never is this launcher or its stage 0 (a test may name it as `PADDOCK_DEV`): that case falls through. `None`: not pinned.
fn run_pinned(root: &Path, args: &[String]) -> Option<i32> {
    if env_var("GITHUB_ACTIONS").as_deref() != Some("true") || args.first().map(String::as_str) != Some("ci") {
        return None;
    }
    let pin = PathBuf::from(env_non_empty("PADDOCK_DEV")?);
    if !is_executable(&pin) || std::env::current_exe().is_ok_and(|me| same_file(&pin, &me)) || same_file(&pin, &root.join("dev")) {
        return None;
    }
    let mut v = proc::command(&pin);
    v.arg("--version");
    let out = proc::capture(v, Duration::from_secs(60), None);
    if !out.ok() || out.stdout.trim().is_empty() {
        return None;
    }
    let mut c = proc::command(&pin);
    c.args(args);
    Some(proc::replace_with(c))
}

/// The CI home a host runner's tool lives in, as a path both a shell and the tool read (forward slashes, no `\\?\\` prefix): see
/// the comment in [`run`]. `Err` is the exit code after the `::error::` line.
fn ci_home() -> Result<String, i32> {
    let home = if let Some(h) = env_non_empty("PADDOCK_CI_HOME") {
        let absolute = h.starts_with('/') || h.as_bytes().get(1) == Some(&b':') && matches!(h.as_bytes().get(2), Some(b'\\' | b'/'));
        if !absolute {
            eprintln!(
                "::error::PADDOCK_CI_HOME ({h}) is not an absolute path. Set it to the runner's CI home, for example /Volumes/ci/paddock-ci."
            );
            return Err(1);
        }
        if !Path::new(&h).is_dir() {
            eprintln!(
                "::error::PADDOCK_CI_HOME ({h}) does not exist: is its disk mounted? Mount it (or create the folder once by hand); the job stops here rather than fill the internal disk."
            );
            return Err(1);
        }
        PathBuf::from(h)
    } else {
        let h = PathBuf::from(home_dir()).join(".paddock-ci");
        if let Err(e) = std::fs::create_dir_all(&h) {
            eprintln!(
                "::error::./dev cannot make the CI home {}: {e}. Make it writable or set PADDOCK_CI_HOME, then run ./dev again.",
                h.display()
            );
            return Err(1);
        }
        h
    };
    mixed_path(&home).map_err(|why| {
        eprintln!("::error::./dev cannot use the CI home {} for the tool: {why}. Set PADDOCK_CI_HOME to a plain drive folder such as C:/ci/paddock-ci, then run ./dev again.", home.display());
        1
    })
}

/// The folder with links resolved (a SUBST or mapped drive too: `canonicalize` follows them), in the mixed form (`C:/ci/x`,
/// `//server/share/x`) that bash and the native tool both read. As `paddock_env::winpath::for_tools` does for the tool (the launcher
/// cannot depend on it): a volume or device path, an unsafe spelling or one over 259 UTF-16 units is refused, never passed on in its
/// verbatim form.
#[expect(
    clippy::disallowed_methods,
    reason = "std only: paddock_env::winpath does not exist before the launcher has run; the verbatim prefix is handled below"
)]
fn mixed_path(p: &Path) -> Result<String, String> {
    let real = std::fs::canonicalize(p)
        .map_err(|e| format!("cannot resolve {}: {e}", p.display()))?
        .display()
        .to_string();
    if !cfg!(windows) {
        return Ok(real);
    }
    let plain = if let Some(unc) = real.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(rest) = real.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        if !(b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\') {
            return Err(format!("{real} is a volume or device path, not a drive folder"));
        }
        rest.to_string()
    } else if real.starts_with(r"\\.\") {
        return Err(format!("{real} is a device path, not a drive folder"));
    } else {
        real
    };
    let units = plain.encode_utf16().count();
    if units > 259 {
        return Err(format!("{plain} is {units} characters long, over the 259 a program accepts"));
    }
    Ok(plain.replace('\\', "/"))
}

/// Whether `dir` is a folder this process can write to (`[ -d ] && [ -w ]`).
fn writable_dir(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let probe = dir.join(download::unique(".paddock-writable."));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// The rest of a launch once the hash is known: build or fetch the tool, then run it.
fn launch(ctx: &Ctx, f: &Found) -> i32 {
    let name = format!("paddock-dev-{}{EXE}", f.host);
    let target_base = ctx.target_dir.clone().unwrap_or_else(|| ctx.tool.join("target"));
    let first = ctx.args.first().map(String::as_str);

    if env_var("PADDOCK_DEV_FRESH").as_deref() == Some("1") {
        if let Err(code) = refuse_build_target() {
            return code;
        }
        let fresh = fresh_dir();
        let _ = std::fs::create_dir_all(&fresh);
        let mut c = proc::command("cargo");
        c.args(["build", "--release", "--locked", "--quiet"])
            .current_dir(&ctx.tool)
            .env_remove("RUSTC_WRAPPER")
            .env("CARGO_TARGET_DIR", &fresh)
            .env("PADDOCK_DEV_HASH", &f.hash);
        let code = proc::run_inherit(c);
        let result = if code != 0 {
            code
        } else if let Err(c) = built_check(&fresh) {
            c
        } else {
            let mut run = proc::command(fresh.join("release").join(format!("paddock-dev{EXE}")));
            run.args(&ctx.args)
                .env("PADDOCK_DEV_WRAPPER_HASH", &f.hash)
                .envs(ctx.extra_env.iter().map(|(k, v)| (*k, v)));
            proc::run_inherit(run)
        };
        let _ = std::fs::remove_dir_all(&fresh);
        return result;
    }

    if f.dirty {
        // A build of uncommitted inputs is nobody's cached tool, and a job must not pin one that changes under it.
        if first == Some("--print-binary") {
            eprintln!("dev: --print-binary: the tool's inputs have uncommitted changes, so there is no cached binary to name.");
            return 3;
        }
        // One folder per compiler: cargo's rlibs of another compiler are never in it. Cargo's own lock on the folder serialises two
        // builds of the same compiler; the tool built here is nobody's cached tool.
        let dirty_target = target_base.join(format!("rustc-{}", f.ckey));
        if let Err(end) = ctx.build(f, &dirty_target, "dirty") {
            return ctx.restart_or_fail(end);
        }
        return ctx.start(&dirty_target.join("release").join(format!("paddock-dev{EXE}")), &f.hash);
    }

    let cache_dir = ctx.home.join(ctx.binroot).join(&f.hash);
    let cached = cache_dir.join(&name);
    // A cached binary must run: an empty or damaged file (a copy that was cut short) is rebuilt. It is never removed first: the
    // rebuild replaces it by an atomic rename, and a failed probe is no proof (a slow `--version` on a loaded machine), while another
    // run may have pinned this path (`--print-binary`) or be about to start it.
    let mut stale = false;
    if cached.exists() && !runs(&cached) {
        eprintln!("dev: the cached tool binary {} does not run; rebuilding it.", cached.display());
        stale = true;
    }
    if stale || !is_executable(&cached) {
        let fetched = !ctx.no_download
            && download::enabled()
            && download::prebuilt(&Fetch {
                hash: &f.hash,
                name: &name,
                host: &f.host,
                cache: &cache_dir,
                explain: &|| explain(f),
            });
        if !fetched && let Err(code) = ctx.build_into_cache(f, &name, &cache_dir, &target_base) {
            return code;
        }
        cache::prune(&ctx.home.join(ctx.binroot), &name, &ctx.home);
        cache::sweep_build_folders(&ctx.home, &target_base, &f.ckey);
    }
    cache::touch(&cache_dir);
    // `--print-binary`: where the tool is (built if need be), without running it. job-isolation pins it for the whole job: the hash
    // covers files the job itself writes (tools/paddock-dev/.cargo/ci.toml), so at the job's end ./dev would want another build.
    if first == Some("--print-binary") {
        say(&cached.display().to_string());
        return 0;
    }
    ctx.start(&cached, &f.hash)
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

impl Ctx {
    /// A local build into the cache entry: one build folder per compiler (never one that spans two): cargo's own lock on it
    /// serialises builds of one compiler, a change to an input rebuilds only the tool's own crate (warm), and a build of another
    /// compiler cannot meet its rlibs (run 8027: E0514). It sits next to the cache in a kept mount, else inside
    /// `PADDOCK_DEV_TARGET_DIR` (or the tool's own target folder). `Err` is the launcher's exit code.
    fn build_into_cache(&self, f: &Found, name: &str, cache_dir: &Path, target_base: &Path) -> Result<(), i32> {
        let tbase: &Path = if self.kept_target { &self.home } else { target_base };
        if !self.kept_target && env_var("GITHUB_ACTIONS").as_deref() == Some("true") && !self.home_set {
            eprintln!(
                "::notice::./dev builds paddock-dev in this job: the runner has no writable {}, so nothing is kept between runs (cold build, about a minute or two).",
                self.ci_mount.display()
            );
        }
        let target = tbase.join(format!("rustc-{}", f.ckey));
        if std::fs::create_dir_all(&target).is_err() {
            eprintln!(
                "::error::./dev cannot make the build folder {}. Make {} writable or set PADDOCK_DEV_TARGET_DIR to a folder you can write, then run ./dev again.",
                target.display(),
                tbase.display()
            );
            return Err(1);
        }
        let b0 = Instant::now();
        // A name of our own in the cache folder (never a PID: the folder may be shared by containers whose PIDs repeat), checked for
        // the hash it was built for before it replaces anything, then published by an atomic rename. Another build of this compiler
        // (another hash, one folder) can replace the binary between cargo's end and the copy: then the copy names the wrong hash and
        // the build is run again, at most three times.
        let mut tries = 0;
        let tmp = loop {
            tries += 1;
            cache::touch(&target);
            eprintln!(
                "dev: building paddock-dev {} in {} (compiler {}, try {tries}).",
                f.hash,
                target.display(),
                f.ckey
            );
            if let Err(end) = self.build(f, &target, &f.hash) {
                return Err(self.restart_or_fail(end));
            }
            let _ = std::fs::create_dir_all(cache_dir);
            let tmp = cache_dir.join(format!("{name}.{}", download::unique("")));
            if std::fs::copy(target.join("release").join(format!("paddock-dev{EXE}")), &tmp).is_err() {
                eprintln!(
                    "::error::./dev could not copy the built paddock-dev into {}. Make it writable, then run ./dev again.",
                    cache_dir.display()
                );
                return Err(1);
            }
            download::make_executable(&tmp);
            if download::reports_hash(&tmp, &f.hash) {
                break tmp;
            }
            let _ = std::fs::remove_file(&tmp);
            if tries >= 3 {
                eprintln!(
                    "::error::./dev built a paddock-dev that is not for hash {} three times in a row (another run builds in {}, or the build ignored PADDOCK_DEV_HASH). Run ./dev again; if it repeats, report it with this log.",
                    f.hash,
                    target.display()
                );
                return Err(1);
            }
        };
        if std::fs::rename(&tmp, cache_dir.join(name)).is_err() {
            // Windows cannot replace a tool that another job is running: the same hash, so that tool is as good as ours.
            let _ = std::fs::remove_file(&tmp);
            let theirs = cache_dir.join(name);
            if runs(&theirs) && download::reports_hash(&theirs, &f.hash) {
                eprintln!("dev: another run published paddock-dev {} first; using it.", f.hash);
            } else {
                eprintln!(
                    "::error::./dev could not publish paddock-dev {} into {}. Close programs that use it or delete the folder, then run ./dev again.",
                    f.hash,
                    cache_dir.display()
                );
                return Err(1);
            }
        }
        eprintln!(
            "dev: built paddock-dev {} in {} s (not downloaded).",
            f.hash,
            b0.elapsed().as_secs()
        );
        Ok(())
    }
}
