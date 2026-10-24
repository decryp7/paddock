//! `paddock-bench`: the budget check of the nightly benchmark workflow, and the first `budgets.toml` from a good run.
//!
//! ```text
//! paddock-bench check   --budgets bench/budgets.toml --results bench/app.results.json [--previous old.json] [--json] [--markdown]
//! paddock-bench propose --results bench/app.results.json [--margin 25]
//! ```
//!
//! Exit codes follow the Paddock CLI conventions (`paddock-cli`): 0 all within budget, 1 a file could not be read, 2 the
//! command line is wrong, 5 a budget was missed or a measurement regressed.

use paddock_bench::{Budgets, Results, check};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "paddock-bench - benchmark budgets (rule 16)

Usage:
  paddock-bench check --budgets <file> --results <file> [--previous <file>] [--json] [--markdown]
  paddock-bench propose --results <file> [--margin <percent>]

Exit codes: 0 within budget, 1 a file could not be read, 2 wrong command line, 5 a budget was missed.";

struct Args {
    command: String,
    budgets: Option<PathBuf>,
    results: Option<PathBuf>,
    previous: Option<PathBuf>,
    margin: f64,
    json: bool,
    markdown: bool,
}

fn parse(args: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args {
        command: String::new(),
        budgets: None,
        results: None,
        previous: None,
        margin: 25.0,
        json: false,
        markdown: false,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--json" => a.json = true,
            "--markdown" => a.markdown = true,
            "--budgets" => a.budgets = Some(value("--budgets")?.into()),
            "--results" => a.results = Some(value("--results")?.into()),
            "--previous" => a.previous = Some(value("--previous")?.into()),
            "--margin" => {
                a.margin = value("--margin")?
                    .parse()
                    .ok()
                    .filter(|m: &f64| (0.0..=100.0).contains(m))
                    .ok_or("--margin is a percent between 0 and 100")?;
            }
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other if a.command.is_empty() => a.command = other.to_string(),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    if a.command.is_empty() {
        return Err("write a command: check or propose".into());
    }
    Ok(Some(a))
}

fn run(a: &Args) -> Result<u8, (u8, String)> {
    let io = |e: paddock_bench::BenchError| (1, e.to_string());
    let need =
        |p: &Option<PathBuf>, flag: &str| p.clone().ok_or((2, format!("{flag} is required")));
    match a.command.as_str() {
        "check" => {
            let budgets = Budgets::load(&need(&a.budgets, "--budgets")?).map_err(io)?;
            let results = Results::load(&need(&a.results, "--results")?).map_err(io)?;
            // A missing or damaged previous run only turns the regression check off; the budgets still apply.
            let previous = a.previous.as_deref().and_then(|p| match Results::load(p) {
                Ok(r) => Some(r),
                Err(e) => {
                    eprintln!("note: no regression check, {e}");
                    None
                }
            });
            let outcome = check(&budgets, &results, previous.as_ref());
            if a.json {
                println!("{}", outcome.to_json());
            } else if a.markdown {
                print!("{}", outcome.render_markdown());
            } else {
                print!("{}", outcome.render_text());
            }
            Ok(if outcome.failed() { 5 } else { 0 })
        }
        "propose" => {
            let results = Results::load(&need(&a.results, "--results")?).map_err(io)?;
            print!(
                "{}",
                Budgets::propose(&results, a.margin).to_toml().map_err(io)?
            );
            Ok(0)
        }
        other => Err((
            2,
            format!("unknown command {other}: write check or propose"),
        )),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(None) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Some(a)) => match run(&a) {
            Ok(code) => ExitCode::from(code),
            Err((code, message)) => {
                eprintln!("error: {message}");
                ExitCode::from(code)
            }
        },
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
