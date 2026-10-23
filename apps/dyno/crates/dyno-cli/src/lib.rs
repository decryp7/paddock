//! The `dyno` command (WP-27, ADR 0014): a second kind of client of the agent's one local protocol. It finds the
//! agent through `agent.json`, refuses another protocol version with a clear message, reads by default, and changes
//! something only for an explicit acting command with `--yes`, through the same requests, validation, protected list
//! and activity log as a click in the app. It links `dyno-core`'s protocol types and `dyno-ipc`, never the platform
//! adapters or the agent.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod args;
pub mod connection;
pub mod render;

pub use connection::{Agent, Env, IpcAgent, Problem};

use args::{Command, Invocation};
use dyno_core::process::{ProcessAction, ProcessRef};
use dyno_core::protocol::{LiveDetail, Reply, Request};
use std::io::Write;

/// Runs `dyno` with the arguments after the program name; returns the exit code (0 done, 1 failed, 2 wrong usage or
/// `--yes` missing, 3 versions differ, 4 Dyno is not running).
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
    // An acting command without --yes explains itself before touching anything, even before connecting.
    if invocation.command.acts() && !invocation.yes {
        let _ = writeln!(err, "dyno: {}", needs_yes(&invocation.command));
        return 2;
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

fn needs_yes(c: &Command) -> String {
    let what = match c {
        Command::End { pid } => format!("This would ask the app with process id {pid} to exit"),
        Command::Cap { pid, percent } => format!("This would limit the app with process id {pid} to {percent}% of the machine's CPU"),
        Command::Undo { id } => format!("This would undo activity entry {id}"),
        _ => "This would change something".to_string(),
    };
    format!("{what}. Nothing was changed. Add --yes to do it.")
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
            let snapshot =
                serde_json::json!({ "system": live.system, "profile": live.profile, "processes": live.processes.unwrap_or_default() });
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
        Command::End { pid } => act(agent, *pid, ProcessAction::End),
        Command::Cap { pid, percent } => act(agent, *pid, ProcessAction::SetCpuCap(Some(*percent))),
        Command::Undo { id } => match agent.call(Request::Undo { activity_id: id.clone() })? {
            Reply::Activity(a) => Ok(format!("{}\n", a.title)),
            _ => Err(unexpected()),
        },
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

/// Resolves a process id to the exact process (id and start time, so a reused id is never hit), then asks the agent to
/// act on it. The agent applies the protected list and records the activity, exactly as for a click in the app.
fn act(agent: &dyn Agent, pid: u32, action: ProcessAction) -> Result<String, Problem> {
    let live = agent.live(LiveDetail::Processes)?;
    let process = live
        .processes
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.pid == pid)
        .ok_or_else(|| Problem::Failed(format!("No process with id {pid} is running (it may have just exited).")))?;
    let target: ProcessRef = process.reference();
    match agent.call(Request::Act { target, action })? {
        Reply::Activity(a) => {
            let undo = if a.undo == dyno_core::activity::UndoState::Available {
                format!(" Undo with: dyno undo {} --yes", a.id)
            } else {
                String::new()
            };
            Ok(format!("{}.{undo}\n", a.title))
        }
        _ => Err(unexpected()),
    }
}
