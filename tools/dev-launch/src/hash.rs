//! The input hash: the content address of the `paddock-dev` binary (ADR 0022 §3.3).
//!
//! The hash is `git hash-object --stdin` over `git ls-tree HEAD -- <inputs>` followed by the `commit-hash:` and `release:`
//! lines of `<rustc> -vV` (in rustc's order) and [`flag_lines`] (the hashed environment variables and a digest of each cargo
//! config file), where `<inputs>` are the paths in `tools/paddock-dev/inputs.txt` and `<rustc>` is the compiler cargo will use.
//! This is the one copy of that logic: the launcher (`./dev --wrapper-hash`) and `paddock-dev` (`dev self hash`) both call it.
//! When an input has uncommitted changes (`git status --porcelain --untracked-files=all`), the hash cannot describe the
//! files and the launcher builds.

use std::path::{Path, PathBuf};

/// What went wrong, and what to do about it (empty when there is nothing to do).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashError {
    /// What went wrong.
    pub message: String,
    /// What to do about it.
    pub hint: String,
}

impl HashError {
    /// An error with no hint.
    pub fn new(message: impl Into<String>) -> HashError {
        HashError {
            message: message.into(),
            hint: String::new(),
        }
    }

    /// The same error with a hint.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> HashError {
        self.hint = hint.into();
        self
    }

    /// The message, then the hint on the same paragraph.
    pub fn text(&self) -> String {
        if self.hint.is_empty() {
            self.message.clone()
        } else {
            format!("{} {}", self.message, self.hint)
        }
    }
}

/// Runs a program in a folder with optional standard input and returns standard output: the port that keeps the computation
/// testable without a repository.
pub type RunFn<'a> = &'a dyn Fn(&str, &[&str], &Path, Option<&[u8]>) -> Result<String, HashError>;

/// The input paths of `inputs.txt`: one per line, `#` comments and blank lines skipped, no spaces, no absolute or `..` paths.
pub fn parse_inputs(text: &str) -> Result<Vec<String>, HashError> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let bad = line.contains(char::is_whitespace) || line.starts_with('/') || line.split('/').any(|p| p == "..") || line.contains('\\');
        if bad {
            return Err(
                HashError::new(format!("inputs.txt: {line:?} is not a plain repository-relative path."))
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
/// in byte order, then one `config-file <digest>` line per config (except those [`only_names_build_folder`] skips), the digest
/// being git's blob id of the config with carriage returns dropped. Configs are reduced to digests because they can hold
/// registry tokens, which must not travel in text a trace could print. Trailing newlines are the caller's to trim.
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
fn config_digest(run: RunFn, root: &Path, text: &str) -> Result<String, HashError> {
    let out = run("git", &["hash-object", "--stdin"], root, Some(text.replace('\r', "").as_bytes()))?;
    Ok(out.trim().to_string())
}

/// Whether a config sets nothing but the build folder (`target-dir` in `[build]`, or a top-level dotted `build.target-dir`),
/// besides blank lines and comments: such a file is not hashed, because the build folder never changes the binary (as
/// `CARGO_TARGET_DIR` does not), and CI's build-folder action writes one next to the tool in the middle of a job. Any other
/// table, key or an inline table counts.
pub fn only_names_build_folder(config: &str) -> bool {
    let text = config.replace('\r', "");
    let mut section = String::new();
    let assigns = |line: &str, key: &str| {
        line.strip_prefix(key)
            .is_some_and(|rest| rest.trim_start_matches([' ', '\t']).starts_with('='))
    };
    for line in text.lines().map(|l| l.trim_start_matches([' ', '\t'])) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = line.chars().filter(|c| !matches!(c, ' ' | '\t')).collect();
            if section != "[build]" {
                return false;
            }
            continue;
        }
        let build_folder =
            (section == "[build]" && assigns(line, "target-dir")) || (section.is_empty() && assigns(line, "build.target-dir"));
        if !build_folder {
            return false;
        }
    }
    true
}

/// The `rustc = "..."` of a `[build]` table (or a top-level dotted `build.rustc = `) in a cargo config, read line by line
/// (so an inline table is not seen).
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

/// The compiler a variable or a config names: `RUSTC`, `CARGO_BUILD_RUSTC`, then the first config (closest first) with a
/// `[build] rustc`; `None` when none does (cargo then runs `rustc` from the tool's folder, so a rustup override applies).
pub fn named_rustc(env: &dyn Fn(&str) -> Option<String>, configs: &[String]) -> Option<String> {
    let non_empty = |k: &str| env(k).filter(|v| !v.is_empty());
    non_empty("RUSTC")
        .or_else(|| non_empty("CARGO_BUILD_RUSTC"))
        .or_else(|| configs.iter().find_map(|c| build_rustc(c)))
        .filter(|v| !v.is_empty())
}

/// The compiler program cargo will use: [`named_rustc`], else `rustc`.
pub fn resolve_rustc(env: &dyn Fn(&str) -> Option<String>, configs: &[String]) -> String {
    named_rustc(env, configs).unwrap_or_else(|| "rustc".to_string())
}

/// The config files cargo reads for the tool, closest first: `config.toml` and `config` in every `.cargo` folder from the
/// tool's own up to the filesystem root, the other `*.toml` files of the tool's `.cargo` (what `include` pulls in), then
/// `$CARGO_HOME`'s; only files that exist, without duplicates.
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

/// The computed input hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputHash {
    /// The content address.
    pub hash: String,
    /// The compiler lines that went into it.
    pub compiler: String,
    /// The hashed environment lines and config digests ([`flag_lines`]), with its trailing newlines.
    pub flags: String,
    /// An input has uncommitted changes: the launcher builds instead of using the hash.
    pub dirty: bool,
    /// The paths hashed.
    pub inputs: Vec<String>,
}

/// Computes the hash for the repository at `root`.
pub fn compute(run: RunFn, env: &dyn Fn(&str) -> Option<String>, vars: &[(String, String)], root: &Path) -> Result<InputHash, HashError> {
    compute_for(run, env, vars, root, "tools/paddock-dev")
}

/// [`compute`] for another tool of the repository (`tools/heavy`): the same hash over the `inputs.txt` in its folder `tool_dir`
/// (relative to `root`), so a tool built on demand is cached under its own content address.
pub fn compute_for(
    run: RunFn,
    env: &dyn Fn(&str) -> Option<String>,
    vars: &[(String, String)],
    root: &Path,
    tool_dir: &str,
) -> Result<InputHash, HashError> {
    let tool = root.join(tool_dir);
    let list = std::fs::read_to_string(tool.join("inputs.txt")).map_err(|e| {
        HashError::new(format!("{tool_dir}/inputs.txt could not be read: {e}.")).with_hint("Run ./dev inside the Paddock repository.")
    })?;
    let inputs = parse_inputs(&list)?;
    let configs: Vec<String> = config_files(root, env)
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect();
    let rustc = resolve_rustc(env, &configs);
    let compiler = compiler_lines(&run(&rustc, &["-vV"], &tool, None)?);
    hash_inputs(run, root, inputs, &compiler, &configs, vars)
}

/// The hash of `inputs` (paths relative to `root`) with the compiler lines already known and the config files' texts:
/// the part of [`compute_for`] after the compiler was asked, which the launcher calls with the `-vV` it ran itself.
pub fn hash_inputs(
    run: RunFn,
    root: &Path,
    inputs: Vec<String>,
    compiler: &str,
    configs: &[String],
    vars: &[(String, String)],
) -> Result<InputHash, HashError> {
    let mut git_args: Vec<&str> = vec!["ls-tree", "HEAD", "--"];
    git_args.extend(inputs.iter().map(String::as_str));
    let tree = run("git", &git_args, root, None)?;
    let digests = configs
        .iter()
        .filter(|c| !only_names_build_folder(c))
        .map(|c| config_digest(run, root, c))
        .collect::<Result<Vec<_>, _>>()?;
    let flags = flag_lines(vars, &digests);
    let hash = run(
        "git",
        &["hash-object", "--stdin"],
        root,
        Some(format!("{tree}{compiler}{}\n", flags.trim_end_matches('\n')).as_bytes()),
    )?
    .trim()
    .to_string();
    let mut status_args: Vec<&str> = vec!["status", "--porcelain", "--untracked-files=all", "--"];
    status_args.extend(inputs.iter().map(String::as_str));
    let dirty = !run("git", &status_args, root, None)?.trim().is_empty();
    Ok(InputHash {
        hash,
        compiler: compiler.to_string(),
        flags,
        dirty,
        inputs,
    })
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A scratch folder that goes with the value.
    struct Scratch(PathBuf);
    impl Scratch {
        #[expect(
            clippy::disallowed_methods,
            reason = "this crate has no scratch helper (it is std only); the folder is removed on drop"
        )]
        fn new() -> Scratch {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir().join(format!(
                "dev-launch-hash-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&p).unwrap();
            Scratch(p)
        }
        fn write(&self, rel: &str, text: &str) {
            let f = self.0.join(rel);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, text).unwrap();
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

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
    fn only_a_build_folder_setting_is_skipped() {
        assert!(only_names_build_folder("# c\n[build]\ntarget-dir = \"/x\"\n\n"));
        assert!(only_names_build_folder("build.target-dir = \"/x\"\r\n"));
        assert!(!only_names_build_folder("[build]\ntarget-dir = \"/x\"\njobs = 2\n"));
        assert!(!only_names_build_folder("[net]\ngit-fetch-with-cli = true\n"));
        assert!(!only_names_build_folder("include = [{ path = \"ci.toml\" }]\n"));
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
        assert_eq!(
            named_rustc(&env(&[]), &[]),
            None,
            "no compiler named: cargo runs rustc from the tool's folder"
        );
    }

    #[test]
    fn config_files_go_closest_first_include_the_tools_toml_files_and_end_with_cargo_home() {
        let d = Scratch::new();
        let (root, home) = (d.0.join("repo"), d.0.join("ch"));
        d.write("repo/tools/paddock-dev/.cargo/config.toml", "a");
        d.write("repo/tools/paddock-dev/.cargo/ci.toml", "b");
        d.write("repo/tools/paddock-dev/.cargo/note.txt", "c");
        d.write("repo/.cargo/config", "d");
        d.write(".cargo/config.toml", "above the repository");
        d.write("ch/config.toml", "e");
        let env = |k: &str| (k == "CARGO_HOME").then(|| home.display().to_string());
        let names: Vec<String> = config_files(&root, &env)
            .iter()
            .map(|p| {
                p.strip_prefix(&d.0)
                    .unwrap()
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            })
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
        let same = |k: &str| (k == "CARGO_HOME").then(|| d.0.join(".cargo").display().to_string());
        assert_eq!(
            config_files(&root, &same)
                .iter()
                .filter(|p| p.ends_with(".cargo/config.toml"))
                .count(),
            2
        );
        let via_home = |k: &str| (k == "HOME").then(|| d.0.display().to_string());
        assert_eq!(config_files(&root, &via_home).len(), 4);
    }

    /// Scripted programs: answers by program and first argument, and records the stdin of `hash-object`.
    struct Fake {
        rustc: String,
        tree: String,
        status: String,
        stdin_seen: RefCell<Vec<String>>,
    }

    impl Fake {
        fn run(&self, program: &str, args: &[&str], stdin: Option<&[u8]>) -> Result<String, HashError> {
            match (program, args.first().copied()) {
                ("git", Some("ls-tree")) => Ok(self.tree.clone()),
                ("git", Some("status")) => Ok(self.status.clone()),
                ("git", Some("hash-object")) => {
                    let text = String::from_utf8_lossy(stdin.unwrap_or_default()).into_owned();
                    self.stdin_seen.borrow_mut().push(text.clone());
                    Ok(format!("hash-of-{}\n", text.len()))
                }
                (p, Some("-vV")) if p == self.rustc => Ok("commit-hash: c1\nhost: h\nrelease: 1.0\n".to_string()),
                _ => Err(HashError::new("unexpected program")),
            }
        }
    }

    fn repo() -> Scratch {
        let d = Scratch::new();
        d.write("tools/paddock-dev/inputs.txt", "# x\ntools/paddock-dev\ncrates/a\n");
        d
    }

    fn fake(rustc: &str, tree: &str, status: &str) -> Fake {
        Fake {
            rustc: rustc.into(),
            tree: tree.into(),
            status: status.into(),
            stdin_seen: RefCell::default(),
        }
    }

    #[test]
    fn compute_hashes_the_tree_and_the_compiler_lines_and_reports_dirt() {
        let d = repo();
        let f = fake("rustc", "T\n", " M tools/paddock-dev/src/x.rs\n");
        let run = |p: &str, a: &[&str], _: &Path, s: Option<&[u8]>| f.run(p, a, s);
        let h = compute(&run, &|_: &str| None, &[], &d.0).unwrap();
        assert_eq!(f.stdin_seen.borrow()[0], "T\ncommit-hash: c1\nrelease: 1.0\n\n");
        assert!(h.dirty && h.inputs == ["tools/paddock-dev", "crates/a"] && h.hash.starts_with("hash-of-"));
        let clean = fake("rustc", "T\n", "\n");
        let run = |p: &str, a: &[&str], _: &Path, s: Option<&[u8]>| clean.run(p, a, s);
        assert!(!compute(&run, &|_: &str| None, &[], &d.0).unwrap().dirty);
    }

    #[test]
    fn compute_asks_the_compiler_named_by_the_environment() {
        let d = repo();
        let f = fake("my-rustc", "", "");
        let run = |p: &str, a: &[&str], _: &Path, s: Option<&[u8]>| f.run(p, a, s);
        let env = |k: &str| (k == "RUSTC").then(|| "my-rustc".to_string());
        assert!(compute(&run, &env, &[], &d.0).is_ok());
        assert!(compute(&run, &|_: &str| None, &[], &d.0).is_err());
    }

    #[test]
    fn every_step_that_fails_stops_the_hash_with_that_error() {
        let d = repo();
        let f = fake("rustc", "T\n", "");
        let ok = |p: &str, a: &[&str], _: &Path, s: Option<&[u8]>| f.run(p, a, s);
        assert!(compute(&ok, &|_: &str| None, &[], &d.0).is_ok());
        for step in ["rustc", "ls-tree", "hash-object", "status"] {
            let fail = |_: &str, args: &[&str], _: &Path, _: Option<&[u8]>| {
                let name = if args.first() == Some(&"-vV") { "rustc" } else { args[0] };
                if name == step {
                    Err(HashError::new(format!("{step} failed")))
                } else {
                    Ok("x\n".to_string())
                }
            };
            let err = compute(&fail, &|_: &str| None, &[], &d.0).unwrap_err();
            assert_eq!(err.message, format!("{step} failed"));
        }
        d.write("tools/paddock-dev/inputs.txt", "has space\n");
        assert!(
            compute(&ok, &|_: &str| None, &[], &d.0)
                .unwrap_err()
                .text()
                .contains("not a plain repository-relative path")
        );
    }

    #[test]
    fn a_missing_input_list_names_the_next_step() {
        let d = Scratch::new();
        let f = fake("rustc", "", "");
        let run = |p: &str, a: &[&str], _: &Path, s: Option<&[u8]>| f.run(p, a, s);
        let err = compute(&run, &|_: &str| None, &[], &d.0).unwrap_err();
        assert!(err.text().contains("inputs.txt") && err.text().contains("Paddock repository"));
    }
}
