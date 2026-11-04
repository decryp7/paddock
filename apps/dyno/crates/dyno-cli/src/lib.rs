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

/// The arguments for the agent's own command line when `args` is `agent ...` (`dyno agent --version`): the agent program is
/// a GUI-subsystem program on Windows (so the login entry opens no console window) and cannot print or return its exit code
/// to a terminal reliably, so `dyno`, a console program, runs the same commands. `None` for any other command.
pub fn agent_args(args: &[String]) -> Option<&[String]> {
    match args {
        [first, rest @ ..] if first == "agent" => Some(rest),
        _ => None,
    }
}

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
        Command::Awake(cmd) => awake(agent, *cmd, json),
        Command::Idle(cmd) => idle(agent, *cmd, json),
        Command::Wake { target, test } => wake(agent, target, *test, json),
        Command::WakeSchedule(cmd) => wake_schedule(agent, cmd, json),
        Command::Help | Command::Version => Ok(String::new()),
    }
}

/// `dyno idle`: what the sleep and shut down rules are doing, Cancel and Snooze.
fn idle(agent: &dyn Agent, cmd: args::IdleCmd, json: bool) -> Result<String, Problem> {
    let request = match cmd {
        args::IdleCmd::Status => Request::PowerState,
        args::IdleCmd::Cancel => Request::PowerIdleCancel,
        args::IdleCmd::Snooze(minutes) => Request::PowerIdleSnooze { minutes },
    };
    match agent.call(request)? {
        Reply::Power(v) => Ok(data(json, &v.idle, || render::idle_text(&v.idle))),
        _ => Err(unexpected()),
    }
}

/// `dyno awake`: ask the agent, show the view it answers (the agent validates; its message says what to fix).
fn awake(agent: &dyn Agent, cmd: args::AwakeCmd, json: bool) -> Result<String, Problem> {
    use args::{AwakeCmd, AwakeWhen};
    use dyno_core::power::awake::{AwakeScope, AwakeSpec, AwakeUntil, BusyCond};
    // "While busy" uses the limits from the person's Settings, so they are read first.
    let busy_conds = |b: args::BusyKinds| -> Result<Vec<BusyCond>, Problem> {
        let Reply::Settings(s) = agent.call(Request::Settings)? else {
            return Err(unexpected());
        };
        let k = &s.power.keep_awake;
        let mut conds = Vec::new();
        if b.cpu {
            conds.push(BusyCond::Cpu { above_pct: k.cpu_pct });
        }
        if b.network {
            conds.push(BusyCond::Network {
                above_bytes_per_s: k.net_bytes_per_s,
            });
        }
        if b.disk {
            conds.push(BusyCond::Disk {
                above_bytes_per_s: k.disk_bytes_per_s,
            });
        }
        Ok(conds)
    };
    let grace_secs = |agent: &dyn Agent| match agent.call(Request::Settings) {
        Ok(Reply::Settings(s)) => s.power.keep_awake.grace_secs,
        _ => 120,
    };
    let request = match cmd {
        AwakeCmd::Status => Request::PowerState,
        AwakeCmd::Off => Request::PowerStopKeepAwake,
        AwakeCmd::On { when, display } => Request::PowerKeepAwake {
            spec: AwakeSpec {
                scope: if display {
                    AwakeScope::SystemAndDisplay
                } else {
                    AwakeScope::System
                },
                until: match when {
                    AwakeWhen::Indefinite => AwakeUntil::Indefinite,
                    AwakeWhen::ForMinutes(minutes) => AwakeUntil::For { minutes },
                    AwakeWhen::AtMinute(minute_of_day) => AwakeUntil::At { minute_of_day },
                    AwakeWhen::WhileBusy(b) => AwakeUntil::WhileBusy {
                        conds: busy_conds(b)?,
                        grace_secs: grace_secs(agent),
                    },
                    AwakeWhen::WhileProcess(pid) => AwakeUntil::WhileProcess {
                        process: dyno_core::process::ProcessRef { pid, started: None },
                    },
                },
            },
        },
    };
    match agent.call(request)? {
        Reply::Power(v) => Ok(data(json, &*v, || render::awake_text(&v))),
        _ => Err(unexpected()),
    }
}

/// `dyno wake`: find the saved computer by name (without case) or hardware address, start the wake job and follow it to its end.
/// There is no timeout (ER-19): a test wake ends by itself after the Settings' wait.
fn wake(agent: &dyn Agent, target: &str, test: bool, json: bool) -> Result<String, Problem> {
    use dyno_core::jobs::JobOutput;
    use dyno_core::wol::Mac;
    let Reply::Wol(view) = agent.call(Request::WolDevices)? else {
        return Err(unexpected());
    };
    let mac = Mac::parse(target).ok();
    let found: Vec<_> = view
        .devices
        .iter()
        .filter(|d| Some(d.mac) == mac || d.name.eq_ignore_ascii_case(target))
        .collect();
    let id = match found.as_slice() {
        [one] => one.id,
        [] => {
            return Err(Problem::Failed(format!(
                "No saved computer is called \"{target}\". Save it in Dyno's Power page first (Wake devices)."
            )))
        }
        _ => {
            return Err(Problem::Failed(
                "More than one saved computer matches; use its hardware address.".into(),
            ))
        }
    };
    let Reply::Job(job) = agent.call(Request::WolWake { id, test })? else {
        return Err(unexpected());
    };
    loop {
        let Reply::Jobs(all) = agent.call(Request::Jobs)? else {
            return Err(unexpected());
        };
        match all.iter().find(|j| j.id == job.id) {
            Some(j) if j.is_terminal() => {
                if let paddock_jobs::wire::WireState::Failed(m) = &j.state {
                    return Err(Problem::Failed(m.clone()));
                }
                break;
            }
            // A job the agent forgot is over; the result read below says what happened.
            None => break,
            Some(_) => std::thread::sleep(std::time::Duration::from_millis(300)),
        }
    }
    match agent.call(Request::JobResult { id: job.id }) {
        Ok(Reply::JobOutput(o)) => match *o {
            JobOutput::Wol(out) => Ok(data(json, &*out, || format!("{}\n", out.message))),
            _ => Err(unexpected()),
        },
        Ok(_) => Err(unexpected()),
        Err(_) => Ok("Cancelled.\n".into()),
    }
}

/// `dyno wake-schedule`: the agent validates and says what to fix; this resolves names to ids and prints what it answers.
fn wake_schedule(agent: &dyn Agent, cmd: &args::WakeScheduleCmd, json: bool) -> Result<String, Problem> {
    use args::WakeScheduleCmd;
    use dyno_core::power::schedule::{DynoJob, Repeat, RowState, WakeInput, WakeRouteState, WakesView};
    let view = |r: Reply| match r {
        Reply::PowerWakes(v) => Ok(*v),
        _ => Err(unexpected()),
    };
    let text = |v: &WakesView| {
        let mut out = String::new();
        match &v.route {
            WakeRouteState::NeedsHelper { reason } | WakeRouteState::Unavailable { reason } => out.push_str(&format!("{reason}\n")),
            _ => {}
        }
        if v.rows.is_empty() {
            out.push_str("No scheduled wakes.\n");
        }
        for r in &v.rows {
            let e = &r.entry;
            let state = match &r.state {
                RowState::Armed { .. } => "armed".to_string(),
                RowState::Off => "off".to_string(),
                RowState::Paused => "paused (scheduled wake is off)".to_string(),
                RowState::NeedsHelper => "needs the helper".to_string(),
                RowState::Failed { why } => format!("not set: {why}"),
            };
            out.push_str(&format!(
                "{}  {}  {:02}:{:02}  {}\n",
                e.id,
                e.name,
                e.minute_of_day / 60,
                e.minute_of_day % 60,
                state
            ));
        }
        out
    };
    match cmd {
        WakeScheduleCmd::List => {
            let v = view(agent.call(Request::PowerWakes)?)?;
            Ok(data(json, &v, || text(&v)))
        }
        WakeScheduleCmd::Cancel { target } => {
            let now = view(agent.call(Request::PowerWakes)?)?;
            let found: Vec<_> = now
                .rows
                .iter()
                .filter(|r| r.entry.name.eq_ignore_ascii_case(target) || r.entry.id.to_string() == *target)
                .collect();
            let id = match found.as_slice() {
                [one] => one.entry.id,
                [] => {
                    return Err(Problem::Failed(format!(
                        "No schedule is called \"{target}\". See: dyno wake-schedule list"
                    )))
                }
                _ => return Err(Problem::Failed("More than one schedule matches; use its id.".into())),
            };
            let v = view(agent.call(Request::PowerCancelWake { id })?)?;
            Ok(data(json, &v, || text(&v)))
        }
        WakeScheduleCmd::Add {
            name,
            at,
            repeat,
            stay,
            devices,
            job,
        } => {
            let Reply::Wol(saved) = agent.call(Request::WolDevices)? else {
                return Err(unexpected());
            };
            let mut ids = Vec::new();
            for d in devices {
                let mac = dyno_core::wol::Mac::parse(d).ok();
                match saved.devices.iter().find(|s| Some(s.mac) == mac || s.name.eq_ignore_ascii_case(d)) {
                    Some(s) => ids.push(s.id),
                    None => {
                        return Err(Problem::Failed(format!(
                            "No saved computer is called \"{d}\". Save it on the Power page first (Wake devices)."
                        )))
                    }
                }
            }
            let repeat = match repeat.as_str() {
                "once" => Repeat::Once,
                "weekdays" => Repeat::Days { mask: 0b0011111 },
                "weekends" => Repeat::Days { mask: 0b1100000 },
                _ => Repeat::Daily,
            };
            let job = job.as_deref().map(|j| {
                if j == "backup" {
                    DynoJob::BackupCheck
                } else {
                    DynoJob::SoftwareInventory
                }
            });
            let entry = WakeInput {
                id: None,
                name: name.clone(),
                minute_of_day: *at,
                repeat,
                stay_awake_minutes: *stay,
                wake_devices: ids,
                job,
                enabled: true,
            };
            let v = view(agent.call(Request::PowerSetWake { entry })?)?;
            Ok(data(json, &v, || text(&v)))
        }
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

#[cfg(test)]
mod agent_route_tests {
    use super::agent_args;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn agent_commands_go_to_the_agent_and_others_do_not() {
        let a = v(&["agent", "service", "install"]);
        assert_eq!(agent_args(&a), Some(&a[1..]));
        assert_eq!(agent_args(&v(&["agent"])), Some(&[][..]));
        assert_eq!(agent_args(&v(&["status"])), None);
        assert_eq!(agent_args(&v(&["--json", "agent"])), None);
        assert_eq!(agent_args(&[]), None);
    }
}
