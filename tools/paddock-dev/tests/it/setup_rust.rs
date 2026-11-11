//! `dev ci setup-rust` against the shell it replaces: the "Install sccache" and "sccache" steps are read from
//! `.gitea/actions/setup-rust/action.yml` and run as the runner runs them (`bash -e -o pipefail`), then the Rust command runs on
//! the same inputs, the same folders and the same shim `sccache`. What both wrote to `$GITHUB_ENV` and `$GITHUB_PATH`, the exit
//! code and every call of sccache (its arguments and the variables that decide its server) must be identical. Cases: the
//! host runner's slots and budgets, a container with and without the `/sccache` mount, Redis, a server that does not start or
//! answer, wrong inputs, no sccache at all, and a pinned copy already in the tool cache. Unix only: the Windows runner runs the
//! real thing on every run (the shell's cygpath branch has no Unix stand-in).
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// The `run: |` body of a step of the action, unindented, with the action's path filled in.
fn step_body(name: &str) -> String {
    let action = repo().join(".gitea/actions/setup-rust");
    let text = std::fs::read_to_string(action.join("action.yml")).unwrap();
    let at = text
        .find(&format!("- name: {name}\n"))
        .unwrap_or_else(|| panic!("step {name} is in the action"));
    let rest = &text[at..];
    let end = rest[1..].find("\n    - ").map_or(rest.len(), |e| e + 1);
    let block = &rest[..end];
    let body = &block[block.find("run: |\n").unwrap() + "run: |\n".len()..];
    body.lines()
        .map(|l| l.get(8..).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("${{ github.action_path }}", &action.to_string_lossy())
}

/// One case: a scratch runner (home, temp, tool cache, the job's env and path files) and a shim sccache.
struct Runner {
    root: PathBuf,
    bin: PathBuf,
    calls: PathBuf,
    _dir: tempfile::TempDir,
}

#[derive(Debug, PartialEq, Eq)]
struct Result {
    code: i32,
    env: String,
    path: String,
    calls: String,
    out: String,
}

impl Runner {
    /// `shim`: shell lines run by the fake sccache before it exits 0 (`None`: no sccache on PATH).
    fn new(shim: Option<&str>) -> Runner {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for d in ["home", "temp", "tools"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let calls = root.join("calls");
        if let Some(shim) = shim {
            write_shim(&bin.join("sccache"), &calls, shim);
        }
        Runner {
            root,
            bin,
            calls,
            _dir: dir,
        }
    }

    fn base_env(&self, extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> = [
            ("PATH", format!("{}:/usr/bin:/bin", self.bin.display())),
            ("HOME", self.root.join("home").display().to_string()),
            ("TMPDIR", self.root.join("temp").display().to_string()),
            ("RUNNER_TEMP", self.root.join("temp").display().to_string()),
            ("RUNNER_TOOL_CACHE", self.root.join("tools").display().to_string()),
            ("RUNNER_OS", "macOS".to_string()),
            ("PADDOCK_CI_SLOT", "s1".to_string()),
            ("GITHUB_ENV", self.root.join("github-env").display().to_string()),
            ("GITHUB_PATH", self.root.join("github-path").display().to_string()),
            // No test reaches the network: the Rust download goes here (the shell's install is only run where it never downloads).
            ("PADDOCK_SCCACHE_RELEASES", format!("file://{}/no-releases", self.root.display())),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        for (k, v) in extra {
            env.retain(|(key, _)| key != k);
            env.push((k.to_string(), v.to_string()));
        }
        env
    }

    fn reset(&self) {
        for f in ["github-env", "github-path"] {
            std::fs::write(self.root.join(f), "").unwrap();
        }
        let _ = std::fs::remove_file(&self.calls);
    }

    fn collect(&self, code: i32, out: String) -> Result {
        let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_default();
        Result {
            code,
            env: read(self.root.join("github-env")),
            path: read(self.root.join("github-path")),
            calls: read(self.calls.clone()),
            out,
        }
    }

    /// The shell steps, in the action's order, as separate steps (a failed step stops the job).
    #[allow(clippy::disallowed_methods)]
    fn shell(&self, input: &Input, extra: &[(&str, &str)], with_install: bool) -> Result {
        self.reset();
        let mut steps = Vec::new();
        if with_install {
            steps.push(step_body("Install sccache"));
        }
        steps.push(step_body("sccache"));
        let (mut code, mut out) = (0, String::new());
        for body in steps {
            let script = self.root.join("step.sh");
            std::fs::write(&script, body).unwrap();
            let r = Command::new("bash")
                .args(["-e", "-o", "pipefail"])
                .arg(&script)
                .env_clear()
                .envs(self.base_env(extra))
                .env("BACKEND", input.backend)
                .env("REDIS_ENDPOINT", input.endpoint)
                .env("REDIS_DB", input.db)
                .output()
                .unwrap();
            out.push_str(&String::from_utf8_lossy(&r.stdout));
            out.push_str(&String::from_utf8_lossy(&r.stderr));
            code = r.status.code().unwrap_or(-1);
            if code != 0 {
                break;
            }
        }
        self.collect(code, out)
    }

    #[allow(clippy::disallowed_methods)]
    fn rust(&self, input: &Input, extra: &[(&str, &str)]) -> Result {
        self.reset();
        let r = Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
            .args([
                "ci",
                "setup-rust",
                "--sccache-backend",
                input.backend,
                "--sccache-redis-db",
                input.db,
            ])
            .current_dir(repo())
            .env_clear()
            .envs(self.base_env(extra))
            .env("PADDOCK_SCCACHE_REDIS_ENDPOINT", input.endpoint)
            .output()
            .unwrap();
        let out = format!("{}{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr));
        self.collect(r.status.code().unwrap_or(-1), out)
    }
}

fn write_shim(path: &Path, calls: &Path, shim: &str) {
    let body = format!(
        "#!/bin/sh\necho \"$* port=$SCCACHE_SERVER_PORT io=$SCCACHE_IGNORE_SERVER_IO_ERROR dir=${{SCCACHE_DIR:-}} size=${{SCCACHE_CACHE_SIZE:-}} redis=${{SCCACHE_REDIS_ENDPOINT:-}} db=${{SCCACHE_REDIS_DB:-}}\" >> '{}'\n[ \"$1\" = --version ] && echo sccache 0.19.0\n{shim}\nexit 0\n",
        calls.display()
    );
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Input {
    backend: &'static str,
    endpoint: &'static str,
    db: &'static str,
}

const LOCAL: Input = Input {
    backend: "local",
    endpoint: "",
    db: "",
};

/// The two implementations agree on everything that reaches the job: exit code, `$GITHUB_ENV`, `$GITHUB_PATH`, sccache calls.
fn same(runner: &Runner, input: &Input, extra: &[(&str, &str)], with_install: bool) -> (Result, Result) {
    let sh = runner.shell(input, extra, with_install);
    let rs = runner.rust(input, extra);
    assert_eq!(
        (sh.code, &sh.env, &sh.path, &sh.calls),
        (rs.code, &rs.env, &rs.path, &rs.calls),
        "shell output:\n{}\nrust output:\n{}",
        sh.out,
        rs.out
    );
    (sh, rs)
}

#[test]
fn a_host_slot_gets_its_own_folder_port_and_share_of_the_budget() {
    let r = Runner::new(Some("true"));
    let (_, rs) = same(&r, &LOCAL, &[], true);
    assert!(rs.env.contains("RUSTC_WRAPPER=sccache\nCARGO_INCREMENTAL=0\n"), "{}", rs.env);
    assert!(
        rs.env.contains("SCCACHE_SERVER_PORT=4227\n") && rs.env.contains("/.paddock-ci/sccache-s1\n"),
        "{}",
        rs.env
    );
    assert!(rs.env.contains("SCCACHE_CACHE_SIZE=10G\n"), "{}", rs.env);
    assert!(rs.calls.contains("--start-server port=4227"), "{}", rs.calls);
    let (_, rs) = same(
        &r,
        &LOCAL,
        &[
            ("PADDOCK_CI_SLOT", "s2"),
            ("PADDOCK_CI_SLOTS", "3"),
            ("PADDOCK_CI_SCCACHE_GB", "60"),
        ],
        true,
    );
    assert!(
        rs.env.contains("SCCACHE_SERVER_PORT=4228\n") && rs.env.contains("SCCACHE_CACHE_SIZE=20G\n"),
        "{}",
        rs.env
    );
    same(
        &r,
        &LOCAL,
        &[("PADDOCK_CI_SLOTS", "40"), ("PADDOCK_CI_SCCACHE_GB", "not-a-number")],
        true,
    );
}

#[test]
fn a_container_uses_the_mount_or_a_folder_that_dies_with_it() {
    let r = Runner::new(Some("true"));
    let mount = r.root.join("mount");
    std::fs::create_dir_all(&mount).unwrap();
    let mount = mount.display().to_string();
    let (_, rs) = same(&r, &LOCAL, &[("RUNNER_OS", "Linux"), ("PADDOCK_SCCACHE_MOUNT", &mount)], true);
    assert!(
        rs.env.contains(&format!("SCCACHE_DIR={mount}\n")) && rs.env.contains("SCCACHE_CACHE_SIZE=20G\n"),
        "{}",
        rs.env
    );
    let absent = r.root.join("absent").display().to_string();
    let (_, rs) = same(&r, &LOCAL, &[("RUNNER_OS", "Linux"), ("PADDOCK_SCCACHE_MOUNT", &absent)], true);
    assert!(rs.env.contains("/temp/sccache\n"), "{}", rs.env);
    assert!(rs.out.contains("::notice::no /sccache mount in this container"), "{}", rs.out);
}

#[test]
fn redis_is_used_when_asked_for_or_when_auto_has_an_endpoint() {
    let r = Runner::new(Some("true"));
    let auto = Input {
        backend: "auto",
        endpoint: "redis://h:6379",
        db: "",
    };
    let (_, rs) = same(&r, &auto, &[], true);
    assert!(
        rs.env.contains("SCCACHE_REDIS_ENDPOINT=redis://h:6379\nSCCACHE_REDIS_DB=5\n"),
        "{}",
        rs.env
    );
    assert!(!rs.env.contains("SCCACHE_DIR"), "{}", rs.env);
    let redis = Input {
        backend: "redis",
        endpoint: "redis://:pw@h:6379",
        db: "7",
    };
    let (_, rs) = same(&r, &redis, &[("RUNNER_OS", "Linux")], true);
    assert!(rs.env.contains("SCCACHE_REDIS_DB=7\n"), "{}", rs.env);
    assert!(!rs.out.contains(":pw@"), "the password is never printed: {}", rs.out);
    same(&r, &Input { endpoint: "", ..auto }, &[], true);
}

#[test]
fn a_server_that_does_not_start_or_answer_leaves_the_wrapper_unset_and_the_step_green() {
    for shim in ["[ \"$1\" = --start-server ] && exit 1", "[ \"$1\" = --show-stats ] && exit 1"] {
        let r = Runner::new(Some(shim));
        let (_, rs) = same(&r, &LOCAL, &[], true);
        assert_eq!((rs.code, rs.env.as_str()), (0, ""), "{shim}");
        assert!(rs.out.contains("::warning::the sccache server did not start"), "{}", rs.out);
    }
}

#[test]
fn wrong_inputs_fail_the_step_with_the_shells_message() {
    let r = Runner::new(Some("true"));
    let (_, rs) = same(
        &r,
        &Input {
            backend: "disk",
            endpoint: "",
            db: "",
        },
        &[],
        true,
    );
    assert_eq!(rs.code, 1);
    assert!(
        rs.out
            .contains("::error::sccache-backend must be auto, redis or local (got 'disk')"),
        "{}",
        rs.out
    );
    let (_, rs) = same(
        &r,
        &Input {
            backend: "redis",
            endpoint: "",
            db: "",
        },
        &[],
        true,
    );
    assert!(
        rs.out
            .contains("::error::sccache-backend is redis but sccache-redis-endpoint is empty"),
        "{}",
        rs.out
    );
}

#[test]
fn without_sccache_the_job_builds_without_the_cache() {
    // The shell's download cannot be pointed away from GitHub, so only its sccache step runs here; the Rust download goes to
    // a folder that has no release and fails, which is a warning.
    let r = Runner::new(None);
    let (_, rs) = same(&r, &LOCAL, &[], false);
    assert_eq!((rs.code, rs.env.as_str(), rs.path.as_str()), (0, "", ""));
    assert!(rs.out.contains("::warning::could not download sccache 0.19.0"), "{}", rs.out);
    assert!(rs.out.contains("::warning::sccache is not available on this runner"), "{}", rs.out);
}

#[test]
fn a_pinned_copy_in_the_tool_cache_is_put_on_path_and_used() {
    let r = Runner::new(None);
    let pinned = r.root.join("tools/sccache/0.19.0");
    std::fs::create_dir_all(&pinned).unwrap();
    write_shim(&pinned.join("sccache"), &r.calls, "true");
    let (_, rs) = same(&r, &LOCAL, &[], true);
    assert_eq!(rs.path, format!("{}\n", pinned.display()));
    assert!(rs.env.contains("RUSTC_WRAPPER=sccache\n"), "{}", rs.env);
}

#[test]
fn a_download_with_the_wrong_checksum_fails_the_step() {
    // Rust only (see above): a release folder that holds garbage under the pinned name.
    let r = Runner::new(None);
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        _ => return,
    };
    let os = if cfg!(target_os = "macos") { "macOS" } else { "Linux" };
    let rel = r.root.join("no-releases/v0.19.0");
    std::fs::create_dir_all(&rel).unwrap();
    std::fs::write(rel.join(format!("sccache-v0.19.0-{target}.tar.gz")), b"not an archive").unwrap();
    let rs = r.rust(&LOCAL, &[("RUNNER_OS", os)]);
    assert_eq!(rs.code, 1, "{}", rs.out);
    assert!(
        rs.out.contains(&format!("::error::sccache 0.19.0 checksum mismatch for {target}")),
        "{}",
        rs.out
    );
    assert!(rs.out.contains("Next step:"), "{}", rs.out);
    assert_eq!(rs.path, "", "nothing is put on PATH");
    assert!(!r.root.join("tools/sccache/0.19.0/sccache").exists());
}
