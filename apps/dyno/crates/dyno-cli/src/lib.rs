//! The `dyno` command (WP-27, ADR 0014): a second kind of client of the agent's one local protocol. It finds the
//! agent through `agent.json`, refuses another protocol version with a clear message and only reads: no command
//! changes anything on the computer. It links `dyno-core`'s protocol types and `dyno-ipc`, never the platform
//! adapters or the agent.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod args;
pub mod connection;
pub mod render;

pub use connection::{Agent, Env, IpcAgent, Problem};

use args::{Command, Invocation};
use dyno_core::protocol::{LiveDetail, Reply, Request};
use std::io::Write;

/// Runs `dyno` with the arguments after the program name; returns the exit code (0 done, 1 failed, 2 wrong usage, 3 versions
/// differ, 4 Dyno is not running).
pub fn run(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let invocation = match args::parse(args) {
        Ok(i) => i,
        Err(e) => {
            let _ = writeln!(err, "dyno: {e}");
            return 2;
        }
    };
    match invocation.command {
        Command::Help => {
            let _ = write!(out, "{}", args::USAGE);
            return 0;
        }
        Command::Version => {
            let _ = writeln!(out, "dyno {}", env!("CARGO_PKG_VERSION"));
            return 0;
        }
        _ => {}
    }
    let agent = match IpcAgent::connect(env) {
        Ok(a) => a,
        Err(p) => {
            let _ = writeln!(err, "dyno: {}", p.message());
            return p.exit_code();
        }
    };
    run_with(&agent, &invocation, out, err)
}

/// Runs an already parsed command against an agent (the seam the tests use).
pub fn run_with(agent: &dyn Agent, i: &Invocation, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match execute(agent, i) {
        Ok(text) => {
            let _ = write!(out, "{text}");
            0
        }
        Err(p) => {
            let _ = writeln!(err, "dyno: {}", p.message());
            p.exit_code()
        }
    }
}

fn unexpected() -> Problem {
    Problem::Failed("Dyno sent an answer this command does not know; update the dyno command.".into())
}

fn data<T: serde::Serialize>(json: bool, v: &T, text: impl FnOnce() -> String) -> String {
    if json {
        format!("{}\n", render::json(v))
    } else {
        text()
    }
}

fn execute(agent: &dyn Agent, i: &Invocation) -> Result<String, Problem> {
    let json = i.json;
    match &i.command {
        Command::Status => {
            let live = agent.live(LiveDetail::Summary)?;
            let info = agent.about();
            Ok(data(json, &render::status_data(&live, &info), || render::status_text(&live, &info)))
        }
        Command::Top { by, count } => {
            let rows = render::top_rows(&agent.live(LiveDetail::Summary)?, *by, *count);
            Ok(data(json, &rows, || render::top_text(&rows)))
        }
        Command::Snapshot => {
            let live = agent.live(LiveDetail::Processes)?;
            let snapshot = serde_json::json!({ "system": live.system, "processes": live.processes.unwrap_or_default() });
            Ok(format!("{}\n", render::json(&snapshot)))
        }
        Command::WhySlow => match agent.call(Request::WhySlow)? {
            Reply::Diagnosis(d) => Ok(data(json, &d, || render::why_slow_text(&d))),
            _ => Err(unexpected()),
        },
        Command::Alarms { all } => {
            let filter = dyno_core::alarms::AlarmFilter {
                include_shelved: *all,
                limit: 100,
                ..Default::default()
            };
            match agent.call(Request::Alarms { filter })? {
                Reply::Alarms(a) => Ok(data(json, &a, || render::alarms_text(&a))),
                _ => Err(unexpected()),
            }
        }
        Command::CountersList { object: None } => match agent.call(Request::CounterObjects)? {
            Reply::CounterObjects(l) => Ok(data(json, &*l, || render::objects_text(&l))),
            _ => Err(unexpected()),
        },
        Command::CountersList { object: Some(name) } => match agent.call(Request::CounterObject { name: name.clone() })? {
            Reply::CounterObject(o) => Ok(data(json, &*o, || render::object_text(&o))),
            _ => Err(unexpected()),
        },
        Command::CountersSearch { query } => match agent.call(Request::CounterSearch {
            query: query.clone(),
            limit: 50,
        })? {
            Reply::CounterHits(h) => Ok(data(json, &h, || render::hits_text(&h))),
            _ => Err(unexpected()),
        },
        Command::CountersGet { paths } => counters_get(agent, paths, json),
        Command::Help | Command::Version => Ok(String::new()),
    }
}

/// Rates need two readings a moment apart, so a counter is read twice and the second reading is shown.
fn counters_get(agent: &dyn Agent, paths: &[String], json: bool) -> Result<String, Problem> {
    let ask = || match agent.call(Request::CounterValues { paths: paths.to_vec() })? {
        Reply::CounterValues(v) => Ok(v),
        _ => Err(unexpected()),
    };
    let mut values = ask()?;
    if values.iter().any(|v| v.value.value().is_none()) {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        values = ask()?;
    }
    Ok(data(json, &values, || render::values_text(&values)))
}
