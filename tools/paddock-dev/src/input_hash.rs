//! `dev self hash`: the content address of the tool's binary (ADR 0022 §3.3).
//!
//! The hash is `git hash-object --stdin` over `git ls-tree HEAD -- <inputs>` followed by the `commit-hash:` and `release:`
//! lines of `<rustc> -vV` (in rustc's order) and [`flag_lines`] (the hashed environment variables and a digest of each cargo config
//! file), where `<inputs>` are the paths in `tools/paddock-dev/inputs.txt` and `<rustc>`
//! is the compiler cargo will use. The `./dev` wrapper computes the same value in shell (`./dev --wrapper-hash`); a test
//! runs both on one repository. When an input has uncommitted changes (`git status --porcelain --untracked-files=all`), the
//! hash cannot describe the files and the wrapper builds.

use paddock_cli::{CliError, Report};
use paddock_proc::{Limits, command as proc_command, run_bounded};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

/// The hash the binary was built from, when the wrapper passed it (`PADDOCK_DEV_HASH`).
pub const EMBEDDED: Option<&str> = option_env!("PADDOCK_DEV_EMBEDDED_HASH");

const TIMEOUT: Duration = Duration::from_secs(60);

/// The input paths of `inputs.txt`: one per line, `#` comments and blank lines skipped, no spaces, no absolute or `..` paths.
pub fn parse_inputs(text: &str) -> Result<Vec<String>, CliError> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let bad = line.contains(char::is_whitespace) || line.starts_with('/') || line.split('/').any(|p| p == "..") || line.contains('\\');
        if bad {
            return Err(
                CliError::failure(format!("inputs.txt: {line:?} is not a plain repository-relative path."))
                    .with_hint("Write one path per line, with / separators, no spaces and no `..`."),
            );
        }
        out.push(line.to_string());
    }
    Ok(out)
}

/// The `commit-hash:` and `release:` lines of `rustc -vV`, in the order rustc prints them, each ending in a newline.
pub fn compiler_lines(vv: &str) -> String {
    vv.lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| l.starts_with("commit-hash:") || l.starts_with("release:"))
        .fold(String::new(), |mut acc, l| {
            acc.push_str(l);
            acc.push('\n');
            acc
        })
}

/// Whether an environment variable changes the compiled code and so is hashed: `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`,
/// `CARGO_BUILD_*`, `CARGO_PROFILE_*` and `CARGO_TARGET_*`, except the build folder, the job count and the compiler (the `-vV` lines name that).
pub fn is_hashed_var(name: &str) -> bool {
    let listed = name == "RUSTFLAGS"
        || name == "CARGO_ENCODED_RUSTFLAGS"
        || name.starts_with("CARGO_BUILD_")
        || name.starts_with("CARGO_PROFILE_")
        || name.starts_with("CARGO_TARGET_");
    listed
        && !matches!(
            name,
            "CARGO_TARGET_DIR" | "CARGO_BUILD_JOBS" | "CARGO_BUILD_TARGET_DIR" | "CARGO_BUILD_RUSTC"
        )
}

/// What else changes the compiled code, as text: the hashed environment variables that are not empty as `NAME=value` lines
/// in byte order, then one `config-file <digest>` line per config, the digest being [`config_digest`]'s. The wrapper builds
/// the same text with `env | grep | sort` and `git hash-object`; trailing newlines are the caller's to trim. Configs are
/// reduced to digests because they can hold registry tokens, which must not travel in text a trace could print.
pub fn flag_lines(vars: &[(String, String)], config_digests: &[String]) -> String {
    let mut lines: Vec<String> = vars
        .iter()
        .filter(|(k, v)| is_hashed_var(k) && !v.is_empty())
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    lines.sort();
    let mut out: String = lines.iter().map(|l| format!("{l}\n")).collect();
    for d in config_digests {
        out.push_str(&format!("config-file {d}\n"));
    }
    out
}

/// Git's blob id of a config's text with carriage returns dropped.
fn config_digest(exec: &dyn Exec, root: &Path, text: &str) -> Result<String, CliError> {
    let out = exec.run("git", &["hash-object", "--stdin"], root, Some(text.replace('\r', "").as_bytes()))?;
    Ok(out.trim().to_string())
}

/// The `rustc = "..."` of a `[build]` table (or a top-level dotted `build.rustc = `) in a cargo config, read line by line
/// exactly as the wrapper's awk does (so an inline table is not seen; the wrapper has the same limit).
pub fn build_rustc(config: &str) -> Option<String> {
    let mut section = String::new();
    for line in config.lines() {
        let trimmed = line.trim_start_matches([' ', '\t']);
        if trimmed.starts_with('[') {
            section = line.chars().filter(|c| !matches!(c, ' ' | '\t')).collect();
            continue;
        }
        let (key, wanted_section) = if section.is_empty() {
            ("build.rustc", "")
        } else {
            ("rustc", "[build]")
        };
        let Some(rest) = trimmed.strip_prefix(key) else { continue };
        let Some(value) = rest.trim_start_matches([' ', '\t']).strip_prefix('=') else {
            continue;
        };
        if section != wanted_section {
            continue;
        }
        let value = value.trim_start_matches([' ', '\t']);
        let value = value.strip_prefix('"').unwrap_or(value);
        let end = value
            .char_indices()
            .filter(|(_, c)| *c == '"')
            .map(|(i, _)| i)
            .find(|i| {
                let after = value[i + 1..].trim_start_matches([' ', '\t']);
                after.is_empty() || after.starts_with('#')
            })
            .unwrap_or(value.len());
        return Some(value[..end].to_string());
    }
    None
}

/// The compiler program cargo will use: `RUSTC`, `CARGO_BUILD_RUSTC`, then the first config (closest first) with a
/// `[build] rustc`, else `rustc`.
pub fn resolve_rustc(env: &dyn Fn(&str) -> Option<String>, configs: &[String]) -> String {
    let non_empty = |k: &str| env(k).filter(|v| !v.is_empty());
    non_empty("RUSTC")
        .or_else(|| non_empty("CARGO_BUILD_RUSTC"))
        .or_else(|| configs.iter().find_map(|c| build_rustc(c)))
        .unwrap_or_else(|| "rustc".to_string())
}

/// The config files cargo reads for the tool, closest first, as the wrapper lists them: `config.toml` and `config` in every
/// `.cargo` folder from the tool's own up to the filesystem root, the other `*.toml` files of the tool's `.cargo` (what
/// `include` pulls in), then `$CARGO_HOME`'s; only files that exist, without duplicates.
pub fn config_files(root: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let tool = root.join("tools/paddock-dev");
    let mut out: Vec<PathBuf> = Vec::new();
    let mut add = |p: PathBuf| {
        if p.is_file() && !out.contains(&p) {
            out.push(p);
        }
    };
    for dir in tool.ancestors() {
        let cargo = dir.join(".cargo");
        add(cargo.join("config.toml"));
        add(cargo.join("config"));
        if dir == tool {
            let mut others: Vec<_> = std::fs::read_dir(&cargo)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.file_name())
                .filter(|n| n.to_string_lossy().ends_with(".toml") && n != "config.toml")
                .collect();
            others.sort();
            others.into_iter().for_each(|n| add(cargo.join(n)));
        }
    }
    let cargo_home = env("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").or_else(|| env("USERPROFILE")).map(|h| PathBuf::from(h).join(".cargo")));
    if let Some(home) = cargo_home {
        add(home.join("config.toml"));
        add(home.join("config"));
    }
    out
}

/// Runs programs for the hash: a port, so the computation is testable without a repository.
pub trait Exec {
    /// Runs `program args...` in `cwd` with `stdin` and returns standard output, or a message when it failed.
    fn run(&self, program: &str, args: &[&str], cwd: &Path, stdin: Option<&[u8]>) -> Result<String, CliError>;
}

/// Feeds `bytes` to the child's standard input (on its own thread, so a large input cannot stall the wait) and waits for the
/// child for at most `limit`; a child still running then is killed and reported as an error.
fn pipe_in(mut child: std::process::Child, bytes: &[u8], limit: Duration) -> std::io::Result<std::process::Output> {
    let stdin = child.stdin.take();
    let deadline = std::time::Instant::now() + limit;
    std::thread::scope(|s| {
        s.spawn(move || {
            if let Some(mut pipe) = stdin {
                // The child may end before reading everything; its exit status says what went wrong.
                let _ = pipe.write_all(bytes);
            }
        });
        loop {
            if child.try_wait()?.is_some() {
                return child.wait_with_output();
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("it did not finish within {} seconds", limit.as_secs()),
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })
}

/// The real thing, through `paddock-proc` (stdin only for `git hash-object`, which reads it and ends).
pub struct Real;

impl Exec for Real {
    fn run(&self, program: &str, args: &[&str], cwd: &Path, stdin: Option<&[u8]>) -> Result<String, CliError> {
        let fail = |why: String| {
            CliError::failure(format!("{program} {}: {why}", args.first().unwrap_or(&"")))
                .with_hint("Check that git and rustc work in this repository.")
        };
        let mut cmd = proc_command(program);
        cmd.args(args).current_dir(cwd);
        if program == "git" {
            // Untranslated messages: callers and tests match on text.
            cmd.env("LC_ALL", "C");
        }
        let out = match stdin {
            None => {
                let out = run_bounded(cmd, Limits::new(TIMEOUT)).map_err(|e| fail(e.to_string()))?;
                if !out.success() {
                    return Err(fail(out.stderr_text().trim().to_string()));
                }
                out.stdout_text()
            }
            Some(bytes) => {
                cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
                let out = cmd
                    .spawn()
                    .and_then(|child| pipe_in(child, bytes, TIMEOUT))
                    .map_err(|e| fail(e.to_string()))?;
                if !out.status.success() {
                    return Err(fail(String::from_utf8_lossy(&out.stderr).trim().to_string()));
                }
                String::from_utf8_lossy(&out.stdout).into_owned()
            }
        };
        Ok(out)
    }
}

/// The computed input hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputHash {
    /// The content address.
    pub hash: String,
    /// The compiler lines that went into it.
    pub compiler: String,
    /// An input has uncommitted changes: the wrapper builds instead of using the hash.
    pub dirty: bool,
    /// The paths hashed.
    pub inputs: Vec<String>,
}

/// Computes the hash for the repository at `root`.
pub fn compute(
    exec: &dyn Exec,
    env: &dyn Fn(&str) -> Option<String>,
    vars: &[(String, String)],
    root: &Path,
) -> Result<InputHash, CliError> {
    let tool = root.join("tools/paddock-dev");
    let list = std::fs::read_to_string(tool.join("inputs.txt")).map_err(|e| {
        CliError::failure(format!("tools/paddock-dev/inputs.txt could not be read: {e}."))
            .with_hint("Run ./dev inside the Paddock repository.")
    })?;
    let inputs = parse_inputs(&list)?;
    let configs: Vec<String> = config_files(root, env)
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect();
    let rustc = resolve_rustc(env, &configs);
    let compiler = compiler_lines(&exec.run(&rustc, &["-vV"], &tool, None)?);
    let mut git_args: Vec<&str> = vec!["ls-tree", "HEAD", "--"];
    git_args.extend(inputs.iter().map(String::as_str));
    let tree = exec.run("git", &git_args, root, None)?;
    let digests = configs
        .iter()
        .map(|c| config_digest(exec, root, c))
        .collect::<Result<Vec<_>, _>>()?;
    let flags = flag_lines(vars, &digests);
    let hash = exec
        .run(
            "git",
            &["hash-object", "--stdin"],
            root,
            Some(format!("{tree}{compiler}{}\n", flags.trim_end_matches('\n')).as_bytes()),
        )?
        .trim()
        .to_string();
    let mut status_args: Vec<&str> = vec!["status", "--porcelain", "--untracked-files=all", "--"];
    status_args.extend(inputs.iter().map(String::as_str));
    let dirty = !exec.run("git", &status_args, root, None)?.trim().is_empty();
    Ok(InputHash {
        hash,
        compiler,
        dirty,
        inputs,
    })
}

/// `dev self hash`.
pub fn command() -> Result<Report, CliError> {
    // An unreadable current folder is an empty path: git then runs in the folder it inherited.
    let cwd = std::env::current_dir().unwrap_or_default();
    let env = |k: &str| std::env::var(k).ok();
    let root = Real
        .run("git", &["rev-parse", "--show-toplevel"], &cwd, None)
        .map_err(|e| e.with_hint("Run ./dev inside the Paddock repository."))?;
    let vars: Vec<(String, String)> = std::env::vars().collect();
    let h = compute(&Real, &env, &vars, Path::new(root.trim()))?;
    let wrapper = env("PADDOCK_DEV_WRAPPER_HASH");
    let text = format!(
        "hash     {}\nwrapper  {}\nembedded {}\ndirty    {}",
        h.hash,
        wrapper.as_deref().unwrap_or("-"),
        EMBEDDED.unwrap_or("-"),
        h.dirty
    );
    Ok(Report::new(
        json!({"hash": h.hash, "wrapper": wrapper, "embedded": EMBEDDED, "dirty": h.dirty, "compiler": h.compiler.lines().collect::<Vec<_>>(), "inputs": h.inputs}),
        text,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn inputs_skip_comments_and_refuse_odd_paths() {
        assert_eq!(parse_inputs("# c\n\n a/b \ncrates/x\n").unwrap(), ["a/b", "crates/x"]);
        for bad in ["/abs", "a/../b", "a b", "a\\b"] {
            assert!(parse_inputs(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_release_and_commit_hash_count_and_host_does_not() {
        let a = "rustc 1.99.0 (abc 2026-01-01)\nbinary: rustc\ncommit-hash: abc\ncommit-date: x\nhost: aarch64-apple-darwin\nrelease: 1.99.0\nLLVM version: 22\n";
        assert_eq!(compiler_lines(a), "commit-hash: abc\nrelease: 1.99.0\n");
        let other_host = a.replace("aarch64-apple-darwin", "x86_64-pc-windows-msvc").replace('\n', "\r\n");
        assert_eq!(compiler_lines(&other_host), compiler_lines(a));
        assert_ne!(compiler_lines(&a.replace("1.99.0", "1.99.1")), compiler_lines(a));
        assert_ne!(compiler_lines(&a.replace("abc", "abd")), compiler_lines(a));
    }

    #[test]
    fn build_rustc_reads_only_the_build_table() {
        assert_eq!(build_rustc("[build]\nrustc = \"/x/rustc\"\n").as_deref(), Some("/x/rustc"));
        assert_eq!(
            build_rustc("[target.x]\nrustc = \"no\"\n[ build ]\n  rustc=\"/y\"  # why\n").as_deref(),
            Some("/y")
        );
        assert_eq!(build_rustc("[build]\nrustc = \"a\\\"b\"\n").as_deref(), Some("a\\\"b"));
        assert_eq!(build_rustc("[build]\nrustc = \"/z\" \n").as_deref(), Some("/z"));
        assert_eq!(build_rustc("[build]\nrustc-wrapper = \"w\"\njobs = 2\n"), None);
        assert_eq!(build_rustc("rustc = \"top\"\n"), None);
        assert_eq!(build_rustc("build.rustc = \"/dotted\"\n[x]\n").as_deref(), Some("/dotted"));
        assert_eq!(build_rustc("[x]\nbuild.rustc = \"no\"\n"), None);
        assert_eq!(build_rustc("[build]\nrustc = bare\n").as_deref(), Some("bare"));
    }

    #[test]
    fn hashed_variables_and_whole_configs_change_the_hashed_text() {
        let var = |k: &str, v: &str| (k.to_string(), v.to_string());
        assert_eq!(flag_lines(&[], &[]), "");
        let vars = [
            var("RUSTFLAGS", "-C target-cpu=native"),
            var("CARGO_PROFILE_RELEASE_LTO", "fat"),
            var("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS", "-Cx"),
            var("CARGO_BUILD_TARGET", "wasm32-unknown-unknown"),
            var("CARGO_ENCODED_RUSTFLAGS", ""),
            var("CARGO_TARGET_DIR", "/ignored"),
            var("CARGO_BUILD_JOBS", "2"),
            var("PATH", "/bin"),
        ];
        assert_eq!(
            flag_lines(&vars, &[]),
            "CARGO_BUILD_TARGET=wasm32-unknown-unknown\nCARGO_PROFILE_RELEASE_LTO=fat\nCARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS=-Cx\nRUSTFLAGS=-C target-cpu=native\n"
        );
        let digests = vec!["d1".to_string(), "d2".to_string()];
        assert_eq!(flag_lines(&[], &digests), "config-file d1\nconfig-file d2\n");
    }

    #[test]
    fn rustc_resolution_order_is_env_then_config_then_path() {
        let env =
            |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        let cfg = vec![
            "[build]\nrustc = \"from-config\"\n".to_string(),
            "[build]\nrustc = \"farther\"\n".to_string(),
        ];
        assert_eq!(resolve_rustc(&env(&[("RUSTC", "r"), ("CARGO_BUILD_RUSTC", "c")]), &cfg), "r");
        assert_eq!(resolve_rustc(&env(&[("RUSTC", ""), ("CARGO_BUILD_RUSTC", "c")]), &cfg), "c");
        assert_eq!(resolve_rustc(&env(&[]), &cfg), "from-config");
        assert_eq!(
            resolve_rustc(&env(&[]), &["[build]\njobs = 1\n".to_string(), cfg[1].clone()]),
            "farther"
        );
        assert_eq!(resolve_rustc(&env(&[]), &[]), "rustc");
    }

    #[test]
    fn config_files_go_closest_first_include_the_tools_toml_files_and_end_with_cargo_home() {
        let d = tempfile::tempdir().unwrap();
        let (root, home) = (d.path().join("repo"), d.path().join("ch"));
        let w = |p: &Path, text: &str| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        w(&root.join("tools/paddock-dev/.cargo/config.toml"), "a");
        w(&root.join("tools/paddock-dev/.cargo/ci.toml"), "b");
        w(&root.join("tools/paddock-dev/.cargo/note.txt"), "c");
        w(&root.join(".cargo/config"), "d");
        w(&d.path().join(".cargo/config.toml"), "above the repository");
        w(&home.join("config.toml"), "e");
        let env = |k: &str| (k == "CARGO_HOME").then(|| home.display().to_string());
        let names: Vec<String> = config_files(&root, &env)
            .iter()
            .map(|p| p.strip_prefix(d.path()).unwrap().display().to_string().replace('\\', "/"))
            .collect();
        assert_eq!(
            names,
            [
                "repo/tools/paddock-dev/.cargo/config.toml",
                "repo/tools/paddock-dev/.cargo/ci.toml",
                "repo/.cargo/config",
                ".cargo/config.toml",
                "ch/config.toml"
            ]
        );
        // CARGO_HOME that is also an ancestor's `.cargo` is listed once; HOME is the fallback.
        let same = |k: &str| (k == "CARGO_HOME").then(|| d.path().join(".cargo").display().to_string());
        assert_eq!(
            config_files(&root, &same)
                .iter()
                .filter(|p| p.ends_with(".cargo/config.toml"))
                .count(),
            2
        );
        let via_home = |k: &str| (k == "HOME").then(|| d.path().display().to_string());
        assert_eq!(config_files(&root, &via_home).len(), 4);
    }

    /// Scripted programs: answers by program and first argument, and records the stdin of `hash-object`.
    struct Fake {
        rustc: String,
        tree: String,
        status: String,
        stdin_seen: RefCell<Vec<String>>,
    }

    impl Exec for Fake {
        fn run(&self, program: &str, args: &[&str], _cwd: &Path, stdin: Option<&[u8]>) -> Result<String, CliError> {
            match (program, args.first().copied()) {
                ("git", Some("ls-tree")) => Ok(self.tree.clone()),
                ("git", Some("status")) => Ok(self.status.clone()),
                ("git", Some("hash-object")) => {
                    let text = String::from_utf8_lossy(stdin.unwrap_or_default()).into_owned();
                    self.stdin_seen.borrow_mut().push(text.clone());
                    Ok(format!("hash-of-{}\n", text.len()))
                }
                (p, Some("-vV")) if p == self.rustc => Ok("commit-hash: c1\nhost: h\nrelease: 1.0\n".to_string()),
                _ => Err(CliError::failure("unexpected program")),
            }
        }
    }

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("tools/paddock-dev")).unwrap();
        std::fs::write(d.path().join("tools/paddock-dev/inputs.txt"), "# x\ntools/paddock-dev\ncrates/a\n").unwrap();
        d
    }

    #[test]
    fn compute_hashes_the_tree_and_the_compiler_lines_and_reports_dirt() {
        let d = repo();
        let fake = Fake {
            rustc: "rustc".into(),
            tree: "T\n".into(),
            status: " M tools/paddock-dev/src/x.rs\n".into(),
            stdin_seen: RefCell::default(),
        };
        let h = compute(&fake, &|_: &str| None, &[], d.path()).unwrap();
        assert_eq!(fake.stdin_seen.borrow()[0], "T\ncommit-hash: c1\nrelease: 1.0\n\n");
        assert!(h.dirty && h.inputs == ["tools/paddock-dev", "crates/a"] && h.hash.starts_with("hash-of-"));
        let clean = Fake {
            status: "\n".into(),
            ..fake
        };
        assert!(!compute(&clean, &|_: &str| None, &[], d.path()).unwrap().dirty);
    }

    #[test]
    fn compute_asks_the_compiler_named_by_the_environment() {
        let d = repo();
        let fake = Fake {
            rustc: "my-rustc".into(),
            tree: String::new(),
            status: String::new(),
            stdin_seen: RefCell::default(),
        };
        let env = |k: &str| (k == "RUSTC").then(|| "my-rustc".to_string());
        assert!(compute(&fake, &env, &[], d.path()).is_ok());
        assert!(compute(&fake, &|_: &str| None, &[], d.path()).is_err());
    }

    #[test]
    fn the_real_runner_reports_missing_programs_and_failures() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(
            Real.run("paddock-no-such-program", &["x"], here, None)
                .unwrap_err()
                .text()
                .contains("not available")
        );
        assert!(Real.run("paddock-no-such-program", &["x"], here, Some(b"in")).is_err());
        assert!(Real.run("git", &["definitely-not-a-git-command"], here, None).is_err());
        assert!(Real.run("git", &["hash-object", "--no-such-flag"], here, Some(b"in")).is_err());
        let blob = Real.run("git", &["hash-object", "--stdin"], here, Some(b"hello\n")).unwrap();
        assert_eq!(
            blob.trim(),
            "ce013625030ba8dba906f756967f9e9ca394464a",
            "git's blob id of \"hello\\n\""
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_reads_its_input_but_never_ends_is_killed_at_the_limit() {
        let mut cmd = proc_command("sh");
        cmd.args(["-c", "cat >/dev/null; sleep 30"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = cmd.spawn().unwrap();
        let err = pipe_in(child, b"x", Duration::from_millis(200)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    }

    #[test]
    fn every_step_that_fails_stops_the_hash_with_that_error() {
        let d = repo();
        let ok = Fake {
            rustc: "rustc".into(),
            tree: "T\n".into(),
            status: String::new(),
            stdin_seen: RefCell::default(),
        };
        assert!(compute(&ok, &|_: &str| None, &[], d.path()).is_ok());
        // Each program fails in turn.
        struct FailOn(&'static str);
        impl Exec for FailOn {
            fn run(&self, program: &str, args: &[&str], _: &Path, _: Option<&[u8]>) -> Result<String, CliError> {
                let step = if args.first() == Some(&"-vV") {
                    "rustc"
                } else {
                    args.first().copied().unwrap_or(program)
                };
                if step == self.0 {
                    Err(CliError::failure(format!("{step} failed")))
                } else {
                    Ok("x\n".to_string())
                }
            }
        }
        for step in ["rustc", "ls-tree", "hash-object", "status"] {
            let err = compute(&FailOn(step), &|_: &str| None, &[], d.path()).unwrap_err();
            assert_eq!(err.message, format!("{step} failed"));
        }
        std::fs::write(d.path().join("tools/paddock-dev/inputs.txt"), "has space\n").unwrap();
        assert!(
            compute(&ok, &|_: &str| None, &[], d.path())
                .unwrap_err()
                .text()
                .contains("not a plain repository-relative path")
        );
    }

    #[test]
    fn a_missing_input_list_names_the_next_step() {
        let d = tempfile::tempdir().unwrap();
        let fake = Fake {
            rustc: "rustc".into(),
            tree: String::new(),
            status: String::new(),
            stdin_seen: RefCell::default(),
        };
        let err = compute(&fake, &|_: &str| None, &[], d.path()).unwrap_err();
        assert!(err.text().contains("inputs.txt") && err.text().contains("Paddock repository"));
    }
}
