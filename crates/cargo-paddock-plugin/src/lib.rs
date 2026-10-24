//! `cargo paddock-plugin`: the plugin author's tool (ADR 0001).
//!
//! ```text
//! cargo paddock-plugin new my-plugin          # a project that depends on paddock-plugin-api alone
//! cargo paddock-plugin test                   # build, start it against a fake host, run the conformance checks
//! cargo paddock-plugin keygen --out me.key    # a publisher key (owner-only file, never overwritten)
//! cargo paddock-plugin pack --key me.key      # dist/<id>-<version>.paddockplugin, deterministic and signed
//! cargo paddock-plugin verify dist/x.paddockplugin --trust <public key>
//! ```
//!
//! The commands follow `paddock-cli`: `--json` writes exactly one object (errors too), data goes to standard output and notes
//! to standard error, and the exit code says what happened (0 ok, 1 failed, 2 wrong command line, 4 refused, 5 findings).
//!
//! Layout: [`args`] parses the command line into a [`args::Command`] and [`template`] makes the project files (both pure);
//! [`new`], [`build`], [`pack`], [`keys`], [`verify`] and [`check`] are the commands; [`cargo`] is the one place a process
//! (`cargo`) is started and `main.rs` is the thin edge that supplies the real working folder and `cargo`.

#![deny(missing_docs)]

pub mod args;
pub mod build;
pub mod cargo;
pub mod check;
pub mod errors;
pub mod keys;
pub mod new;
pub mod pack;
pub mod project;
pub mod template;
pub mod verify;

#[cfg(test)]
mod testutil;

use args::Command;
use cargo::Cargo;
use paddock_cli::{CliError, CommonFlags, Exit, Help, Report, finish, version_line};
use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The program's name in `--help` and `--version`.
pub const NAME: &str = "cargo paddock-plugin";

/// What a command works with, so tests supply a folder, a fake `cargo` and a place for the notes.
pub struct Context<'a> {
    /// The working folder; relative paths in arguments are relative to it.
    pub cwd: PathBuf,
    /// Runs `cargo`.
    pub cargo: &'a dyn Cargo,
    /// Writes a progress note (standard error unless `--quiet`).
    pub note: &'a dyn Fn(&str),
    /// `--quiet`: cargo is told too.
    pub quiet: bool,
}

impl Context<'_> {
    /// The project folder: `--dir` (relative to the working folder) or the working folder.
    pub fn dir(&self, dir: Option<&Path>) -> PathBuf {
        dir.map_or_else(|| self.cwd.clone(), |d| self.cwd.join(d))
    }
}

/// Runs one parsed command.
///
/// # Errors
///
/// See the command's module.
pub fn execute(command: &Command, ctx: &Context) -> Result<Report, CliError> {
    match command {
        Command::New(a) => new::new(ctx, a),
        Command::Build(a) => build::build(ctx, a),
        Command::Pack(a) => pack::pack(ctx, a),
        Command::Keygen(a) => keys::keygen(ctx, a),
        Command::Sign(a) => keys::sign(ctx, a),
        Command::Verify(a) => verify::verify(ctx, a),
        Command::Run(a) => check::run(ctx, a),
        Command::Test(a) => check::test(ctx, a),
    }
}

/// The `--help` text.
pub fn help() -> String {
    Help::new(NAME, "build, package, sign and test Paddock plugins")
        .command("new <name>", "make a plugin project (depends on paddock-plugin-api alone)")
        .command("build", "cargo build --release for this computer or each --target")
        .command("pack", "write dist/<id>-<version>.paddockplugin (deterministic; signed with --key)")
        .command("keygen", "make a publisher key pair: the secret key goes to --out (never overwritten)")
        .command("sign <package>", "sign an existing package with --key")
        .command("verify <package>", "check checksums and signature; with --trust, that a trusted key signed it")
        .command("run", "start the built plugin against a fake host and print what it describes")
        .command("test", "start it and run the conformance checks (exit 5 when something is found)")
        .option("--dir <project>", "the project folder (default: the current folder)")
        .option("--target <triple>", "build or pack for this target (repeat for several)")
        .option("--program <target>=<file>", "pack: a program built elsewhere; run/test: --program <file> to start")
        .option("--key <file>", "the secret key file to sign with")
        .option("--out <file>", "pack/sign: the package file; keygen: the key file")
        .option("--trust <public key or file>", "verify: a trusted publisher key (repeat for several)")
        .option("--id <id>, --publisher <name>", "new: the plugin id and the publisher shown to users")
        .option("--api-path <crates folder>", "new: depend on a checkout's platform crates by path")
        .option("--no-build", "run/test: use the program already built")
        .common_options()
        .exit_codes(&[Exit::Ok, Exit::Failure, Exit::Usage, Exit::Refused, Exit::Findings])
        .example("cargo paddock-plugin new my-plugin && cd my-plugin && cargo paddock-plugin test")
        .example("cargo paddock-plugin pack --key ~/.paddock/me.key && cargo paddock-plugin verify dist/*.paddockplugin")
        .render()
}

/// The whole command line after the program name: `cargo` passes `paddock-plugin` first, which is dropped. Returns the exit
/// code; results go to `out`, failures and notes to `err`.
pub fn run_cli(
    args: &[String],
    cwd: PathBuf,
    cargo: &dyn Cargo,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    let args = match args.split_first() {
        Some((first, rest)) if first == "paddock-plugin" => rest,
        _ => args,
    };
    let (flags, rest) = CommonFlags::extract(args);
    if flags.help {
        let _ = out.write_all(help().as_bytes());
        return Exit::Ok;
    }
    if flags.version {
        let _ = writeln!(out, "{}", version_line(NAME, env!("CARGO_PKG_VERSION")));
        return Exit::Ok;
    }
    let result = {
        let sink = RefCell::new(&mut *err);
        let note = |m: &str| {
            if !flags.quiet {
                let _ = writeln!(sink.borrow_mut(), "{m}");
            }
        };
        let ctx = Context {
            cwd,
            cargo,
            note: &note,
            quiet: flags.quiet,
        };
        args::parse(&rest).and_then(|c| execute(&c, &ctx))
    };
    finish(flags.format, result, out, err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::FakeCargo;

    fn cli(args: &[&str], dir: &Path) -> (Exit, String, String) {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let exit = run_cli(
            &args,
            dir.to_path_buf(),
            FakeCargo::making_nothing(),
            &mut out,
            &mut err,
        );
        (
            exit,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn help_and_version_are_answered_before_anything_else() {
        let dir = tempfile::tempdir().unwrap();
        let (x, out, _) = cli(&["paddock-plugin", "--help"], dir.path());
        assert_eq!(x, Exit::Ok);
        for word in [
            "new <name>",
            "pack",
            "keygen",
            "sign",
            "verify",
            "test",
            "Exit codes",
            "--trust",
        ] {
            assert!(out.contains(word), "{word}");
        }
        let (x, out, _) = cli(&["-V"], dir.path());
        assert_eq!(x, Exit::Ok);
        assert_eq!(out.trim(), format!("{NAME} {}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn results_and_failures_follow_the_family_conventions() {
        let dir = tempfile::tempdir().unwrap();
        // Text: the note goes to stderr only when it is not quiet (new writes none), the result to stdout.
        let (x, out, err) = cli(&["paddock-plugin", "new", "hello"], dir.path());
        assert_eq!((x, err.is_empty()), (Exit::Ok, true), "{err}");
        assert!(out.contains("made the plugin project"));
        // JSON: one object on stdout.
        let (x, out, _) = cli(&["new", "second", "--json", "-q"], dir.path());
        assert_eq!(x, Exit::Ok);
        let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(v["ok"], true);
        // A failure: exit 4, the JSON error object on stdout and the text on stderr.
        let (x, out, err) = cli(&["new", "hello", "--json"], dir.path());
        assert_eq!(x, Exit::Refused);
        let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(
            (v["ok"].as_bool(), v["exit"].as_i64()),
            (Some(false), Some(4))
        );
        assert!(
            err.starts_with("error: ") && err.contains("already exists"),
            "{err}"
        );
        // A wrong command line: exit 2.
        let (x, _, err) = cli(&["paddock-plugin"], dir.path());
        assert_eq!(x, Exit::Usage);
        assert!(err.contains("Say what to do"));
    }

    #[test]
    fn notes_go_to_standard_error_unless_quiet() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("prog");
        std::fs::write(&program, "x").unwrap();
        crate::project::tests::project(dir.path(), "prog");
        let path = program.to_str().unwrap();
        let (x, _, err) = cli(
            &[
                "pack",
                "--program",
                &format!("{}={path}", paddock_plugin_package::manifest::host_target()),
            ],
            dir.path(),
        );
        assert_eq!(x, Exit::Ok);
        assert!(err.contains("Packing"), "{err}");
        let (_, _, err) = cli(
            &[
                "pack",
                "-q",
                "--program",
                &format!("{}={path}", paddock_plugin_package::manifest::host_target()),
            ],
            dir.path(),
        );
        assert!(err.is_empty(), "{err}");
    }

    #[test]
    fn every_command_is_wired_to_its_module() {
        let dir = tempfile::tempdir().unwrap();
        let cargo = FakeCargo::making_nothing();
        let note = |_: &str| {};
        let ctx = Context {
            cwd: dir.path().into(),
            cargo,
            note: &note,
            quiet: false,
        };
        assert_eq!(ctx.dir(None), dir.path());
        assert_eq!(ctx.dir(Some(Path::new("x"))), dir.path().join("x"));
        let key = dir.path().join("k.key");
        for words in [
            vec!["keygen", "--out", key.to_str().unwrap()],
            vec!["sign", "nothing", "--key", key.to_str().unwrap()],
            vec!["verify", "nothing"],
            vec!["pack"],
            vec!["build"],
            vec!["run"],
            vec!["test"],
        ] {
            let c =
                args::parse(&words.iter().map(ToString::to_string).collect::<Vec<_>>()).unwrap();
            let r = execute(&c, &ctx);
            // Each reaches its own module: keygen works, the rest fail on what they need (no project, no package).
            assert_eq!(r.is_ok(), words[0] == "keygen", "{words:?}: {r:?}");
        }
    }
}
