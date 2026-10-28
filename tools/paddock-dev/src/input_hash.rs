//! `dev self hash`: the content address of the tool's binary (ADR 0022 §3.3).
//!
//! The hash is `git hash-object --stdin` over `git ls-tree HEAD -- <inputs>` followed by the `commit-hash:` and `release:`
//! lines of `<rustc> -vV` (in rustc's order), where `<inputs>` are the paths in `tools/paddock-dev/inputs.txt` and `<rustc>`
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

/// The `rustc = "..."` of a `[build]` table in a cargo config, read line by line exactly as the wrapper's awk does (so a
/// dotted `build.rustc = ` key or an inline table is not seen; the wrapper has the same limit).
pub fn build_rustc(config: &str) -> Option<String> {
    let mut section = String::new();
    for line in config.lines() {
        let trimmed = line.trim_start_matches([' ', '\t']);
        if trimmed.starts_with('[') {
            section = line.chars().filter(|c| !matches!(c, ' ' | '\t')).collect();
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("rustc") else { continue };
        let Some(value) = rest.trim_start_matches([' ', '\t']).strip_prefix('=') else {
            continue;
        };
        if section != "[build]" {
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

/// The configs on the tool's path, closest first: the tool's own folder, `tools/`, the root and `$CARGO_HOME`.
pub fn config_paths(root: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let cargo_home = env("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").or_else(|| env("USERPROFILE")).map(|h| PathBuf::from(h).join(".cargo")));
    [
        root.join("tools/paddock-dev/.cargo"),
        root.join("tools/.cargo"),
        root.join(".cargo"),
    ]
    .into_iter()
    .chain(cargo_home)
    .map(|d| d.join("config.toml"))
    .collect()
}

/// Runs programs for the hash: a port, so the computation is testable without a repository.
pub trait Exec {
    /// Runs `program args...` in `cwd` with `stdin` and returns standard output, or a message when it failed.
    fn run(&self, program: &str, args: &[&str], cwd: &Path, stdin: Option<&[u8]>) -> Result<String, CliError>;
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
                let mut child = cmd.spawn().map_err(|e| fail(e.to_string()))?;
                if let Some(mut pipe) = child.stdin.take() {
                    pipe.write_all(bytes).map_err(|e| fail(e.to_string()))?;
                }
                let out = child.wait_with_output().map_err(|e| fail(e.to_string()))?;
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
pub fn compute(exec: &dyn Exec, env: &dyn Fn(&str) -> Option<String>, root: &Path) -> Result<InputHash, CliError> {
    let tool = root.join("tools/paddock-dev");
    let list = std::fs::read_to_string(tool.join("inputs.txt")).map_err(|e| {
        CliError::failure(format!("tools/paddock-dev/inputs.txt could not be read: {e}."))
            .with_hint("Run ./dev inside the Paddock repository.")
    })?;
    let inputs = parse_inputs(&list)?;
    let configs: Vec<String> = config_paths(root, env)
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect();
    let rustc = resolve_rustc(env, &configs);
    let compiler = compiler_lines(&exec.run(&rustc, &["-vV"], &tool, None)?);
    let mut git_args: Vec<&str> = vec!["ls-tree", "HEAD", "--"];
    git_args.extend(inputs.iter().map(String::as_str));
    let tree = exec.run("git", &git_args, root, None)?;
    let hash = exec
        .run(
            "git",
            &["hash-object", "--stdin"],
            root,
            Some(format!("{tree}{compiler}").as_bytes()),
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
    let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
    let env = |k: &str| std::env::var(k).ok();
    let root = Real
        .run("git", &["rev-parse", "--show-toplevel"], &cwd, None)
        .map_err(|e| e.with_hint("Run ./dev inside the Paddock repository."))?;
    let h = compute(&Real, &env, Path::new(root.trim()))?;
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
        assert_eq!(build_rustc("[build]\nrustc = bare\n").as_deref(), Some("bare"));
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
    fn config_paths_go_closest_first_and_cargo_home_wins_over_home() {
        let env = |k: &str| match k {
            "CARGO_HOME" => Some("/ch".to_string()),
            "HOME" => Some("/h".to_string()),
            _ => None,
        };
        let p = config_paths(Path::new("/r"), &env);
        assert!(p[0].ends_with("tools/paddock-dev/.cargo/config.toml") && p[2].ends_with(".cargo/config.toml"));
        assert_eq!(p[3], Path::new("/ch/config.toml"));
        let home_only = |k: &str| (k == "HOME").then(|| "/h".to_string());
        assert_eq!(config_paths(Path::new("/r"), &home_only)[3], Path::new("/h/.cargo/config.toml"));
        assert_eq!(config_paths(Path::new("/r"), &|_: &str| None).len(), 3);
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
        let h = compute(&fake, &|_: &str| None, d.path()).unwrap();
        assert_eq!(fake.stdin_seen.borrow()[0], "T\ncommit-hash: c1\nrelease: 1.0\n");
        assert!(h.dirty && h.inputs == ["tools/paddock-dev", "crates/a"] && h.hash.starts_with("hash-of-"));
        let clean = Fake {
            status: "\n".into(),
            ..fake
        };
        assert!(!compute(&clean, &|_: &str| None, d.path()).unwrap().dirty);
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
        assert!(compute(&fake, &env, d.path()).is_ok());
        assert!(compute(&fake, &|_: &str| None, d.path()).is_err());
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

    #[test]
    fn a_missing_input_list_names_the_next_step() {
        let d = tempfile::tempdir().unwrap();
        let fake = Fake {
            rustc: "rustc".into(),
            tree: String::new(),
            status: String::new(),
            stdin_seen: RefCell::default(),
        };
        let err = compute(&fake, &|_: &str| None, d.path()).unwrap_err();
        assert!(err.text().contains("inputs.txt") && err.text().contains("Paddock repository"));
    }
}
