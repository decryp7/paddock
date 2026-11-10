//! `dev hooks install`: put the guard binary and its shim where Claude Code's hook finds them (ADR 0083 §2, plan RT-4).
//!
//! The hook must not run through the `./dev` wrapper (150 ms or more per call, a cargo build on a cache miss). So the binary is
//! copied to `<git-common-dir>/paddock/hooks/paddock-dev`, one copy for every worktree of the repository, by an atomic rename
//! so a running hook never sees half a file. A stamp beside it says which sources it was built from.
//!
//! A binary is installed only when it was built from `origin/main` (its embedded input hash, `dev self hash`, equals the hash
//! of the inputs at that revision), so a branch's half-built rules never guard everyone; `--local` installs any binary and the
//! stamp says so. The shim `tools/githooks/claude-pre-tool-use` is POSIX `sh` (bootstrap exception: it must run before any
//! binary exists). It is a tracked file and `install` never writes it: this module holds the same text (a test compares them)
//! and install warns when the checkout's copy is missing or differs. Install does not touch git's configuration either:
//! `core.hooksPath` would silently switch off `.git/hooks/commit-msg` (the ER-5 trailer and subject-length guard), so setting it
//! is left to the tracked-hooks change of ADR 0038.

use crate::input_hash::{self, EMBEDDED, Exec, Real};
use clap::Args;
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::path::{Path, PathBuf};

/// The text of the shim `tools/githooks/claude-pre-tool-use`.
pub const SHIM: &str = include_str!("claude-pre-tool-use.sh");
/// The shim's place in the repository (a tracked file).
pub const SHIM_PATH: &str = "tools/githooks/claude-pre-tool-use";
/// The folder of tracked hook scripts.
pub const HOOKS_DIR: &str = "tools/githooks";
/// The installed binary's file name.
pub const BIN_NAME: &str = if cfg!(windows) { "paddock-dev.exe" } else { "paddock-dev" };
/// The stamp's file name.
pub const STAMP_NAME: &str = "paddock-dev.stamp.json";

/// Arguments of `hooks install`.
#[derive(Args)]
pub struct InstallArgs {
    /// Install this binary although it was not built from the base revision (a branch build, to try a rule). The stamp says so.
    #[arg(long)]
    local: bool,
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// The revision a binary must be built from.
    #[arg(long, default_value = "origin/main")]
    base: String,
}

/// Where the binary being installed comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// `--local`: not checked.
    Local,
    /// Built from the base revision: its embedded input hash equals the hash at that revision.
    Base {
        /// The revision named (`origin/main`).
        base: String,
        /// The commit it named.
        commit: String,
        /// The input hash.
        hash: String,
    },
}

/// `dev hooks install`.
pub fn command(a: &InstallArgs) -> Result<Report, CliError> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let root = match &a.root {
        Some(r) => r.clone(),
        // A bare repository has no top level: its own folder is the root then (the shim and the binary live in it).
        None => match Real.run("git", &["rev-parse", "--show-toplevel"], &cwd, None) {
            Ok(top) => PathBuf::from(top.trim()),
            Err(_)
                if Real
                    .run("git", &["rev-parse", "--is-bare-repository"], &cwd, None)
                    .is_ok_and(|b| b.trim() == "true") =>
            {
                cwd.clone()
            }
            Err(e) => return Err(e.with_hint("Run ./dev hooks install inside the Paddock repository.")),
        },
    };
    let exe = std::env::current_exe()
        .map_err(|e| CliError::failure(format!("the path of this binary is unknown: {e}.")).with_hint("Run it as ./dev hooks install."))?;
    let origin = if a.local {
        Origin::Local
    } else {
        check_origin(&Real, &root, &a.base)?
    };
    install(&Real, &root, &exe, &origin)
}

/// Refuses a binary that does not say it was built from `base`'s sources, or says it was built from others.
pub fn verify(embedded: Option<&str>, expected: &str, base: &str) -> Result<(), CliError> {
    let hint = format!("Build from {base} (switch to it, run ./dev hooks install) or pass --local to install this binary anyway.");
    match embedded {
        None => {
            Err(CliError::refused("this binary does not say which sources it was built from (no embedded input hash).").with_hint(hint))
        }
        Some(h) if h != expected => Err(CliError::refused(format!(
            "this binary was built from other sources than {base}: its input hash is {h}, {base}'s is {expected}."
        ))
        .with_hint(hint)),
        Some(_) => Ok(()),
    }
}

/// Runs the check for the real binary: [`EMBEDDED`] against the hash at `base`.
fn check_origin(exec: &dyn Exec, root: &Path, base: &str) -> Result<Origin, CliError> {
    let commit = exec
        .run(
            "git",
            &["rev-parse", "--verify", "--quiet", &format!("{base}^{{commit}}")],
            root,
            None,
        )
        .map_err(|_| {
            CliError::failure(format!("{base} is not a revision in this repository."))
                .with_hint("Run git fetch origin, or pass --local to install this binary without the check.")
        })?
        .trim()
        .to_string();
    let hash = expected_hash(exec, root, base)?;
    verify(EMBEDDED, &hash, base)?;
    Ok(Origin::Base {
        base: base.to_string(),
        commit,
        hash,
    })
}

/// Runs git as the input hash does, but reads the inputs from `rev` instead of `HEAD`.
struct AtRev<'a> {
    exec: &'a dyn Exec,
    rev: &'a str,
}

impl Exec for AtRev<'_> {
    fn run(&self, program: &str, args: &[&str], cwd: &Path, stdin: Option<&[u8]>) -> Result<String, CliError> {
        if program == "git" && args.starts_with(&["ls-tree", "HEAD", "--"]) {
            let mut at: Vec<&str> = args.to_vec();
            at[1] = self.rev;
            return self.exec.run(program, &at, cwd, stdin);
        }
        self.exec.run(program, args, cwd, stdin)
    }
}

/// The input hash of the tool's inputs at `rev` (`inputs.txt`, the compiler and the build flags are the working tree's).
pub fn expected_hash(exec: &dyn Exec, root: &Path, rev: &str) -> Result<String, CliError> {
    let env = |k: &str| std::env::var(k).ok();
    let vars: Vec<(String, String)> = std::env::vars().collect();
    Ok(input_hash::compute(&AtRev { exec, rev }, &env, &vars, root)?.hash)
}

/// What the tracked shim looks like in the checkout `install` ran in.
fn shim_state(root: &Path) -> &'static str {
    match std::fs::read_to_string(root.join(SHIM_PATH)) {
        Err(_) => "missing",
        Ok(t) if t == SHIM => "current",
        Ok(_) => "differs",
    }
}

/// Copies `exe` to the common git folder and writes the stamp beside it. Nothing else is written: not a file of the checkout
/// (the shim is tracked; a missing or different one is a warning) and not git's configuration (`core.hooksPath` would switch
/// off `.git/hooks/commit-msg`, the ER-5 guard; setting it belongs to the tracked-hooks change of ADR 0038).
pub fn install(exec: &dyn Exec, root: &Path, exe: &Path, origin: &Origin) -> Result<Report, CliError> {
    let fail = |what: &str, e: std::io::Error| {
        CliError::failure(format!("{what}: {e}.")).with_hint("Check that the folder is writable, then run ./dev hooks install again.")
    };
    if !exe.is_file() {
        return Err(CliError::failure(format!("{} is not a file.", exe.display())).with_hint("Run it as ./dev hooks install."));
    }
    let common = exec
        .run("git", &["rev-parse", "--path-format=absolute", "--git-common-dir"], root, None)
        .map_err(|e| e.with_hint("Run ./dev hooks install inside a git repository."))?;
    let common = PathBuf::from(common.trim());
    if !common.is_dir() {
        return Err(
            CliError::failure(format!("git's common folder {} does not exist.", common.display()))
                .with_hint("Run ./dev hooks install inside a git repository."),
        );
    }

    let dir = common.join("paddock").join("hooks");
    std::fs::create_dir_all(&dir).map_err(|e| fail(&format!("{} could not be made", dir.display()), e))?;
    let dest = dir.join(BIN_NAME);
    let size = place(exe, &dest, true).map_err(|e| fail(&format!("{} could not be written", dest.display()), e))?;
    let (base, commit, hash) = match origin {
        Origin::Base { base, commit, hash } => (json!(base), json!(commit), json!(hash)),
        Origin::Local => (json!(null), json!(null), json!(EMBEDDED)),
    };
    let stamp = json!({
        "source": exe.display().to_string(),
        "bytes": size,
        "local": matches!(origin, Origin::Local),
        "base": base,
        "commit": commit,
        "hash": hash,
        "installed_unix": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
    });
    let stamp_path = dir.join(STAMP_NAME);
    let mut text = serde_json::to_string_pretty(&stamp).unwrap_or_default();
    text.push('\n');
    place_bytes(text.as_bytes(), &stamp_path, false).map_err(|e| fail(&format!("{} could not be written", stamp_path.display()), e))?;

    let state = shim_state(root);
    let shim_line = match state {
        "current" => format!("shim      {SHIM_PATH} is current"),
        "differs" => format!(
            "shim      warning: {SHIM_PATH} differs from this binary's shim. The tracked file wins: update it through a change (the text is in src/hook/claude-pre-tool-use.sh)."
        ),
        _ => format!(
            "shim      warning: {SHIM_PATH} is missing in this checkout (a sparse checkout without tools? git sparse-checkout add tools). Claude Code's hook needs it."
        ),
    };
    let kind = match origin {
        Origin::Local => "local build".to_string(),
        Origin::Base { base, commit, .. } => format!("built from {base} ({})", commit.get(..9).unwrap_or(commit)),
    };
    let text = format!("binary    {} ({size} bytes, {kind})\n{shim_line}", dest.display());
    Ok(Report::new(
        json!({
            "binary": dest.display().to_string(),
            "bytes": size,
            "stamp": stamp_path.display().to_string(),
            "local": matches!(origin, Origin::Local),
            "shim_state": state,
        }),
        text,
    ))
}

/// Copies `from` to `to` through a file next to it and a rename; returns the size.
fn place(from: &Path, to: &Path, executable: bool) -> std::io::Result<u64> {
    let tmp = tmp_name(to);
    let result = std::fs::copy(from, &tmp).and_then(|n| {
        if executable {
            set_executable(&tmp)?;
        }
        std::fs::rename(&tmp, to).map(|()| n)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn place_bytes(bytes: &[u8], to: &Path, executable: bool) -> std::io::Result<()> {
    let tmp = tmp_name(to);
    let result = std::fs::write(&tmp, bytes).and_then(|()| {
        if executable {
            set_executable(&tmp)?;
        }
        std::fs::rename(&tmp, to)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn tmp_name(to: &Path) -> PathBuf {
    let name = to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    to.with_file_name(format!("{name}.new-{}", std::process::id()))
}

#[cfg(unix)]
fn set_executable(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn set_executable(_: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_cli::Exit;

    /// Runs programs with a home, git configuration and Claude variables of their own: a test never reads or changes the user's.
    struct Isolated {
        home: PathBuf,
    }

    impl Exec for Isolated {
        fn run(&self, program: &str, args: &[&str], cwd: &Path, stdin: Option<&[u8]>) -> Result<String, CliError> {
            use std::io::Write;
            use std::process::Stdio;
            let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
            let mut cmd = paddock_proc::command(program);
            cmd.args(args)
                .current_dir(cwd)
                .env("HOME", &self.home)
                .env("USERPROFILE", &self.home)
                .env("GIT_CONFIG_GLOBAL", null)
                .env("GIT_CONFIG_SYSTEM", null)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.com")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.com")
                .env_remove("CLAUDE_PROJECT_DIR")
                .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = cmd.spawn().map_err(|e| CliError::failure(format!("{program}: {e}")))?;
            if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
                let _ = pipe.write_all(bytes);
            }
            let out = child.wait_with_output().map_err(|e| CliError::failure(format!("{program}: {e}")))?;
            if !out.status.success() {
                return Err(CliError::failure(format!(
                    "{program} {}: {}",
                    args.first().unwrap_or(&""),
                    String::from_utf8_lossy(&out.stderr).trim()
                )));
            }
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        exec: Isolated,
        root: PathBuf,
        exe: PathBuf,
    }

    /// A throwaway repository with one commit, a fake binary and a home of its own.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let home = base.join("home");
        let root = base.join("repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(root.join("tools/paddock-dev")).unwrap();
        std::fs::write(root.join("tools/paddock-dev/inputs.txt"), "tools/paddock-dev\n").unwrap();
        std::fs::create_dir_all(root.join(HOOKS_DIR)).unwrap();
        std::fs::write(root.join(HOOKS_DIR).join("README"), "x\n").unwrap();
        let exe = base.join("fake-paddock-dev");
        std::fs::write(&exe, "#!/bin/sh\necho fake\n").unwrap();
        let exec = Isolated { home };
        exec.run("git", &["init", "-q", "-b", "main"], &root, None).unwrap();
        exec.run("git", &["add", "-A"], &root, None).unwrap();
        exec.run("git", &["commit", "-q", "-m", "base"], &root, None).unwrap();
        Fixture {
            _dir: dir,
            exec,
            root,
            exe,
        }
    }

    fn git(f: &Fixture, args: &[&str]) -> String {
        f.exec.run("git", args, &f.root, None).unwrap()
    }

    fn common_hooks(f: &Fixture) -> PathBuf {
        let common = git(f, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
        PathBuf::from(common.trim()).join("paddock").join("hooks")
    }

    #[test]
    fn verify_refuses_an_unknown_or_other_build_and_names_the_next_step() {
        assert!(verify(Some("abc"), "abc", "origin/main").is_ok());
        let none = verify(None, "abc", "origin/main").unwrap_err();
        assert!(none.to_string().contains("no embedded input hash"));
        let other = verify(Some("abc"), "def", "origin/main").unwrap_err();
        let text = other.to_string();
        assert!(
            text.contains("abc") && text.contains("def") && text.contains("origin/main"),
            "{text}"
        );
        assert_eq!(other.exit, Exit::Refused);
        assert!(other.hint.contains("--local"));
    }

    #[test]
    fn the_expected_hash_is_the_hash_at_the_base_not_at_head() {
        let f = fixture();
        git(&f, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        let at_base = expected_hash(&f.exec, &f.root, "origin/main").unwrap();
        assert_eq!(at_base, expected_hash(&f.exec, &f.root, "HEAD").unwrap());
        std::fs::write(f.root.join("tools/paddock-dev/new.rs"), "fn main() {}\n").unwrap();
        git(&f, &["add", "-A"]);
        git(&f, &["commit", "-q", "-m", "change an input"]);
        assert_eq!(
            expected_hash(&f.exec, &f.root, "origin/main").unwrap(),
            at_base,
            "the base did not move"
        );
        assert_ne!(expected_hash(&f.exec, &f.root, "HEAD").unwrap(), at_base, "head's inputs changed");
    }

    #[test]
    fn a_missing_base_is_refused_with_the_fetch_hint() {
        let f = fixture();
        let e = check_origin(&f.exec, &f.root, "origin/main").unwrap_err();
        assert!(e.to_string().contains("not a revision") && e.hint.contains("git fetch"), "{e}");
    }

    #[test]
    fn a_binary_without_an_embedded_hash_is_refused_unless_local() {
        // the test binary is not built through ./dev, so nothing is embedded
        if EMBEDDED.is_none() {
            let f = fixture();
            git(&f, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
            assert!(check_origin(&f.exec, &f.root, "origin/main").is_err());
        }
    }

    #[test]
    fn install_copies_the_binary_and_a_stamp_and_changes_nothing_else() {
        let f = fixture();
        let home_before = std::fs::read_dir(&f.exec.home).unwrap().count();
        let tracked_before = git(&f, &["status", "--porcelain", "--untracked-files=all"]);
        let report = install(&f.exec, &f.root, &f.exe, &Origin::Local).unwrap();
        let dir = common_hooks(&f);
        let bin = dir.join(BIN_NAME);
        assert_eq!(std::fs::read(&bin).unwrap(), std::fs::read(&f.exe).unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&bin).unwrap().permissions().mode() & 0o777, 0o755);
        }
        let stamp: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join(STAMP_NAME)).unwrap()).unwrap();
        assert_eq!(stamp["local"], true);
        assert_eq!(stamp["base"], serde_json::Value::Null);
        // P1: no core.hooksPath (it would switch off .git/hooks), and no file of the checkout is written
        assert!(
            f.exec
                .run("git", &["config", "--local", "--get", "core.hooksPath"], &f.root, None)
                .is_err()
        );
        assert_eq!(git(&f, &["status", "--porcelain", "--untracked-files=all"]), tracked_before);
        assert!(!report.text.contains("hooksPath") && report.data.get("hooks_path_set").is_none());
        assert_eq!(
            std::fs::read_dir(&f.exec.home).unwrap().count(),
            home_before,
            "the home was not touched"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(leftovers.len(), 2, "binary and stamp only: {leftovers:?}");
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_commit_msg_hook_still_runs_after_install() {
        use std::os::unix::fs::PermissionsExt;
        let f = fixture();
        let hook = f.root.join(".git/hooks/commit-msg");
        std::fs::write(&hook, "#!/bin/sh\necho ran > \"$(dirname \"$0\")/marker\"\nexit 0\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        install(&f.exec, &f.root, &f.exe, &Origin::Local).unwrap();
        std::fs::write(f.root.join("x.txt"), "x\n").unwrap();
        git(&f, &["add", "-A"]);
        git(&f, &["commit", "-q", "-m", "after install"]);
        assert_eq!(
            std::fs::read_to_string(f.root.join(".git/hooks/marker")).unwrap(),
            "ran\n",
            "commit-msg did not run"
        );
    }

    #[test]
    fn a_base_origin_is_stamped_with_its_commit_and_hash() {
        let f = fixture();
        let origin = Origin::Base {
            base: "origin/main".into(),
            commit: "0123456789abcdef".into(),
            hash: "h1".into(),
        };
        let report = install(&f.exec, &f.root, &f.exe, &origin).unwrap();
        assert!(report.text.contains("built from origin/main (012345678)"), "{}", report.text);
        let stamp_text = std::fs::read_to_string(report.data["stamp"].as_str().unwrap()).unwrap();
        let stamp: serde_json::Value = serde_json::from_str(&stamp_text).unwrap();
        assert_eq!(stamp["local"], false);
        assert_eq!(stamp["hash"], "h1");
        assert_eq!(stamp["commit"], "0123456789abcdef");
    }

    #[test]
    fn a_linked_worktree_installs_into_the_shared_common_folder() {
        let f = fixture();
        let wt = f.root.parent().unwrap().join("wt");
        git(&f, &["worktree", "add", "-q", "-b", "other", wt.to_str().unwrap()]);
        install(&f.exec, &wt, &f.exe, &Origin::Local).unwrap();
        assert!(f.root.join(".git/paddock/hooks").join(BIN_NAME).is_file());
        assert!(!wt.join(".git").is_dir(), "a linked worktree's .git is a file");
    }

    #[test]
    fn a_bare_repository_and_its_worktree_share_one_install() {
        let f = fixture();
        let bare = f.root.parent().unwrap().join("bare.git");
        f.exec
            .run(
                "git",
                &["clone", "-q", "--bare", f.root.to_str().unwrap(), bare.to_str().unwrap()],
                &f.root,
                None,
            )
            .unwrap();
        let wt = f.root.parent().unwrap().join("bare-wt");
        f.exec
            .run("git", &["worktree", "add", "-q", wt.to_str().unwrap(), "main"], &bare, None)
            .unwrap();
        install(&f.exec, &wt, &f.exe, &Origin::Local).unwrap();
        assert!(
            bare.join("paddock/hooks").join(BIN_NAME).is_file(),
            "the binary is in the bare repository's folder"
        );
        // installing from inside the bare repository itself works too (no worktree to find)
        let again = install(&f.exec, &bare, &f.exe, &Origin::Local).unwrap();
        assert!(again.data["binary"].as_str().unwrap().contains("bare.git"));
    }

    #[test]
    fn the_tracked_shim_is_checked_never_written() {
        let f = fixture();
        let shim = f.root.join(SHIM_PATH);
        // missing: a warning, no file
        let missing = install(&f.exec, &f.root, &f.exe, &Origin::Local).unwrap();
        assert_eq!(missing.data["shim_state"], "missing");
        assert!(missing.text.contains("warning") && !shim.exists(), "{}", missing.text);
        // different: a warning, the file is untouched
        std::fs::write(&shim, "#!/bin/sh\nexit 0\n").unwrap();
        let differs = install(&f.exec, &f.root, &f.exe, &Origin::Local).unwrap();
        assert_eq!(differs.data["shim_state"], "differs");
        assert!(differs.text.contains("warning"), "{}", differs.text);
        assert_eq!(std::fs::read_to_string(&shim).unwrap(), "#!/bin/sh\nexit 0\n");
        // same: quiet
        std::fs::write(&shim, SHIM).unwrap();
        let same = install(&f.exec, &f.root, &f.exe, &Origin::Local).unwrap();
        assert_eq!(same.data["shim_state"], "current");
        assert!(!same.text.contains("warning"));
    }

    #[test]
    fn a_second_run_replaces_the_binary_atomically_and_leaves_no_temporary_file() {
        let f = fixture();
        install(&f.exec, &f.root, &f.exe, &Origin::Local).unwrap();
        std::fs::write(&f.exe, "#!/bin/sh\necho second\n").unwrap();
        let second = install(&f.exec, &f.root, &f.exe, &Origin::Local).unwrap();
        let installed = std::fs::read_to_string(second.data["binary"].as_str().unwrap()).unwrap();
        assert_eq!(installed, "#!/bin/sh\necho second\n");
        assert_eq!(std::fs::read_dir(common_hooks(&f)).unwrap().count(), 2);
    }

    #[test]
    fn a_missing_binary_or_a_folder_that_is_no_repository_is_refused_with_a_next_step() {
        let f = fixture();
        let e = install(&f.exec, &f.root, &f.root.join("nope"), &Origin::Local).unwrap_err();
        assert!(e.to_string().contains("not a file"));
        let outside = f.exe.parent().unwrap().join("not-a-repo");
        std::fs::create_dir_all(&outside).unwrap();
        // the temp folder's parents hold no repository
        let e = install(&f.exec, &outside, &f.exe, &Origin::Local).unwrap_err();
        assert!(e.hint.contains("git repository"), "{e}");
    }

    #[test]
    fn the_tracked_shim_is_the_text_install_compares_with_and_starts_no_program() {
        let tracked = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../githooks/claude-pre-tool-use")).unwrap();
        assert_eq!(tracked, SHIM);
        assert!(SHIM.lines().count() <= 10, "a shim is a few lines");
        assert!(SHIM.starts_with("#!/bin/sh\n"));
        // shell builtins only: no git (about 10 ms a call), no sed, no cat
        for program in ["bash", "jq", "python", "git ", "sed ", "cat", "dirname"] {
            assert!(!SHIM.contains(program), "{program}");
        }
        assert!(SHIM.contains("--local"), "the bare-repository way out is in the deny text");
    }

    /// Runs the shim with an empty PATH, so any program it starts fails: only shell builtins and the final `exec` work.
    #[cfg(unix)]
    fn shim_without_path(f: &Fixture, shim: &Path, cwd: &Path, project_dir: Option<&str>, input: &str) -> String {
        let mut args = vec!["PATH=/nonexistent".to_string()];
        if let Some(d) = project_dir {
            args.push(format!("CLAUDE_PROJECT_DIR={d}"));
        }
        args.extend(["/bin/sh".to_string(), shim.to_string_lossy().into_owned()]);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        f.exec.run("env", &args, cwd, Some(input.as_bytes())).unwrap()
    }

    #[cfg(unix)]
    const BUILTIN_BINARY: &str = "#!/bin/sh\nIFS= read -r l\nprintf 'args: %s <%s>\\n' \"$*\" \"$l\"\n";

    #[cfg(unix)]
    const BASH_LS: &str = "{\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"ls\"}}\n";

    #[cfg(unix)]
    #[test]
    fn the_shim_denies_everything_but_the_install_command_while_no_binary_is_installed() {
        let f = fixture();
        let shim = f.root.join(SHIM_PATH);
        std::fs::write(&shim, SHIM).unwrap();
        let call = |cmd: &str| format!("{{\"tool_name\":\"Bash\",\"tool_input\":{{\"command\":\"{cmd}\"}}}}\n");
        for denied in [
            "ls",
            "./dev hooks install && rm x",
            "echo ./dev hooks install",
            "./dev hooks install --other",
        ] {
            let out = shim_without_path(&f, &shim, &f.root, None, &call(denied));
            let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("{denied}: {e}: {out}"));
            assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny", "{denied}");
            assert!(
                v["hookSpecificOutput"]["permissionDecisionReason"]
                    .as_str()
                    .unwrap()
                    .contains("./dev hooks install")
            );
        }
        for allowed in ["./dev hooks install", "./dev hooks install --local"] {
            assert_eq!(shim_without_path(&f, &shim, &f.root, None, &call(allowed)), "", "{allowed}");
        }
        let edit = shim_without_path(
            &f,
            &shim,
            &f.root,
            None,
            "{\"tool_name\":\"Edit\",\"tool_input\":{\"file_path\":\"/x\"}}\n",
        );
        assert!(edit.contains("deny"), "an edit is denied too");
    }

    #[cfg(unix)]
    #[test]
    fn the_shim_finds_the_installed_binary_from_every_checkout_layout_without_starting_any_program() {
        let f = fixture();
        let shim = f.root.join(SHIM_PATH);
        std::fs::write(&shim, SHIM).unwrap();
        std::fs::write(&f.exe, BUILTIN_BINARY).unwrap();
        install(&f.exec, &f.root, &f.exe, &Origin::Local).unwrap();
        let wt = f.root.parent().unwrap().join("wt");
        git(&f, &["worktree", "add", "-q", "-b", "other", wt.to_str().unwrap()]);
        let want = "args: hook pre-tool-use <INPUT>\n";
        // the primary, a subfolder of it, the linked worktree (its .git is a file), a subfolder of that
        for cwd in [f.root.clone(), f.root.join("tools/paddock-dev"), wt.clone(), wt.join("tools")] {
            assert_eq!(shim_without_path(&f, &shim, &cwd, None, "INPUT\n"), want, "{}", cwd.display());
        }
        // CLAUDE_PROJECT_DIR wins over the working folder
        let elsewhere = f.exe.parent().unwrap().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        assert_eq!(
            shim_without_path(&f, &shim, &elsewhere, Some(wt.to_str().unwrap()), "INPUT\n"),
            want
        );
        // a relative gitdir line is read relative to the worktree; the common folder comes from its `commondir` file
        let rel = f.root.parent().unwrap().join("rel");
        std::fs::create_dir_all(&rel).unwrap();
        std::fs::write(rel.join(".git"), "gitdir: ../repo/.git/worktrees/wt\n").unwrap();
        assert_eq!(shim_without_path(&f, &shim, &rel, Some(rel.to_str().unwrap()), "INPUT\n"), want);
        // a worktree folder that is not named like its gitdir still resolves (no name parsing)
        let renamed = f.root.parent().unwrap().join("renamed");
        std::fs::create_dir_all(&renamed).unwrap();
        std::fs::write(
            renamed.join(".git"),
            format!("gitdir: {}\n", f.root.join(".git/worktrees/wt").display()),
        )
        .unwrap();
        assert_eq!(shim_without_path(&f, &shim, &renamed, None, "INPUT\n"), want);
    }

    #[cfg(unix)]
    #[test]
    fn the_shim_works_in_a_bare_repository_and_its_worktrees() {
        let f = fixture();
        let shim = f.root.join(SHIM_PATH);
        std::fs::write(&shim, SHIM).unwrap();
        let bare = f.root.parent().unwrap().join("bare.git");
        f.exec
            .run(
                "git",
                &["clone", "-q", "--bare", f.root.to_str().unwrap(), bare.to_str().unwrap()],
                &f.root,
                None,
            )
            .unwrap();
        let wt = f.root.parent().unwrap().join("bare-wt");
        f.exec
            .run("git", &["worktree", "add", "-q", wt.to_str().unwrap(), "main"], &bare, None)
            .unwrap();
        // before the install: denied, with the way out in the text
        let out = shim_without_path(&f, &shim, &bare, None, BASH_LS);
        assert!(out.contains("deny") && out.contains("--local"), "{out}");
        std::fs::write(&f.exe, BUILTIN_BINARY).unwrap();
        install(&f.exec, &wt, &f.exe, &Origin::Local).unwrap();
        for cwd in [&bare, &wt] {
            let out = shim_without_path(&f, &shim, cwd, None, "INPUT\n");
            assert_eq!(out, "args: hook pre-tool-use <INPUT>\n", "{}", cwd.display());
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_shim_also_finds_a_binary_with_the_windows_name() {
        let f = fixture();
        let shim = f.root.join(SHIM_PATH);
        std::fs::write(&shim, SHIM).unwrap();
        let dir = f.root.join(".git/paddock/hooks");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("paddock-dev.exe");
        std::fs::write(&exe, BUILTIN_BINARY).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(
            shim_without_path(&f, &shim, &f.root, None, "INPUT\n"),
            "args: hook pre-tool-use <INPUT>\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_shim_still_denies_without_a_binary_a_repository_or_a_readable_git_file() {
        let f = fixture();
        let shim = f.root.join(SHIM_PATH);
        std::fs::write(&shim, SHIM).unwrap();
        let denied = |cwd: &Path, project: Option<&str>| shim_without_path(&f, &shim, cwd, project, BASH_LS);
        assert!(denied(&f.root, None).contains("deny"), "no binary installed");
        let outside = f.exe.parent().unwrap().join("not-a-repo");
        std::fs::create_dir_all(&outside).unwrap();
        assert!(denied(&outside, None).contains("deny"), "no repository");
        // a relative project dir, an empty one and .git files that name nothing usable
        assert!(denied(&outside, Some("relative/dir")).contains("deny"));
        assert!(denied(&outside, Some("")).contains("deny"));
        std::fs::write(outside.join(".git"), "gitdir: ../modules/x\n").unwrap();
        assert!(denied(&outside, None).contains("deny"), "a submodule's git file");
        std::fs::write(outside.join(".git"), "").unwrap();
        assert!(denied(&outside, None).contains("deny"), "an empty git file");
        let ok = shim_without_path(
            &f,
            &shim,
            &f.root,
            None,
            "{\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"./dev hooks install\"}}\n",
        );
        assert_eq!(ok, "", "the install command still passes");
    }

    #[test]
    fn the_settings_proposal_wires_the_shim_for_the_guarded_tools() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../githooks/claude-settings.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let entries = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        let matcher = regex::Regex::new(&format!("^(?:{})$", entries[0]["matcher"].as_str().unwrap())).unwrap();
        for tool in [
            "Bash",
            "Edit",
            "Write",
            "NotebookEdit",
            "mcp__serena__initial_instructions",
            "mcp__serena__serena_repl",
        ] {
            assert!(matcher.is_match(tool), "{tool}");
        }
        for tool in ["Read", "Grep", "Glob", "Agent", "mcp__other__x", "BashOutput", "WebFetch"] {
            assert!(!matcher.is_match(tool), "{tool}");
        }
        let hook = &entries[0]["hooks"][0];
        assert_eq!(hook["type"], "command");
        assert!(hook["command"].as_str().unwrap().ends_with(SHIM_PATH));
        assert!(hook["command"].as_str().unwrap().contains("$CLAUDE_PROJECT_DIR"));
        assert_eq!(v["hooks"].as_object().unwrap().len(), 1, "only PreToolUse");
    }

    #[test]
    fn the_readme_says_which_settings_file_lands_and_when() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../githooks/README.md")).unwrap();
        for needle in [
            "claude-settings.json",
            ".claude/settings.json",
            "untracked",
            "--local",
            "bare",
            "core.hooksPath",
        ] {
            assert!(text.contains(needle), "the README does not mention {needle}");
        }
    }
}
