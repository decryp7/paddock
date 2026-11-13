//! The host steps of `dev ci` against the shell they replace (docs/plans/ci-in-rust.md, WP-4): `stable-workspace`, `node-caches`,
//! `host-tools`, `sccache-stats`, `build-folder`, `prune-folder`, `prune-build-folder` and `host-cleanup` run once as the shell does
//! (`conformance/ci/host/*.sh`, `build-folder.sh`, `tools/prune-*.sh`, `tools/host-cleanup.sh`, under the runner's `bash -e -o
//! pipefail`) and once as the Rust command, each on its own copy of the same fixture folder, and must agree on everything that
//! reaches the job: the exit code, standard output, `$GITHUB_ENV`, `$GITHUB_PATH`, the calls of the wrapped programs and the state of
//! the folders afterwards. Spellings that differ on purpose are normalised before the comparison: durations, the scratch folder, the
//! `run:`/`ran:` lines of the wrapper contract, `ci isolation`'s own messages (standard error in the script) and the `Next step:` the Rust errors add to the shell's cause (ER-27). Every case
//! runs under a scratch folder (ER-14: never the machine's CI home, temp folders or processes). Unix only: Windows jobs run the real
//! thing, and `host_steps_golden.rs` keeps the answers as literals for every OS.
// windows-twin: host_steps_golden.rs
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    paddock_env::winpath::real(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

/// One fixture: a scratch folder with the runner's folders in it.
struct Fx {
    dir: tempfile::TempDir,
}

impl Fx {
    fn root(&self) -> PathBuf {
        paddock_env::winpath::real(self.dir.path()).unwrap()
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.root().join(rel)
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.p(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn script(&self, rel: &str, body: &str) {
        self.write(rel, &format!("#!/bin/sh\n{body}\n"));
        std::fs::set_permissions(self.p(rel), std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A program on the front of PATH that records its call (arguments, and what it saw of the job's token) in `calls`.
    fn shim(&self, name: &str, body: &str) {
        let calls = self.p("calls");
        self.script(
            &format!("bin/{name}"),
            &format!("echo \"{name} $* token=${{GITHUB_TOKEN:-}}\" >> '{}'\n{body}", calls.display()),
        );
    }

    fn big(&self, rel: &str, kb: usize) {
        let data: Vec<u8> = (0..kb * 1024).map(|i| (i.wrapping_mul(2_654_435_761usize) >> 7) as u8).collect();
        self.write(
            &format!("{rel}/blob"),
            &String::from_utf8_lossy(&data.iter().map(|b| b % 26 + b'a').collect::<Vec<u8>>()),
        );
    }
}

fn fixture() -> Fx {
    // The scratch folder is `$TMPDIR` (a folder of the agent's own when the tests run under an agent), never the machine's own.
    let fx = Fx {
        dir: tempfile::Builder::new().prefix("paddock-hs-").tempdir().unwrap(),
    };
    std::fs::create_dir_all(fx.p("home")).unwrap();
    std::fs::create_dir_all(fx.p("bin")).unwrap();
    std::fs::create_dir_all(fx.p("work/abc/hostexecutor")).unwrap();
    fx.write("work/abc/hostexecutor/Cargo.toml", "x");
    fx
}

/// What a run leaves: the exit code, the normalised output and the files the job reads, and the folders' state.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    code: i32,
    stdout: String,
    github_env: String,
    github_path: String,
    calls: String,
    tree: Vec<String>,
}

/// The text with the fixture's folder, durations and the spellings that differ on purpose normalised.
fn norm(text: &str, fx: &Fx) -> String {
    let root = fx.root().display().to_string();
    let dur = regex::Regex::new(r" in \d+ s\b").unwrap();
    let private = regex::Regex::new(r"<ROOT>/tmp/(tmp\.[A-Za-z0-9]+|guardian-as-user\.[0-9]+\.[0-9]+)").unwrap();
    let secs_cell = regex::Regex::new(r"\| \d+ s \|").unwrap();
    let sizes = regex::Regex::new(r"(is|still) \d+( \(cap|;)").unwrap();
    let bash_killed = regex::Regex::new(r": line \d+: +\d+ Killed ").unwrap();
    text.replace(&root, "<ROOT>")
        .lines()
        // The wrapper contract's lines, and the one line per pattern the Rust clean-up adds (ER-27).
        // The shim's `kit:` lines name the implementation it chose; they are not the step's output.
        .filter(|l| {
            !l.starts_with("run: ")
                && !l.starts_with("ran: ")
                && !l.starts_with("host-cleanup: pattern ")
                && !l.starts_with("kit: ")
                // bash's own note about the phase it killed at its limit (a job-control message, not the step's): macOS bash 3.2
                // says `Killed: 9`, Linux bash 5 `<script>: line N: <pid> Killed  <command>`.
                && !l.contains(" Killed: 9 ")
                && !bash_killed.is_match(l)
        })
        .map(|l| {
            let l = dur.replace_all(l, " in N s").to_string();
            // The guardian step's private folder: `mktemp -d` in the shell, `guardian-as-user.<pid>.<n>` in Rust.
            let l = private.replace_all(&l, "<PRIVATE>").to_string();
            let l = secs_cell.replace_all(&l, "| N s |").to_string();
            // The shell says `cargo test --no-run failed`, the Rust command quotes the command (backticks) and names how it ended.
            let l = l.replace(
                "`cargo test --no-run` failed (exit 101; output above).",
                "cargo test --no-run failed (output above)",
            );
            let l = l.replace("cargo test --no-run failed (exit 101)", "cargo test --no-run failed (output above)");
            // The shell's `bounded` prints the exit status of the phase's last command; the Rust phase has one failure, `exit 1`.
            let l = l.replace(" failed (exit 128) in N s", " failed (exit 1) in N s");
            // A git that hangs: the Rust tool names the command in the last output (the shell's was empty, it ended the process itself).
            let l = l
                .replace("Last output: kept-clone: git fetch failed (timed out after 1 s).", "Last output: .")
                .replace("(kept-clone: git fetch failed (timed out after 1 s))", "()");
            // The shell's last-output text ends with a space (the `tr` of the tail's last newline); the Rust text does not.
            let l = l.replace(" ); ", "); ").replace(") )", "))");
            // `du`'s directory blocks and ours differ by a kilobyte or two: the size in a prune line is not compared.
            let l = sizes
                .replace_all(&l, "$1 N$2")
                .to_string()
                .replace(" (.gitea/actions/host-tools)", "");
            // The guardian shell ends its error with the next step as a plain sentence; the Rust line says `Next step:`, cut below.
            let l = l.replace(
                ". Run the same command in the CI image to see it",
                ". Next step: run the same command in the CI image to see it.",
            );
            match (l.starts_with("::error::"), l.find(" Next step: ")) {
                (true, Some(at)) => l[..at].to_string(),
                _ => l,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end_matches('\n')
        .to_string()
}

/// Every file, folder and link below the fixture's `work`, `home`, `ws`, `mount` and `tmp` folders (the job's own files `github-*` and `calls` are
/// compared separately): `kind path` or `l path -> target`.
/// Left out: `.paddock-owner.toml`, which the Rust kit writes through dev-disk's slot paths and the shell fallback (deleted at WP-11) does not.
fn tree(fx: &Fx) -> Vec<String> {
    let mut out = Vec::new();
    let root = fx.root();
    let written = regex::Regex::new(r"(?m)^# Written by .*$").unwrap();
    let mut stack = vec![root.clone()];
    while let Some(d) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let rel = p.strip_prefix(&root).unwrap().display().to_string();
            if matches!(rel.as_str(), "bin" | "calls" | "github-env" | "github-path") || rel.starts_with("bin/") {
                continue;
            }
            // The owner record `./dev disk` reads (feat/dev-disk): the Rust kit's slot paths write it, the shell fallback does not.
            // It is not part of what a step does, so the parity of the two compares everything else.
            if e.file_name() == ".paddock-owner.toml" {
                continue;
            }
            let t = e.file_type().unwrap();
            if t.is_symlink() {
                let target = std::fs::read_link(&p)
                    .unwrap()
                    .display()
                    .to_string()
                    .replace(&root.display().to_string(), "<ROOT>");
                out.push(format!("l {rel} -> {target}"));
            } else if t.is_dir() {
                out.push(format!("d {rel}"));
                stack.push(p);
            } else {
                // The size of a file is its content: a blob compares by length, text by text.
                let len = std::fs::metadata(&p).unwrap().len();
                let content = if len < 400 {
                    let text = std::fs::read_to_string(&p)
                        .unwrap_or_default()
                        .replace(&root.display().to_string(), "<ROOT>");
                    written.replace_all(&text, "# Written by <the step>").to_string()
                } else {
                    format!("{len} bytes")
                };
                out.push(format!("f {rel} {content:?}"));
            }
        }
    }
    out.sort();
    out
}

/// How to run one step: the shell form and the Rust form, with `<ROOT>` in any argument or variable replaced by the fixture's folder.
struct Step {
    shell: Vec<&'static str>,
    shell_env: Vec<(&'static str, String)>,
    rust: Vec<&'static str>,
    env: Vec<(&'static str, String)>,
    cwd: Option<&'static str>,
}

fn fill(s: &str, fx: &Fx) -> String {
    s.replace("<ROOT>", &fx.root().display().to_string())
}

fn base_env(fx: &Fx) -> Vec<(String, String)> {
    vec![
        ("PATH".into(), format!("{}:/usr/bin:/bin", fx.p("bin").display())),
        ("HOME".into(), fx.p("home").display().to_string()),
        ("PADDOCK_CI_TABLES".into(), repo().join("conformance/ci").display().to_string()),
        // A secret of the job that no step has any use for: the Rust command never passes it on.
        ("GITHUB_TOKEN".into(), "tok-secret".into()),
        ("GITHUB_ENV".into(), fx.p("github-env").display().to_string()),
        ("GITHUB_PATH".into(), fx.p("github-path").display().to_string()),
    ]
}

fn run_one(fx: &Fx, step: &Step, rust: bool) -> Seen {
    // Standard error joins standard output in the order written (`exec 2>&1`): `ci isolation`'s own messages go to standard error in
    // the script, and the step prints them with its own lines, so the two are compared as the job log shows them.
    let mut cmd = paddock_proc::command("sh");
    cmd.args(["-c", "exec 2>&1; exec \"$@\"", "sh"]);
    if rust {
        cmd.arg(env!("CARGO_BIN_EXE_paddock-dev"));
        cmd.arg("ci").args(step.rust.iter().map(|a| fill(a, fx)));
    } else {
        cmd.arg("bash");
        // The script's own path is the repository's; the arguments after it are the fixture's.
        let script = repo().join(step.shell[0]);
        cmd.args(["-e", "-o", "pipefail"])
            .arg(script)
            .args(step.shell[1..].iter().map(|a| fill(a, fx)));
    }
    let cwd = step.cwd.map_or_else(repo, |c| fx.p(c));
    cmd.current_dir(cwd).env_clear();
    for (k, v) in base_env(fx)
        .into_iter()
        .chain(step.env.iter().map(|(k, v)| (k.to_string(), fill(v, fx))))
    {
        cmd.env(k, v);
    }
    if !rust {
        for (k, v) in &step.shell_env {
            cmd.env(k, fill(v, fx));
        }
    }
    let out = cmd.output().unwrap();
    let read = |rel: &str| std::fs::read_to_string(fx.p(rel)).unwrap_or_default();
    // The shell passes the whole environment to the programs it runs, so its shims see the job's token; the Rust command never lets
    // it through (ToolCall). The shell's side is read without it so every other difference shows, and a Rust leak shows as one.
    let calls = if rust {
        read("calls")
    } else {
        read("calls").replace("token=tok-secret", "token=")
    };
    Seen {
        code: out.status.code().unwrap_or(-1),
        stdout: norm(&String::from_utf8_lossy(&out.stdout), fx),
        github_env: norm(&read("github-env"), fx),
        github_path: norm(&read("github-path"), fx),
        calls: norm(&calls, fx),
        tree: tree(fx),
    }
}

/// Runs both on a fresh fixture each (made by `setup`) and demands they agree; returns what the Rust command left.
fn same(setup: &dyn Fn(&Fx), step: &Step) -> Seen {
    let (a, b) = (fixture(), fixture());
    setup(&a);
    setup(&b);
    let shell = run_one(&a, step, false);
    let rust = run_one(&b, step, true);
    assert_eq!(shell, rust, "the shell (left) and the Rust command (right) differ");
    rust
}

// ---- stable-workspace ----

fn stable(os: &'static str, extra: Vec<(&'static str, String)>) -> Step {
    let mut env = vec![
        ("RUNNER_OS", os.to_string()),
        ("PADDOCK_CI_SLOT", "s1".into()),
        ("GITHUB_WORKSPACE", "<ROOT>/work/abc/hostexecutor".into()),
    ];
    env.extend(extra);
    Step {
        shell: vec!["conformance/ci/host/stable-workspace.sh"],
        shell_env: vec![("NAME", "decryp7/paddock-crate".into())],
        rust: vec!["stable-workspace", "--name", "decryp7/paddock-crate"],
        env,
        cwd: Some("work/abc/hostexecutor"),
    }
}

#[test]
fn stable_workspace_moves_the_checkout_leaves_a_link_and_sets_the_workspace() {
    let seen = same(&|_| {}, &stable("macOS", vec![]));
    assert_eq!(seen.code, 0, "{seen:?}");
    assert!(
        seen.github_env
            .starts_with("PADDOCK_WORKSPACE=<ROOT>/home/.paddock-ci/src/decryp7_paddock-crate-s1"),
        "{seen:?}"
    );
    assert!(
        seen.tree
            .iter()
            .any(|l| l.starts_with("l work/abc/hostexecutor -> <ROOT>/home/.paddock-ci/src/decryp7_paddock-crate-s1")),
        "{:?}",
        seen.tree
    );
    assert!(
        seen.tree
            .iter()
            .any(|l| l.starts_with("f home/.paddock-ci/src/decryp7_paddock-crate-s1/Cargo.toml")),
        "{:?}",
        seen.tree
    );
}

#[test]
fn stable_workspace_leaves_linux_a_linked_folder_and_a_job_without_a_slot_alone() {
    same(&|_| {}, &stable("Linux", vec![]));
    let linked = same(
        &|fx| {
            std::fs::create_dir_all(fx.p("elsewhere")).unwrap();
            std::fs::remove_dir_all(fx.p("work/abc/hostexecutor")).unwrap();
            std::os::unix::fs::symlink(fx.p("elsewhere"), fx.p("work/abc/hostexecutor")).unwrap();
        },
        &stable("macOS", vec![]),
    );
    assert_eq!(linked.stdout, "stable-workspace: already linked");
    let mut noslot = stable("macOS", vec![]);
    noslot.env.retain(|(k, _)| *k != "PADDOCK_CI_SLOT");
    let seen = same(&|_| {}, &noslot);
    assert!(
        seen.stdout
            .contains("::warning::stable-workspace: no kept folder of this job's slot"),
        "{seen:?}"
    );
}

// ---- node-caches ----

fn node_caches(os: &'static str, angular: &'static str, extra: Vec<(&'static str, String)>) -> Step {
    let mut env = vec![
        ("RUNNER_OS", os.to_string()),
        ("PADDOCK_CI_SLOT", "s1".into()),
        ("GITHUB_WORKSPACE", "<ROOT>/ws".into()),
        ("PADDOCK_CI_MOUNT", "<ROOT>/no-mount".into()),
    ];
    env.extend(extra);
    Step {
        shell: vec!["conformance/ci/host/node-caches.sh"],
        shell_env: vec![("ANGULAR_DIR", angular.into()), ("NAME", "e2e-1".into())],
        rust: if angular.is_empty() {
            vec!["node-caches", "--name", "e2e-1"]
        } else {
            vec!["node-caches", "--angular-dir", "ui", "--name", "e2e-1"]
        },
        env,
        cwd: None,
    }
}

fn with_stale_angular(fx: &Fx) {
    fx.write("ws/ui/.angular/cache/stale", "");
}

#[test]
fn node_caches_keeps_npm_and_the_angular_cache_in_the_slots_folders_on_a_host() {
    let seen = same(&with_stale_angular, &node_caches("macOS", "ui", vec![]));
    assert!(
        seen.github_env.starts_with("npm_config_cache=<ROOT>/home/.paddock-ci/npm-s1"),
        "{seen:?}"
    );
    assert!(
        seen.tree
            .contains(&"l ws/ui/.angular/cache -> <ROOT>/home/.paddock-ci/angular/e2e-1-s1".to_string()),
        "{:?}",
        seen.tree
    );
    same(&with_stale_angular, &node_caches("macOS", "", vec![]));
}

#[test]
fn node_caches_uses_a_containers_mount_and_says_so_without_one() {
    let notice = same(&with_stale_angular, &node_caches("Linux", "ui", vec![]));
    assert!(
        notice.stdout.starts_with("::notice::no /sccache mount in this container"),
        "{notice:?}"
    );
    let with_mount = same(
        &|fx| {
            with_stale_angular(fx);
            std::fs::create_dir_all(fx.p("no-mount")).unwrap();
        },
        &node_caches("Linux", "ui", vec![]),
    );
    assert!(
        with_mount.github_env.contains("npm_config_cache=<ROOT>/no-mount/npm"),
        "{with_mount:?}"
    );
}

// ---- host-tools ----

const PIN: &str = "0.9.149";

fn host_tools(os: &'static str) -> Step {
    Step {
        shell: vec!["conformance/ci/host/host-tools.sh"],
        shell_env: vec![],
        rust: vec!["host-tools"],
        env: vec![("RUNNER_OS", os.to_string()), ("PADDOCK_CI_SLOT", "s1".into())],
        cwd: None,
    }
}

fn nextest(fx: &Fx, dir: &str, version: &str) {
    fx.script(&format!("{dir}/cargo-nextest"), &format!("echo 'cargo-nextest {version} (x)'"));
}

#[test]
fn host_tools_keeps_the_pinned_program_and_checks_the_version_on_every_runner() {
    // A download fails the test: a kept or image copy must not be fetched again.
    let kept = |fx: &Fx| {
        fx.shim("curl", "echo curl was called >&2; exit 7");
        nextest(fx, &format!("home/.paddock-ci/tools/cargo-nextest/{PIN}"), PIN);
    };
    let seen = same(&kept, &host_tools("macOS"));
    assert_eq!(seen.code, 0, "{seen:?}");
    assert!(
        seen.github_path
            .contains(&format!("<ROOT>/home/.paddock-ci/tools/cargo-nextest/{PIN}")),
        "{seen:?}"
    );
    assert!(seen.stdout.ends_with(&format!("cargo-nextest {PIN}")), "{seen:?}");
    // A kept copy of another version fails the check.
    let stale = same(
        &|fx| nextest(fx, &format!("home/.paddock-ci/tools/cargo-nextest/{PIN}"), "0.9.1"),
        &host_tools("macOS"),
    );
    assert_eq!(stale.code, 1);
    assert!(
        stale
            .stdout
            .contains(&format!("::error::cargo-nextest is 0.9.1 on this runner, the pin is {PIN}")),
        "{stale:?}"
    );
    // Linux: the image's copy only, nothing exported; a missing one is an error.
    let image = same(&|fx| nextest(fx, "bin", PIN), &host_tools("Linux"));
    assert_eq!((image.code, image.github_path.as_str()), (0, ""));
    let missing = same(&|_| {}, &host_tools("Linux"));
    assert_eq!(missing.code, 1);
    assert!(missing.stdout.contains("cargo-nextest is missing on this runner"), "{missing:?}");
}

// ---- sccache-stats ----

fn sccache_stats(port: Option<&'static str>) -> Step {
    Step {
        shell: vec!["conformance/ci/host/sccache-stats.sh"],
        shell_env: vec![],
        rust: vec!["sccache-stats"],
        env: port.map(|p| vec![("SCCACHE_SERVER_PORT", p.to_string())]).unwrap_or_default(),
        cwd: None,
    }
}

#[test]
fn sccache_stats_prints_the_statistics_and_stops_only_the_jobs_own_server() {
    let shim = |fx: &Fx| fx.shim("sccache", "echo 'Compile requests 10'");
    let own = same(&shim, &sccache_stats(Some("4501")));
    assert_eq!(
        own.calls, "sccache --show-stats token=\nsccache --stop-server token=",
        "the token never reaches sccache"
    );
    assert!(own.stdout.contains("Compile requests 10"), "{own:?}");
    let default_port = same(&shim, &sccache_stats(None));
    assert_eq!(
        default_port.calls, "sccache --show-stats token=",
        "a server on the default port is left running"
    );
    same(&|_| {}, &sccache_stats(None));
    let broken = same(&|fx| fx.shim("sccache", "exit 2"), &sccache_stats(None));
    assert!(broken.stdout.contains("::notice::sccache statistics are unavailable"), "{broken:?}");
}

// ---- build-folder ----

fn build_folder(os: &'static str, name: &'static str, slot: &'static str) -> Step {
    let mut env = vec![("RUNNER_OS", os.to_string())];
    if !slot.is_empty() {
        env.push(("PADDOCK_CI_SLOT", slot.to_string()));
    }
    Step {
        shell: vec!["conformance/ci/build-folder.sh", "<ROOT>/ws", name],
        shell_env: vec![],
        rust: vec!["build-folder", "<ROOT>/ws", name],
        env,
        cwd: None,
    }
}

fn cargo_ws(fx: &Fx, include: bool) {
    fx.write(
        "ws/.cargo/config.toml",
        if include {
            "include = [{ path = \"ci.toml\", optional = true }]\n"
        } else {
            "[alias]\n"
        },
    );
    fx.write("ws/.cargo/ci.toml", "# left by an earlier job\n");
}

#[test]
fn build_folder_writes_the_kept_folder_to_the_included_ci_toml_and_target_in_a_container() {
    let host = same(&|fx| cargo_ws(fx, true), &build_folder("macOS", "spotter-rust", "s1"));
    assert!(
        host.tree.iter().any(|l| l.starts_with("f ws/.cargo/ci.toml")
            && l.ends_with("[build]\\ntarget-dir = '<ROOT>/home/.paddock-ci/target-spotter-rust-s1'\\n\"")),
        "cargo reads the folder from the included ci.toml: {:?}",
        host.tree
    );
    // A container, a second slot, a config without the include, a bad name and a host job without a slot.
    for (os, name, slot, include, code) in [
        ("Linux", "x", "", true, 0),
        ("macOS", "x", "s2", true, 0),
        ("macOS", "x", "s1", false, 1),
        ("macOS", "a b", "s1", true, 1),
        ("macOS", "x", "", true, 1),
    ] {
        let seen = same(&move |fx| cargo_ws(fx, include), &build_folder(os, name, slot));
        assert_eq!(seen.code, code, "{os} {name} {slot} {include}: {seen:?}");
    }
}

// ---- prune ----

fn prune_folder(dir: &'static str, cap: &'static str) -> Step {
    Step {
        shell: vec!["tools/prune-build-folders.sh", dir, cap],
        shell_env: vec![],
        rust: vec!["prune-folder", dir, cap],
        env: vec![("PADDOCK_PRUNE_UNIT", "kb".into())],
        cwd: None,
    }
}

#[test]
fn prune_folder_removes_the_incremental_caches_first_then_the_folder_and_never_follows_a_link() {
    let layout = |incremental: usize, deps: usize| {
        move |fx: &Fx| {
            fx.big("ci/build/debug/incremental", incremental);
            fx.big("ci/build/debug/deps", deps);
        }
    };
    let under = same(&layout(100, 1), &prune_folder("<ROOT>/ci/build", "100000"));
    assert_eq!(under.stdout, "");
    let first = same(&layout(400, 50), &prune_folder("<ROOT>/ci/build", "300"));
    assert!(
        first.stdout.contains("removing incremental caches") && !first.tree.iter().any(|l| l.contains("incremental")),
        "{first:?}"
    );
    let still = same(&layout(10, 400), &prune_folder("<ROOT>/ci/build", "300"));
    assert!(
        still.stdout.contains("still ") && !still.tree.iter().any(|l| l.contains("ci/build")),
        "{still:?}"
    );
    same(&|_| {}, &prune_folder("<ROOT>/ci/nope", "1"));
    let linked = same(
        &|fx| {
            fx.big("real/incremental", 500);
            std::fs::create_dir_all(fx.p("ci")).unwrap();
            std::os::unix::fs::symlink(fx.p("real"), fx.p("ci/link")).unwrap();
        },
        &prune_folder("<ROOT>/ci/link", "1"),
    );
    assert!(
        linked.tree.iter().any(|l| l.starts_with("f real/incremental/blob")),
        "the target of a link is left alone"
    );
    // A bad cap is a usage error in both (the messages go to standard error).
    let bad = same(&layout(1, 1), &prune_folder("<ROOT>/ci/build", "many"));
    assert_eq!(bad.code, 2);
}

fn prune_ci(dir: &'static str, slot: &'static str, drop: bool) -> Step {
    let mut shell = vec!["tools/prune-ci-build-folder.sh", dir, "1"];
    let mut rust = vec!["prune-build-folder", "--max-gb", "1"];
    if drop {
        shell.push("--drop-bundles");
        rust.push("--drop-bundles");
    }
    let mut env = vec![
        ("PADDOCK_PRUNE_UNIT", "kb".to_string()),
        ("RUNNER_OS", "macOS".into()),
        ("PADDOCK_BUILD_FOLDER", dir.to_string()),
    ];
    if !slot.is_empty() {
        env.push(("PADDOCK_CI_SLOT", slot.to_string()));
    }
    Step {
        shell,
        shell_env: vec![],
        rust,
        env,
        cwd: None,
    }
}

#[test]
fn prune_build_folder_caps_only_this_jobs_kept_folder_and_leaves_everything_else_alone() {
    let setup = |fx: &Fx| {
        fx.big("home/.paddock-ci/target-app-app-s1/debug/incremental", 3000);
        fx.big("home/.paddock-ci/target-app-app-s1/dist/bundle", 10);
        fx.big("home/.paddock-ci/target-app-app-s2/debug/incremental", 3000);
        fx.big("home/.paddock-ci/notes/debug/incremental", 3000);
        fx.big("home/Documents/debug/incremental", 3000);
        std::os::unix::fs::symlink(fx.p("home/.paddock-ci/target-app-app-s1"), fx.p("home/.paddock-ci/target-link")).unwrap();
    };
    let own = same(&setup, &prune_ci("<ROOT>/home/.paddock-ci/target-app-app-s1", "s1", true));
    assert!(
        !own.tree
            .iter()
            .any(|l| l.contains("target-app-app-s1/debug/incremental") || l.contains("dist/bundle")),
        "{own:?}"
    );
    for (dir, slot) in [
        ("<ROOT>/home/.paddock-ci/target-app-app-s2", "s1"),
        ("<ROOT>/home/.paddock-ci/target-app-app-s1", ""),
        ("<ROOT>/home", "s1"),
        ("/", "s1"),
        ("<ROOT>/home/.paddock-ci/notes", "s1"),
        ("<ROOT>/home/.paddock-ci/target-link", "s1"),
        ("<ROOT>/home/.paddock-ci", "s1"),
        ("<ROOT>/home/.paddock-ci/target-app-app-s1/../../Documents", "s1"),
    ] {
        let seen = same(&setup, &prune_ci(dir, slot, false));
        assert_eq!(seen.code, 0, "{dir}");
        assert!(
            seen.tree.iter().any(|l| l.starts_with("f home/Documents/debug/incremental/blob")),
            "{dir} must not delete the user's files"
        );
    }
}

// ---- host-cleanup ----

fn cleanup(args: Vec<&'static str>, extra: Vec<(&'static str, String)>) -> Step {
    let mut shell = vec!["tools/host-cleanup.sh"];
    shell.extend(args.iter().copied());
    let mut rust = vec!["host-cleanup"];
    rust.extend(args.iter().copied());
    let mut env = vec![
        ("RUNNER_OS", "macOS".to_string()),
        ("PADDOCK_CI_SLOT", "s2".into()),
        ("PADDOCK_JOB_TEMP", "<ROOT>/tmp".into()),
        ("RUNNER_TEMP", "<ROOT>/tmp".into()),
    ];
    env.extend(extra);
    Step {
        shell,
        shell_env: vec![],
        rust,
        env,
        cwd: Some("work"),
    }
}

fn cleanup_fx(fx: &Fx) {
    std::fs::create_dir_all(fx.p("work")).unwrap();
    for d in ["tmp/pitwall-abc", "tmp/pitwall-def", "tmp/other-job"] {
        std::fs::create_dir_all(fx.p(d)).unwrap();
    }
    fx.write("work/dist/x", "1");
    fx.write("work/app/dist/y", "2");
    // Nothing is running that the shim lists, and ps itself is the shim: the real process table is never read or touched (ER-14).
    fx.script("bin/pkill", "");
    fx.script("bin/ps", "");
}

fn pkill_env(args: &mut Vec<(&'static str, String)>) {
    args.push(("PADDOCK_PKILL", "<ROOT>/bin/pkill".into()));
    args.push(("PADDOCK_PS", "<ROOT>/bin/ps".into()));
}

#[test]
fn host_cleanup_removes_only_this_jobs_prefixed_temp_folders_and_keeps_build_outputs_unless_asked() {
    let mut env = vec![("PADDOCK_BUILD_FOLDER", "/ci/target-pitwall-app-s2".to_string())];
    pkill_env(&mut env);
    let seen = same(
        &cleanup_fx,
        &cleanup(
            vec![
                "--id",
                "pitwall",
                "--process-pattern",
                "{build}/debug/pitwall-server",
                "--process-pattern",
                "fake[-_]engine",
                "--process-pattern",
                "chrome-{slot}",
            ],
            env.clone(),
        ),
    );
    assert!(
        seen.stdout.contains("skipped the process pattern 'chrome-{slot}'")
            && seen
                .stdout
                .contains("skipped the process pattern 'fake[-_]engine': it is not anchored"),
        "{seen:?}"
    );
    assert!(
        !seen.tree.iter().any(|l| l.contains("tmp/pitwall-")) && seen.tree.contains(&"d tmp/other-job".to_string()),
        "{:?}",
        seen.tree
    );
    assert!(
        seen.tree.iter().any(|l| l.starts_with("f work/dist/x")),
        "build outputs stay without --build-outputs"
    );
    let with = same(&cleanup_fx, &cleanup(vec!["--id", "pitwall", "--build-outputs"], env));
    assert!(
        !with.tree.iter().any(|l| l.contains("work/dist") || l.contains("work/app/dist")),
        "{with:?}"
    );
}

#[test]
fn host_cleanup_without_a_temp_folder_leaves_the_shared_temp_alone_and_refuses_bad_arguments_with_exit_2() {
    let mut env = vec![("TMPDIR", "<ROOT>/shared".to_string())];
    pkill_env(&mut env);
    let setup = |fx: &Fx| {
        cleanup_fx(fx);
        std::fs::create_dir_all(fx.p("shared/pitwall-it-other-job")).unwrap();
    };
    let mut no_temp = cleanup(vec!["--id", "pitwall", "--temp-prefix", "pitwall-it-"], env.clone());
    no_temp.env.retain(|(k, _)| *k != "PADDOCK_JOB_TEMP" && *k != "RUNNER_TEMP");
    let seen = same(&setup, &no_temp);
    assert!(seen.stdout.contains("no temp folder of this job"), "{seen:?}");
    assert!(seen.tree.contains(&"d shared/pitwall-it-other-job".to_string()));
    for bad in [vec![], vec!["--id", "Bad Id"], vec!["--id", "x", "--temp-prefix", "../"]] {
        assert_eq!(same(&cleanup_fx, &cleanup(bad, env.clone())).code, 2);
    }
    assert_eq!(same(&cleanup_fx, &cleanup(vec!["--id", "x", "--temp-prefix", "job-"], env)).code, 0);
}

#[test]
fn host_cleanup_names_a_process_that_survives_the_same_way_in_both() {
    // The table is a shim that lists one process of this job, one of another slot and an unrelated one, with a pid no process has;
    // the process is never real, so nothing is ended in either implementation (ER-14) and the leak check names the survivor.
    let setup = |fx: &Fx| {
        cleanup_fx(fx);
        fx.script(
            "bin/ps",
            "printf '%s\\n' ' 3999999999 /ci/target-pitwall-app-s2/debug/pitwall-server --fake' ' 3999999998 /ci/target-pitwall-app-s3/debug/pitwall-server --fake' ' 3999999997 /usr/bin/other'",
        );
    };
    let mut env = vec![("PADDOCK_BUILD_FOLDER", "/ci/target-pitwall-app-s2".to_string())];
    pkill_env(&mut env);
    let seen = same(
        &setup,
        &cleanup(
            vec!["--id", "pitwall", "--process-pattern", "{build}[/\\\\]debug[/\\\\]pitwall-server"],
            env,
        ),
    );
    assert!(
        seen.stdout
            .contains("::warning::host-cleanup: processes of this job are still running after the clean-up"),
        "{seen:?}"
    );
    assert!(
        seen.stdout
            .contains("3999999999 /ci/target-pitwall-app-s2/debug/pitwall-server --fake"),
        "{seen:?}"
    );
    assert!(
        !seen.stdout.contains("3999999998") && !seen.stdout.contains("3999999997"),
        "only this job's processes: {seen:?}"
    );
}

/// The Rust command really ends a process of the job (a child of this test whose command line names the job's temp folder), asks it
/// first and leaves one of another job alone: through `paddock_proc` after the identity check (rule 53). The shell did it with pkill.
#[test]
fn host_cleanup_ends_a_real_process_of_the_job_and_only_that_one() {
    let fx = fixture();
    // The real `ps` and the real process table: no shim of either on this fixture's PATH.
    std::fs::create_dir_all(fx.p("work")).unwrap();
    let spawn = |dir: &str| {
        std::fs::create_dir_all(fx.p(dir)).unwrap();
        paddock_proc::command("perl")
            .arg("-e")
            .arg("sleep 30")
            .arg(fx.p(&format!("{dir}/marker-x")))
            .spawn()
            .unwrap()
    };
    let mut mine = spawn("tmp");
    let mut theirs = spawn("other-job-tmp");
    // Both have started `perl`; wait until the table shows them before the clean-up looks (a poll on the condition, never a sleep).
    let ps_lists = |marker: &Path| {
        let out = paddock_proc::command("ps").args(["-axo", "pid=,command="]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&marker.display().to_string())
    };
    assert!(paddock_testkit::poll_until_within(std::time::Duration::from_secs(30), || ps_lists(
        &fx.p("tmp/marker-x")
    )
        && ps_lists(&fx.p("other-job-tmp/marker-x"))));
    let mut env: Vec<(String, String)> = base_env(&fx);
    env.extend([("RUNNER_OS", "macOS"), ("PADDOCK_CI_SLOT", "s2")].map(|(k, v)| (k.to_string(), v.to_string())));
    env.push(("PADDOCK_JOB_TEMP".into(), fx.p("tmp").display().to_string()));
    env.push(("RUNNER_TEMP".into(), fx.p("tmp").display().to_string()));
    let out = paddock_proc::command(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(["ci", "host-cleanup", "--id", "pitwall", "--process-pattern", "{tmp}[/\\\\]marker-x"])
        .current_dir(fx.p("work"))
        .env_clear()
        .envs(env)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stdout));
    let status = mine.wait().unwrap();
    assert!(!status.success(), "the job's process was ended: {status:?}");
    assert!(theirs.try_wait().unwrap().is_none(), "another job's process runs on");
    theirs.kill().unwrap();
    theirs.wait().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("still running"), "{text}");
}

// ---- kept-clone ----

const KEPT_PROFILES: &str =
    "exclude lean big/ large\nexclude lean apps/*/docs/pics shots\nexclude full nothing/ nothing\nbootstrap .gitea\n";

/// `git` in `dir` with a fixed identity and dates (both fixtures get the same commit ids) and no user or system configuration.
fn git(fx: &Fx, dir: &Path, args: &[&str]) -> String {
    let out = paddock_proc::command("git")
        .args(["-c", "protocol.file.allow=always", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .env("HOME", fx.p("home"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// An origin that allows partial clones and fetches by commit (as the Gitea server does) with one commit, and the job's bootstrap
/// checkout of it at `work/abc/hostexecutor` (what `actions/checkout` leaves, with its credential header), and the profile file.
fn kept_fixture(fx: &Fx) {
    let (repo, origin) = (fx.p("work-repo"), fx.p("origin.git"));
    std::fs::create_dir_all(&repo).unwrap();
    git(fx, &repo, &["init", "-q"]);
    for (rel, text) in [
        ("crates/a/lib.rs", "one"),
        ("big/shot.png", "png"),
        ("apps/x/docs/pics/s.png", "png"),
        ("apps/x/docs/guide.md", "guide"),
        (".gitignore", "target/\n"),
    ] {
        fx.write(&format!("work-repo/{rel}"), text);
    }
    git(fx, &repo, &["add", "-A"]);
    git(fx, &repo, &["commit", "-qm", "one"]);
    git(fx, fx.root().as_path(), &["clone", "-q", "--bare", "work-repo", "origin.git"]);
    git(fx, &origin, &["config", "uploadpack.allowFilter", "true"]);
    git(fx, &origin, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
    git(
        fx,
        &repo,
        &[
            "remote",
            "add",
            "origin",
            &format!("file://{}", paddock_testkit::path::text(&origin)),
        ],
    );
    fx.write("profiles.txt", KEPT_PROFILES);
    std::fs::remove_dir_all(fx.p("work/abc/hostexecutor")).unwrap();
    bootstrap_at(fx, &git(fx, &repo, &["rev-parse", "HEAD"]));
}

/// The job's bootstrap checkout at `sha` (made again when it is not there yet).
fn bootstrap_at(fx: &Fx, sha: &str) {
    let ws = fx.p("work/abc/hostexecutor");
    let url = format!("file://{}", paddock_testkit::path::text(fx.p("origin.git")));
    if !ws.exists() {
        git(fx, &fx.root(), &["clone", "-q", "--no-checkout", &url, "work/abc/hostexecutor"]);
        git(
            fx,
            &ws,
            &[
                "config",
                "http.https://example.invalid/.extraheader",
                "AUTHORIZATION: basic SECRETTOKEN",
            ],
        );
    }
    git(fx, &ws, &["fetch", "-q", "origin", sha]);
    git(fx, &ws, &["-c", "advice.detachedHead=false", "checkout", "-q", "--detach", sha]);
}

/// A later job: the runner gives it a new work folder (the old one, a link into the kept clone on macOS, is gone) checked out at the
/// origin's newest commit.
fn fresh_job(fx: &Fx) {
    let ws = fx.p("work/abc/hostexecutor");
    match std::fs::symlink_metadata(&ws) {
        Ok(m) if m.file_type().is_symlink() => std::fs::remove_file(&ws).unwrap(),
        Ok(_) => std::fs::remove_dir_all(&ws).unwrap(),
        Err(_) => {}
    }
    bootstrap_at(fx, &git(fx, &fx.p("work-repo"), &["rev-parse", "HEAD"]));
}

/// A new commit on the origin; returns its id.
fn kept_commit(fx: &Fx, rel: &str, text: &str, msg: &str) -> String {
    let repo = fx.p("work-repo");
    fx.write(&format!("work-repo/{rel}"), text);
    git(fx, &repo, &["add", "-A"]);
    git(fx, &repo, &["commit", "-qm", msg]);
    git(fx, &repo, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    git(fx, &repo, &["rev-parse", "HEAD"])
}

fn kept_step(os: &'static str, profile: &'static str, extra: Vec<(&'static str, String)>) -> Step {
    let mut env = vec![
        ("RUNNER_OS", os.to_string()),
        ("PADDOCK_CI_SLOT", "s1".into()),
        ("GITHUB_WORKSPACE", "<ROOT>/work/abc/hostexecutor".into()),
        ("GIT_CONFIG_NOSYSTEM", "1".into()),
    ];
    env.extend(extra);
    Step {
        shell: vec!["conformance/ci/kept-clone.sh"],
        shell_env: vec![
            ("NAME", "decryp7/paddock-crate".into()),
            ("PROFILE", profile.into()),
            ("KEPT_CLONE_PROFILES", "<ROOT>/profiles.txt".into()),
        ],
        rust: vec![
            "kept-clone",
            "--name",
            "decryp7/paddock-crate",
            "--profile",
            profile,
            "--profiles",
            "<ROOT>/profiles.txt",
        ],
        env,
        cwd: Some("work/abc/hostexecutor"),
    }
}

/// The slot's kept clone as git sees it: what a later job (or a person) would find, compared besides the folder tree (whose `.git`
/// objects differ in their packing, not in what they hold).
fn kept_facts(fx: &Fx) -> Vec<String> {
    let src = fx.p("home/.paddock-ci/src");
    let Some(stable) = std::fs::read_dir(&src).ok().and_then(|rd| {
        rd.flatten().map(|e| e.path()).find(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("decryp7_paddock-crate-"))
        })
    }) else {
        return vec!["facts: no kept clone".into()];
    };
    let q = |args: &[&str]| -> String {
        let out = paddock_proc::command("git")
            .args(args)
            .current_dir(&stable)
            .env("HOME", fx.p("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).replace(&fx.root().display().to_string(), "<ROOT>")
    };
    let mut keys: Vec<String> = q(&["config", "--local", "--list"])
        .lines()
        .map(|l| l.split('=').next().unwrap().to_string())
        .collect();
    keys.sort();
    vec![
        format!("facts: head {}", q(&["rev-parse", "HEAD"]).trim()),
        format!("facts: config keys {keys:?}"),
        format!("facts: files {:?}", q(&["ls-files", "-v"]).lines().collect::<Vec<_>>()),
        format!(
            "facts: status {:?}",
            q(&["status", "--porcelain", "--ignored"]).lines().collect::<Vec<_>>()
        ),
        format!("facts: marker {}", stable.join(".git/paddock-kept-clone").exists()),
        format!("facts: remote {}", q(&["config", "remote.origin.url"]).trim()),
    ]
}

fn run_kept(fx: &Fx, step: &Step, rust: bool) -> Seen {
    let mut seen = run_one(fx, step, rust);
    // The kept clone's own `.git` (and the bootstrap's) hold packs and logs that differ in bytes; `kept_facts` compares what they mean.
    seen.tree.retain(|l| !l.contains("/.git/"));
    seen.tree.extend(kept_facts(fx));
    seen
}

/// Both implementations on a fresh kept fixture each: a first job, then `between` (what a later job finds), then a second job.
/// Every job's result must agree; returns the Rust command's second result.
fn kept_twice(first: &Step, between: &dyn Fn(&Fx), second: &Step) -> Seen {
    let (a, b) = (fixture(), fixture());
    kept_fixture(&a);
    kept_fixture(&b);
    let (s1, r1) = (run_kept(&a, first, false), run_kept(&b, first, true));
    assert_eq!(s1, r1, "the first job: the shell (left) and the Rust command (right) differ");
    for fx in [&a, &b] {
        between(fx);
        fresh_job(fx);
    }
    let (s2, r2) = (run_kept(&a, second, false), run_kept(&b, second, true));
    assert_eq!(s2, r2, "the second job: the shell (left) and the Rust command (right) differ");
    r2
}

fn kept_once(setup: &dyn Fn(&Fx), step: &Step) -> Seen {
    let (a, b) = (fixture(), fixture());
    for fx in [&a, &b] {
        kept_fixture(fx);
        setup(fx);
    }
    let (s, r) = (run_kept(&a, step, false), run_kept(&b, step, true));
    assert_eq!(s, r, "the shell (left) and the Rust command (right) differ");
    r
}

#[test]
fn kept_clone_creates_the_slots_clone_at_the_commit_without_the_excluded_paths() {
    let seen = kept_once(&|_| {}, &kept_step("macOS", "lean", vec![]));
    assert_eq!(seen.code, 0, "{seen:?}");
    assert!(seen.stdout.contains("kept-clone: ready: "), "{}", seen.stdout);
    assert!(seen.stdout.contains("(created, profile lean, 3 files)"), "{}", seen.stdout);
    assert!(
        seen.github_env
            .starts_with("PADDOCK_WORKSPACE=<ROOT>/home/.paddock-ci/src/decryp7_paddock-crate-s1"),
        "{seen:?}"
    );
    let t = &seen.tree;
    assert!(
        t.iter()
            .any(|l| l.starts_with("f home/.paddock-ci/src/decryp7_paddock-crate-s1/crates/a/lib.rs")),
        "{t:?}"
    );
    assert!(
        !t.iter()
            .any(|l| l.contains("crate-s1/big") || l.contains("crate-s1/apps/x/docs/pics")),
        "an excluded path is not checked out: {t:?}"
    );
    assert!(
        t.iter().any(|l| l.starts_with("l work/abc/hostexecutor -> ")),
        "macOS leaves a link: {t:?}"
    );
    assert!(!seen.stdout.contains("SECRETTOKEN"), "the credential header is never printed");
    // On a Windows runner the work folder stays (act holds it open); on Linux nothing is done.
    let win = kept_once(&|_| {}, &kept_step("Windows", "lean", vec![]));
    assert!(!win.tree.iter().any(|l| l.starts_with("l work/abc/hostexecutor")), "{:?}", win.tree);
    let linux = kept_once(&|_| {}, &kept_step("Linux", "lean", vec![]));
    assert_eq!(linux.stdout, "");
    assert!(linux.tree.contains(&"facts: no kept clone".to_string()));
}

#[test]
fn kept_clone_reuses_the_clone_and_leaves_a_clean_tree_whatever_the_last_job_left() {
    let step = kept_step("macOS", "lean", vec![]);
    let seen = kept_twice(
        &step,
        &|fx| {
            fx.write("home/.paddock-ci/src/decryp7_paddock-crate-s1/crates/a/untracked.rs", "x");
            fx.write("home/.paddock-ci/src/decryp7_paddock-crate-s1/target/ignored.bin", "x");
            fx.write("home/.paddock-ci/src/decryp7_paddock-crate-s1/crates/a/lib.rs", "modified");
            fx.write("home/.paddock-ci/src/decryp7_paddock-crate-s1/big/leak.txt", "leak");
            // The next job runs at the next commit.
            kept_commit(fx, "crates/a/lib.rs", "two", "two");
        },
        &step,
    );
    assert!(seen.stdout.contains("(reused, profile lean"), "{}", seen.stdout);
    assert!(seen.stdout.contains("kept-clone: clean: done"), "{}", seen.stdout);
    assert!(seen.tree.contains(&"facts: status []".to_string()), "{:?}", seen.tree);
    assert!(
        !seen.tree.iter().any(|l| l.contains("untracked.rs") || l.contains("leak.txt")),
        "{:?}",
        seen.tree
    );
}

#[test]
fn kept_clone_makes_the_clone_again_when_it_is_foreign_stale_locked_unmarked_or_too_big() {
    let step = kept_step("macOS", "lean", vec![]);
    // A config key this step did not write (it changes what git does): one warning naming it, a new clone.
    let seen = kept_twice(
        &step,
        &|fx| {
            let k = fx.p("home/.paddock-ci/src/decryp7_paddock-crate-s1");
            git(fx, &k, &["config", "--local", "core.autocrlf", "true"]);
            git(fx, &k, &["config", "--local", "core.fsmonitor", "touch /nonexistent/never"]);
        },
        &step,
    );
    assert!(
        seen.stdout.contains("carries the local git config key 'core.autocrlf'"),
        "{}",
        seen.stdout
    );
    assert!(seen.stdout.contains("(created, profile lean"), "{}", seen.stdout);
    // The marker is missing (an older layout's checkout): never reused.
    let seen = kept_twice(
        &step,
        &|fx| std::fs::remove_file(fx.p("home/.paddock-ci/src/decryp7_paddock-crate-s1/.git/paddock-kept-clone")).unwrap(),
        &step,
    );
    assert!(seen.stdout.contains("(created, profile lean"), "{}", seen.stdout);
    // Too big.
    let big = kept_step("macOS", "lean", vec![("PADDOCK_KEPT_CLONE_MAX_MIB", "0".into())]);
    let seen = kept_twice(&step, &|_| {}, &big);
    assert!(seen.stdout.contains("larger than 0 MiB; starting again"), "{}", seen.stdout);
    // A broken HEAD.
    let seen = kept_twice(
        &step,
        &|fx| fx.write("home/.paddock-ci/src/decryp7_paddock-crate-s1/.git/HEAD", "garbage\n"),
        &step,
    );
    assert!(seen.stdout.contains("(created, profile lean"), "{}", seen.stdout);
    // A stale lock: the first bring-up fails on it, a new clone succeeds, and the failing command is named.
    let seen = kept_twice(
        &step,
        &|fx| {
            fx.write("home/.paddock-ci/src/decryp7_paddock-crate-s1/.git/index.lock", "");
            kept_commit(fx, "crates/a/lib.rs", "two", "two");
        },
        &step,
    );
    assert!(
        seen.stdout.contains("kept-clone: git "),
        "the failing command is named: {}",
        seen.stdout
    );
    assert!(seen.stdout.contains("(created, profile lean"), "{}", seen.stdout);
    // The tag run never reuses a clone shared with branch jobs: the shell has FRESH=true, the Rust command --fresh.
    let (a, b) = (fixture(), fixture());
    for fx in [&a, &b] {
        kept_fixture(fx);
    }
    let first = kept_step("macOS", "lean", vec![]);
    let _ = (run_kept(&a, &first, false), run_kept(&b, &first, true));
    fresh_job(&a);
    fresh_job(&b);
    let mut shell_fresh = kept_step("macOS", "lean", vec![]);
    shell_fresh.shell_env.push(("FRESH", "true".into()));
    let mut rust_fresh = kept_step("macOS", "lean", vec![]);
    rust_fresh.rust.push("--fresh");
    let (s, r) = (run_kept(&a, &shell_fresh, false), run_kept(&b, &rust_fresh, true));
    assert_eq!(s, r, "FRESH: the shell (left) and the Rust command (right) differ");
    assert!(
        r.stdout.contains("(created, profile lean") && !r.stdout.contains("kept-clone: clean"),
        "{}",
        r.stdout
    );
}

#[test]
fn kept_clone_falls_back_to_the_work_folder_and_fails_only_on_an_unknown_profile() {
    // The origin is gone: the clone cannot be made; the job builds from the work folder with one warning.
    let seen = kept_once(
        &|fx| {
            git(
                fx,
                &fx.p("work/abc/hostexecutor"),
                &[
                    "remote",
                    "set-url",
                    "origin",
                    &format!("file://{}", paddock_testkit::path::text(fx.root().join("no-such-origin.git"))),
                ],
            );
        },
        &kept_step("macOS", "lean", vec![]),
    );
    assert_eq!(seen.code, 0, "{seen:?}");
    assert!(seen.stdout.contains("building from the work folder"), "{}", seen.stdout);
    assert!(!seen.github_env.contains("PADDOCK_WORKSPACE"), "{seen:?}");
    // No slot: nothing to keep.
    let mut noslot = kept_step("macOS", "lean", vec![]);
    noslot.env.retain(|(k, _)| *k != "PADDOCK_CI_SLOT");
    let seen = kept_once(&|_| {}, &noslot);
    assert!(
        seen.stdout.contains("::warning::kept-clone: no kept folder of this job's slot"),
        "{}",
        seen.stdout
    );
    // The full profile leaves nothing out; an undeclared one is the one failure, with one ::error::.
    let full = kept_once(&|_| {}, &kept_step("macOS", "full", vec![]));
    assert!(full.tree.iter().any(|l| l.contains("crate-s1/big/shot.png")), "{:?}", full.tree);
    let bad = kept_once(&|_| {}, &kept_step("macOS", "nope", vec![]));
    assert_eq!(bad.code, 1);
    assert_eq!(
        bad.stdout.lines().filter(|l| l.starts_with("::error::")).count(),
        1,
        "{}",
        bad.stdout
    );
    assert!(
        bad.stdout
            .contains("kept-clone: profile 'nope' is not declared in <ROOT>/profiles.txt"),
        "{}",
        bad.stdout
    );
}

#[test]
fn kept_clone_plants_in_the_clone_do_not_act_in_the_next_job() {
    let step = kept_step("macOS", "lean", vec![]);
    let seen = kept_twice(
        &step,
        &|fx| {
            let k = fx.p("home/.paddock-ci/src/decryp7_paddock-crate-s1");
            // A hook that would leave a mark, an attributes file that would rewrite files, a link of the Angular cache to a kept folder.
            fx.script(
                "home/.paddock-ci/src/decryp7_paddock-crate-s1/.git/hooks/post-checkout",
                &format!("echo ran > '{}'", fx.p("hook-ran").display()),
            );
            fx.write(
                "home/.paddock-ci/src/decryp7_paddock-crate-s1/.git/info/attributes",
                "* text eol=crlf\n",
            );
            fx.write("kept-angular-cache/cached.bin", "keep");
            std::fs::create_dir_all(k.join("ui/.angular")).unwrap();
            std::os::unix::fs::symlink(fx.p("kept-angular-cache"), k.join("ui/.angular/cache")).unwrap();
            kept_commit(fx, "crates/a/lib.rs", "two", "two");
        },
        &step,
    );
    assert!(
        !seen.tree.iter().any(|l| l.starts_with("f hook-ran")),
        "the planted hook did not run: {:?}",
        seen.tree
    );
    assert!(
        seen.tree.contains(&"f kept-angular-cache/cached.bin \"keep\"".to_string()),
        "the link's target survives: {:?}",
        seen.tree
    );
}

#[test]
fn kept_clone_a_phase_that_hangs_ends_in_one_warning_and_never_fails_the_job() {
    // A `git fetch` that sleeps: the phase is bounded (1 s), then the work folder is expanded to the profile.
    let real = String::from_utf8(paddock_proc::command("sh").args(["-c", "command -v git"]).output().unwrap().stdout).unwrap();
    let setup = move |fx: &Fx| {
        fx.script(
            "bin/git",
            &format!(
                "case \" $* \" in *\" fetch \"*) exec sleep 30;; esac\nexec '{}' \"$@\"",
                real.trim()
            ),
        )
    };
    let seen = kept_once(
        &setup,
        &kept_step("macOS", "lean", vec![("PADDOCK_KEPT_CLONE_TIMEOUT", "1".into())]),
    );
    assert_eq!(seen.code, 0, "{seen:?}");
    assert!(seen.stdout.contains("did not finish within 1 s"), "{}", seen.stdout);
    assert!(seen.stdout.contains("building from the work folder"), "{}", seen.stdout);
    assert!(!seen.stdout.contains("::error::"), "{}", seen.stdout);
}

// ---- boot-volume ----

/// What a Mac's `df`, `diskutil` and `getconf` answer, by folder: a path under `ext` is on the external volume `/Volumes/Ext Disk`, any
/// other on `/`. The programs are shims (the test never asks the machine it runs on); their answers are fixed by the fixture, not by the
/// environment, as the tool starts them with a cleared one.
struct Mac {
    /// `df` names no mount.
    no_mount: bool,
    /// `diskutil` fails.
    no_plist: bool,
    /// What `getconf DARWIN_USER_TEMP_DIR` prints (`<ROOT>` filled in); empty: it fails.
    getconf: &'static str,
}

const MAC: Mac = Mac {
    no_mount: false,
    no_plist: false,
    getconf: "<ROOT>/home/internal-temp/",
};

fn boot_machine(fx: &Fx, mac: &Mac) {
    for d in ["ext/tmp", "home/internal-temp"] {
        std::fs::create_dir_all(fx.p(d)).unwrap();
    }
    let header = "echo 'Filesystem 512-blocks Used Available Capacity Mounted on'";
    let df = if mac.no_mount {
        header.to_string()
    } else {
        format!("case \"$2\" in */ext|*/ext/*) m='/Volumes/Ext Disk';; *) m='/';; esac\n{header}\necho \"/dev/disk9s1 100 1 99 1% $m\"")
    };
    fx.script("bin/df", &df);
    let plist = if mac.no_plist {
        "exit 1".to_string()
    } else {
        "case \"$*\" in *Ext*) v='<false/>';; *) v='<true/>';; esac\nprintf '<dict>\\n\\t<key>Internal</key>\\n\\t%s\\n</dict>\\n' \"$v\""
            .to_string()
    };
    fx.script("bin/diskutil", &plist);
    let getconf = if mac.getconf.is_empty() {
        "exit 1".to_string()
    } else {
        format!("printf '%s\\n' '{}'", fill(mac.getconf, fx))
    };
    fx.script("bin/getconf", &getconf);
}

fn boot(args: &[&'static str], extra: Vec<(&'static str, String)>) -> Step {
    let mut shell = vec!["conformance/ci/boot-volume.sh"];
    shell.extend_from_slice(args);
    let mut rust = vec!["boot-volume"];
    rust.extend_from_slice(args);
    Step {
        shell,
        shell_env: vec![],
        rust,
        env: extra,
        cwd: None,
    }
}

#[test]
fn boot_volume_says_which_volume_a_folder_is_on_and_where_the_boot_volumes_temp_folder_is() {
    let mac = |m: Mac| move |fx: &Fx| boot_machine(fx, &m);
    // `external`: by the exit code; a folder that is not made yet sits on the volume of its nearest parent.
    for (folder, code) in [
        ("<ROOT>/ext/tmp", 0),
        ("<ROOT>/home", 1),
        ("<ROOT>/ext/not/yet/made", 0),
        ("<ROOT>/home/not/yet/made", 1),
    ] {
        let seen = same(&mac(MAC), &boot(&["external", folder], vec![]));
        assert_eq!(seen.code, code, "{folder}: {seen:?}");
    }
    // `temp`: an internal folder stays (a trailing slash goes), an external one moves to the user's temp folder of the boot volume.
    let kept = same(&mac(MAC), &boot(&["temp", "<ROOT>/home/"], vec![]));
    assert_eq!(kept.stdout, "<ROOT>/home");
    let moved = same(&mac(MAC), &boot(&["temp", "<ROOT>/ext/tmp"], vec![]));
    assert!(moved.stdout.ends_with("\n<ROOT>/home/internal-temp"), "{moved:?}");
    assert!(
        moved
            .stdout
            .contains("boot-volume: using <ROOT>/home/internal-temp instead of <ROOT>/ext/tmp: "),
        "{moved:?}"
    );
    // No folder given: $TMPDIR.
    let from_env = same(&mac(MAC), &boot(&["temp"], vec![("TMPDIR", "<ROOT>/ext/tmp".into())]));
    assert!(from_env.stdout.ends_with("\n<ROOT>/home/internal-temp"), "{from_env:?}");
    // getconf fails, names a relative folder, a missing one, or an external one: the folder stays, with the warning.
    for getconf in ["", "relative", "<ROOT>/missing/", "<ROOT>/ext/tmp"] {
        let seen = same(&mac(Mac { getconf, ..MAC }), &boot(&["temp", "<ROOT>/ext/tmp"], vec![]));
        assert!(seen.stdout.ends_with("\n<ROOT>/ext/tmp"), "{getconf}: {seen:?}");
        assert!(
            seen.stdout.contains(
                "::warning::boot-volume: <ROOT>/ext/tmp is on an external volume and getconf DARWIN_USER_TEMP_DIR gave no internal folder ("
            ),
            "{getconf}: {seen:?}"
        );
    }
    // What cannot be judged is internal, with the reason on standard error; a failing diskutil is internal without one.
    let nomount = same(&mac(Mac { no_mount: true, ..MAC }), &boot(&["external", "<ROOT>/ext/tmp"], vec![]));
    assert_eq!(nomount.code, 1);
    assert!(nomount.stdout.contains("df names no mount; treated as internal"), "{nomount:?}");
    let noplist = same(&mac(Mac { no_plist: true, ..MAC }), &boot(&["external", "<ROOT>/ext/tmp"], vec![]));
    assert_eq!((noplist.code, noplist.stdout.as_str()), (1, ""));
}

// ---- guardian-as-user ----

/// A `crates` folder with a `cargo` that builds two unit-test binaries (fake programs) and a private temp folder. `proc_exit` is what
/// the paddock-proc test binary exits with; a failing one leaves the guardian's log, as the real tests do.
fn guardian_machine(fx: &Fx, proc_exit: i32) {
    std::fs::create_dir_all(fx.p("tmp")).unwrap();
    fx.shim(
        "cargo",
        "echo '   Compiling paddock-proc v0.1.0'\n\
         echo '  Executable unittests src/lib.rs (target/debug/deps/paddock_proc-0123abcd)' >&2\n\
         echo '  Executable tests/it/main.rs (target/debug/deps/it-ff)' >&2\n\
         echo '  Executable unittests src/lib.rs (target/debug/deps/paddock_chromium-9f8e7d)' >&2",
    );
    fx.script(
        "crates/target/debug/deps/paddock_proc-0123abcd",
        &format!(
            "echo \"proc tests: $*\"\n\
             [ {proc_exit} = 0 ] || {{ echo 'guardian said no' > \"$PADDOCK_PROC_GUARDIAN_LOG\"; exit {proc_exit}; }}"
        ),
    );
    fx.script("crates/target/debug/deps/paddock_chromium-9f8e7d", "echo \"chromium tests: $*\"");
}

fn guardian() -> Step {
    Step {
        shell: vec!["conformance/ci/guardian-as-user.sh"],
        shell_env: vec![],
        rust: vec!["guardian-as-user"],
        // The summary file is read as `github-path` so the comparison sees it.
        env: vec![
            ("TMPDIR", "<ROOT>/tmp".into()),
            ("GITHUB_STEP_SUMMARY", "<ROOT>/github-path".into()),
        ],
        cwd: Some("crates"),
    }
}

/// bash before 4.4 (macOS ships 3.2) fails `"${as_user[@]}"` of an empty array under `set -u`: the script is for the Linux image (bash 5).
fn bash_too_old_for_the_script() -> bool {
    let out = paddock_proc::command("bash")
        .args(["-c", "echo $((BASH_VERSINFO[0] * 100 + BASH_VERSINFO[1]))"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse::<u32>().unwrap_or(0) < 404
}

/// The Rust command alone on a fixture (the checks of its own output).
fn guardian_rust(setup: &dyn Fn(&Fx)) -> Seen {
    let fx = fixture();
    setup(&fx);
    run_one(&fx, &guardian(), true)
}

fn guardian_broken_build(fx: &Fx) {
    guardian_machine(fx, 0);
    fx.shim("cargo", "echo 'error[E0425]: nothing here' >&2; exit 101");
}

/// The script against the command on the same three fixtures. On a bash older than 4.4 the shell side cannot run, so the case is not
/// measured: it returns early with the reason printed (an env-gated test in the gate register, like the repo's other environment-gated
/// tests) unless `PADDOCK_REQUIRE_SHELL_PARITY` is set, which turns "not measured" into a failure (a Linux leg sets it).
#[test]
fn guardian_as_user_matches_the_shell() {
    if bash_too_old_for_the_script() {
        assert!(
            std::env::var_os("PADDOCK_REQUIRE_SHELL_PARITY").is_none(),
            "NOT MEASURED: bash is older than 4.4 and PADDOCK_REQUIRE_SHELL_PARITY demands the shell side"
        );
        eprintln!(
            "NOT MEASURED: guardian-as-user.sh parity (bash < 4.4: empty-array expansion under set -u); set PADDOCK_REQUIRE_SHELL_PARITY=1 to fail instead"
        );
        return;
    }
    same(&|fx| guardian_machine(fx, 0), &guardian());
    same(&|fx| guardian_machine(fx, 3), &guardian());
    same(&guardian_broken_build, &guardian());
}

#[test]
fn guardian_as_user_builds_the_unit_tests_and_runs_the_two_guardian_cases_as_the_calling_user() {
    // Run by a user (the test is not root: no setpriv, no sudo, ER-14), the tests run directly; the root path is the unit tests' (a fake
    // machine records the setpriv command line).
    let seen = guardian_rust(&|fx| guardian_machine(fx, 0));
    assert_eq!(seen.code, 0, "{seen:?}");
    assert!(
        seen.stdout.contains("Building the paddock-proc and paddock-chromium unit tests"),
        "{seen:?}"
    );
    assert!(seen.stdout.contains("proc tests: guardian::"), "{seen:?}");
    assert!(
        seen.stdout
            .contains("chromium tests: launch::tests::a_browser_whose_owner_is_killed"),
        "{seen:?}"
    );
    assert!(seen.stdout.contains("The guardian's acting tests passed as uid "), "{seen:?}");
    assert!(seen.github_path.contains("| `guardian::` | uid "), "{seen:?}");
    assert!(
        seen.calls
            .starts_with("cargo test --no-run --all-features -p paddock-proc -p paddock-chromium"),
        "{seen:?}"
    );
    // A failing test: its guardian log, one error line with the command, the summary row.
    let red = guardian_rust(&|fx| guardian_machine(fx, 3));
    assert_eq!(red.code, 1, "{red:?}");
    assert!(red.stdout.contains("The guardian's log:\nguardian said no"), "{red:?}");
    assert_eq!(red.stdout.lines().filter(|l| l.starts_with("::error::")).count(), 1, "{red:?}");
    assert!(
        red.stdout.contains("::error::guardian-as-user: `guardian::` failed as uid "),
        "{red:?}"
    );
    assert!(red.github_path.contains("failed (exit 3)"), "{red:?}");
    // A build that fails: its last lines, then one error line.
    let broken = guardian_rust(&guardian_broken_build);
    assert_eq!(broken.code, 1);
    assert!(broken.stdout.contains("error[E0425]: nothing here"), "{broken:?}");
    assert!(
        broken.stdout.contains("::error::guardian-as-user: cargo test --no-run failed"),
        "{broken:?}"
    );
}
