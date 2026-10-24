//! The `paddock-bench` command, as a function over arguments and writers (the binary only calls [`run`]).
//!
//! Follows `paddock-cli`: `--json`, `--quiet`, `--help`, `--version`; exit 0 within budget, 1 an unreadable file, 2 a wrong
//! command line, 5 a budget missed (`Exit::Findings`).

use crate::{BenchError, Budgets, Results, check};
use paddock_cli::{CliError, CommonFlags, Exit, Format, Help, Report, finish, version_line};
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;

/// The program's name.
pub const NAME: &str = "paddock-bench";

fn help() -> Help {
    Help::new(
        NAME,
        "check benchmark results against their budgets (rule 16)",
    )
    .command(
        "check",
        "compare --results with --budgets (and --previous); exit 5 when a budget is missed",
    )
    .command("propose", "write a first budgets file from --results")
    .option("--budgets <file>", "the budgets.toml")
    .option("--results <file>", "the results file the benchmark wrote")
    .option(
        "--previous <file>",
        "the last good results, for the regression check",
    )
    .option("--markdown", "print the check as a Markdown table")
    .option(
        "--margin <percent>",
        "propose: room above (below) the measured value, default 25",
    )
    .common_options()
    .exit_codes(&[Exit::Ok, Exit::Failure, Exit::Usage, Exit::Findings])
    .example("paddock-bench check --budgets bench/budgets.toml --results bench/app.results.json")
}

struct Args {
    command: String,
    budgets: Option<PathBuf>,
    results: Option<PathBuf>,
    previous: Option<PathBuf>,
    margin: f64,
    markdown: bool,
}

fn parse(rest: &[String]) -> Result<Args, CliError> {
    let mut a = Args {
        command: String::new(),
        budgets: None,
        results: None,
        previous: None,
        margin: 25.0,
        markdown: false,
    };
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| CliError::usage(format!("{flag} needs a value.")))
        };
        match arg.as_str() {
            "--markdown" => a.markdown = true,
            "--budgets" => a.budgets = Some(value("--budgets")?.into()),
            "--results" => a.results = Some(value("--results")?.into()),
            "--previous" => a.previous = Some(value("--previous")?.into()),
            "--margin" => {
                a.margin = value("--margin")?
                    .parse()
                    .ok()
                    .filter(|m: &f64| (0.0..=100.0).contains(m))
                    .ok_or_else(|| CliError::usage("--margin is a percent between 0 and 100."))?;
            }
            other if other.starts_with('-') => {
                return Err(CliError::usage(format!("unknown option {other}."))
                    .with_hint("Run with --help."));
            }
            other if a.command.is_empty() => a.command = other.to_string(),
            other => return Err(CliError::usage(format!("unexpected argument {other}."))),
        }
    }
    if a.command.is_empty() {
        return Err(
            CliError::usage("write a command: check or propose.").with_hint("Run with --help.")
        );
    }
    Ok(a)
}

fn failure(e: BenchError) -> CliError {
    CliError::failure(e.to_string())
}

fn need(p: &Option<PathBuf>, flag: &str) -> Result<PathBuf, CliError> {
    p.clone()
        .ok_or_else(|| CliError::usage(format!("{flag} is required.")))
}

fn execute(a: &Args, quiet: bool, err: &mut dyn Write) -> Result<Report, CliError> {
    match a.command.as_str() {
        "check" => {
            let budgets = Budgets::load(&need(&a.budgets, "--budgets")?).map_err(failure)?;
            let results = Results::load(&need(&a.results, "--results")?).map_err(failure)?;
            // A missing or damaged previous run only turns the regression check off; the budgets still apply.
            let previous = a.previous.as_deref().and_then(|p| match Results::load(p) {
                Ok(r) => Some(r),
                Err(e) => {
                    if !quiet {
                        let _ = writeln!(err, "note: no regression check, {e}");
                    }
                    None
                }
            });
            let outcome = check(&budgets, &results, previous.as_ref());
            let text = if a.markdown {
                outcome.render_markdown()
            } else {
                outcome.render_text()
            };
            let report = Report::new(outcome.to_json(), text);
            Ok(if outcome.failed() {
                report.with_exit(Exit::Findings)
            } else {
                report
            })
        }
        "propose" => {
            let results = Results::load(&need(&a.results, "--results")?).map_err(failure)?;
            let text = Budgets::propose(&results, a.margin)
                .to_toml()
                .map_err(failure)?;
            Ok(Report::new(Value::Null, text))
        }
        other => Err(CliError::usage(format!("unknown command {other}."))
            .with_hint("Write check or propose.")),
    }
}

/// Runs the command with `args` (the program name not included); returns the exit code to end with.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Exit {
    let (flags, rest) = CommonFlags::extract(args);
    if flags.help {
        let _ = write!(out, "{}", help().render());
        return Exit::Ok;
    }
    if flags.version {
        let _ = writeln!(out, "{}", version_line(NAME, env!("CARGO_PKG_VERSION")));
        return Exit::Ok;
    }
    let result = parse(&rest).and_then(|a| execute(&a, flags.quiet, err));
    // `propose` prints TOML, which JSON mode would wrap; it is TOML in both, so it bypasses `Format::Json`.
    let format = if rest.first().map(String::as_str) == Some("propose") {
        Format::Text
    } else {
        flags.format
    };
    finish(format, result, out, err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Kind, Measurement};

    fn go(args: &[&str]) -> (Exit, String, String) {
        let args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let x = run(&args, &mut o, &mut e);
        (
            x,
            String::from_utf8(o).unwrap(),
            String::from_utf8(e).unwrap(),
        )
    }

    /// Writes results with one query-latency measurement `q` and a budgets file with limit 100; returns their paths.
    fn files(dir: &std::path::Path, value: f64) -> (String, String) {
        let mut r = Results::new("demo", "1");
        r.measurements.push(Measurement {
            name: "q".into(),
            kind: Kind::QueryLatency,
            value,
            runs: 3,
        });
        let rp = dir.join("r.json");
        r.save(&rp).unwrap();
        let bp = dir.join("b.toml");
        std::fs::write(&bp, "[[budget]]\nname = \"q\"\nlimit = 100\n").unwrap();
        (bp.to_string_lossy().into(), rp.to_string_lossy().into())
    }

    #[test]
    fn help_and_version_exit_0() {
        let (x, o, _) = go(&["--help"]);
        assert_eq!(x, Exit::Ok);
        assert!(
            o.contains("Usage:") && o.contains("check") && o.contains("5"),
            "{o}"
        );
        let (x, o, _) = go(&["-V"]);
        assert_eq!((x, o.starts_with("paddock-bench ")), (Exit::Ok, true));
    }

    #[test]
    fn check_passes_fails_and_speaks_json() {
        let d = tempfile::tempdir().unwrap();
        let (b, r) = files(d.path(), 90.0);
        let (x, o, _) = go(&["check", "--budgets", &b, "--results", &r]);
        assert_eq!(x, Exit::Ok);
        assert!(o.contains("1 passed, 0 failed"), "{o}");
        let (b, r) = files(d.path(), 500.0);
        let (x, o, _) = go(&["check", "--budgets", &b, "--results", &r, "--json"]);
        assert_eq!(x, Exit::Findings);
        let v: Value = serde_json::from_str(o.trim()).unwrap();
        assert_eq!(
            (v["ok"].as_bool(), v["withinBudget"].as_bool()),
            (Some(true), Some(false))
        );
        let (_, o, _) = go(&["check", "--budgets", &b, "--results", &r, "--markdown"]);
        assert!(o.contains("| q |"), "{o}");
    }

    #[test]
    fn a_broken_previous_run_only_warns_unless_quiet() {
        let d = tempfile::tempdir().unwrap();
        let (b, r) = files(d.path(), 90.0);
        let bad = d.path().join("prev.json");
        std::fs::write(&bad, "{").unwrap();
        let bad = bad.to_string_lossy().into_owned();
        let (x, _, e) = go(&[
            "check",
            "--budgets",
            &b,
            "--results",
            &r,
            "--previous",
            &bad,
        ]);
        assert!(x == Exit::Ok && e.contains("no regression check"), "{e}");
        let (x, _, e) = go(&[
            "check",
            "--budgets",
            &b,
            "--results",
            &r,
            "--previous",
            &bad,
            "-q",
        ]);
        assert!(x == Exit::Ok && e.is_empty(), "{e}");
        let (x, _, _) = go(&["check", "--budgets", &b, "--results", &r, "--previous", &r]);
        assert_eq!(x, Exit::Ok, "a good previous run is used");
    }

    #[test]
    fn propose_prints_toml_even_in_json_mode() {
        let d = tempfile::tempdir().unwrap();
        let (_, r) = files(d.path(), 80.0);
        for extra in [&[][..], &["--json"][..]] {
            let mut args = vec!["propose", "--results", &r, "--margin", "50"];
            args.extend_from_slice(extra);
            let (x, o, _) = go(&args);
            assert_eq!(x, Exit::Ok);
            assert!(
                o.contains("name = \"q\"") && o.contains("limit = 120"),
                "{o}"
            );
        }
    }

    #[test]
    fn wrong_command_lines_exit_2_and_unreadable_files_exit_1() {
        let d = tempfile::tempdir().unwrap();
        let (b, r) = files(d.path(), 90.0);
        for args in [
            vec![],
            vec!["frobnicate"],
            vec!["check"],
            vec!["check", "--budgets", &b],
            vec!["propose"],
            vec!["check", "--nope"],
            vec!["check", "--budgets"],
            vec!["check", "extra", "more"],
            vec!["propose", "--margin", "500"],
            vec!["propose", "--margin", "x"],
        ] {
            let (x, _, e) = go(&args);
            assert_eq!(x, Exit::Usage, "{args:?}: {e}");
            assert!(e.starts_with("error: "), "{e}");
        }
        let missing = d.path().join("none.json").to_string_lossy().into_owned();
        assert_eq!(
            go(&["check", "--budgets", &b, "--results", &missing]).0,
            Exit::Failure
        );
        assert_eq!(
            go(&["check", "--budgets", &missing, "--results", &r]).0,
            Exit::Failure
        );
        assert_eq!(go(&["propose", "--results", &missing]).0, Exit::Failure);
        let (x, o, _) = go(&["check", "--json", "--budgets", &missing, "--results", &r]);
        assert_eq!(x, Exit::Failure);
        assert!(o.contains("\"ok\":false"), "{o}");
    }
}
