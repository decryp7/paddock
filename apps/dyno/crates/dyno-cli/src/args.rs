//! The command line, parsed once into a typed [`Invocation`] (parse, don't validate).

/// What to sort `top` by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBy {
    /// CPU.
    Cpu,
    /// Memory.
    Memory,
}

/// A command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `dyno status`: how the computer is doing right now.
    Status,
    /// `dyno why-slow`: the diagnosis.
    WhySlow,
    /// `dyno top`: the busiest apps.
    Top {
        /// Sort key.
        by: TopBy,
        /// How many rows.
        count: usize,
    },
    /// `dyno alarms`: open alarms.
    Alarms {
        /// Include shelved ones.
        all: bool,
    },
    /// `dyno snapshot`: everything measured right now, as one document (nothing is stored).
    Snapshot,
    /// `dyno counters list [object]`.
    CountersList {
        /// An object to open (the objects when absent).
        object: Option<String>,
    },
    /// `dyno counters search <words>`.
    CountersSearch {
        /// The words.
        query: String,
    },
    /// `dyno counters get <path>...`.
    CountersGet {
        /// The counter paths.
        paths: Vec<String>,
    },
    /// `dyno awake [on|off|status]`: keep the computer awake (ADR 0032).
    Awake(AwakeCmd),
    /// `dyno idle [status|cancel|snooze MIN]`: the sleep and shut down rules' state, and Cancel (ADR 0032).
    Idle(IdleCmd),
    /// `dyno wake <name|address> [--test]`: wake a saved computer over the network (ADR 0032).
    Wake {
        /// The saved computer's name, or its hardware address.
        target: String,
        /// Wait for it to answer and say whether it woke.
        test: bool,
    },
    /// `dyno wake-schedule list|add|cancel`: scheduled wakes of this computer (ADR 0032 section 5).
    WakeSchedule(WakeScheduleCmd),
    /// `dyno --help`.
    Help,
    /// `dyno --version`.
    Version,
}

/// What `dyno idle` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleCmd {
    /// Show what the idle and timed rules are doing.
    Status,
    /// Cancel the countdown.
    Cancel,
    /// Snooze it this many minutes.
    Snooze(u32),
}

/// What `dyno wake-schedule` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WakeScheduleCmd {
    /// Show the schedules.
    List,
    /// Add one: at a time of day, then stay awake, wake saved computers or run a job.
    Add {
        /// The schedule's name.
        name: String,
        /// The local time of day, minutes after midnight.
        at: u16,
        /// `once`, `daily`, `weekdays` or `weekends`.
        repeat: String,
        /// Stay awake this many minutes.
        stay: Option<u32>,
        /// Saved computers to wake, by name or hardware address.
        devices: Vec<String>,
        /// `backup` or `software`.
        job: Option<String>,
    },
    /// Cancel one, by name or id.
    Cancel {
        /// Its name or id.
        target: String,
    },
}

/// What `dyno awake` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwakeCmd {
    /// Show whether the computer is being held awake.
    Status,
    /// Hold it awake.
    On {
        /// Until when.
        when: AwakeWhen,
        /// Keep the display on too.
        display: bool,
    },
    /// Let go.
    Off,
}

/// Until when `dyno awake on` holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwakeWhen {
    /// Until `dyno awake off`.
    Indefinite,
    /// For this many minutes.
    ForMinutes(u32),
    /// Until the next time the local clock shows this minute of the day.
    AtMinute(u16),
    /// While any of these is busy (the limits are the person's Settings); a program is followed with `--while-pid`.
    WhileBusy(BusyKinds),
    /// While this process runs (the agent adds its start time).
    WhileProcess(u32),
}

/// Which "busy" conditions `--busy` named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BusyKinds {
    /// CPU.
    pub cpu: bool,
    /// Network.
    pub network: bool,
    /// Disk.
    pub disk: bool,
}

/// "30m", "2h", "1h30m", "45" (minutes) as minutes; the engine checks the range.
fn parse_minutes(text: &str) -> Option<u32> {
    let t = text.trim().to_ascii_lowercase();
    if let Ok(n) = t.parse::<u32>() {
        return Some(n);
    }
    let (mut total, mut digits) = (0u32, String::new());
    for c in t.chars() {
        match c {
            '0'..='9' => digits.push(c),
            'h' | 'm' => {
                let n: u32 = digits.parse().ok()?;
                digits.clear();
                total = total.checked_add(if c == 'h' { n.checked_mul(60)? } else { n })?;
            }
            _ => return None,
        }
    }
    (digits.is_empty() && total > 0).then_some(total)
}

/// "cpu,disk" as the conditions named.
fn parse_busy(text: &str) -> Option<BusyKinds> {
    let mut b = BusyKinds::default();
    for part in text.split(',') {
        match part.trim().to_ascii_lowercase().as_str() {
            "cpu" => b.cpu = true,
            "network" | "net" => b.network = true,
            "disk" => b.disk = true,
            _ => return None,
        }
    }
    (b != BusyKinds::default()).then_some(b)
}

/// "18:30" as the minute of the day.
fn parse_clock(text: &str) -> Option<u16> {
    let (h, m) = text.trim().split_once(':')?;
    let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// What to do.
    pub command: Command,
    /// Machine-readable output.
    pub json: bool,
}

/// The usage text.
pub const USAGE: &str = "dyno: look at how your computer is doing, from a terminal or a script\n\n\
usage: dyno <command> [options]\n\n\
commands:\n  \
mcp                                   speak MCP on stdin/stdout for a host (key in DYNO_MCP_KEY; Dyno must be open)\n  \
status [--json]                       how the computer is doing right now\n  \
idle [status|cancel|snooze MIN]       the sleep/shut down rules: what they wait for; cancel a countdown\n  \
awake [on|off|status] [--for 90m|2h] [--until 18:30] [--busy cpu,network,disk] [--while-pid N] [--display]\n  \
                                      keep the computer awake (on without --for or --until: until `awake off`)\n  \
wake NAME|ADDRESS [--test] [--json]   wake a saved computer (Wake-on-LAN); --test waits for it to answer\n  \
wake-schedule list|add NAME --at 07:30 [--repeat once|daily|weekdays|weekends] [--stay 30m] [--device NAME]... [--job backup|software]|cancel NAME|ID\n  \
                                      scheduled wakes of this computer\n  \
why-slow [--json]                     why it feels slow, in plain words\n  \
top [--by cpu|memory] [-n N] [--json] the busiest apps\n  \
alarms [--all] [--json]               alarms that need attention\n  \
snapshot                              everything measured now, as JSON (nothing is stored)\n  \
counters list [OBJECT] [--json]       the performance counters this computer offers\n  \
counters search WORDS... [--json]     find a counter by name\n  \
counters get PATH... [--json]         read counters now (PATH looks like \\Memory\\Free)\n  \
agent COMMAND...                      the background agent's own commands (--version, status, stop, once,\n  \
                                      service install|uninstall|status|ensure); on Windows the agent program\n  \
                                      has no console, so use this to see its output and exit code\n\n\
Dyno only looks: no command changes anything on your computer, except `awake`, `wake` and `wake-schedule` (power, only when you ask), `agent stop` and `agent service`.\n\
exit codes: 0 done, 1 failed, 2 wrong usage, 3 versions differ, 4 Dyno is not running\n";

/// Why a command line is not usable (the message says what to type instead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError(pub String);

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

fn err<T>(msg: impl Into<String>) -> Result<T, UsageError> {
    Err(UsageError(msg.into()))
}

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Invocation, UsageError> {
    let mut json = false;
    let mut all = false;
    let mut by = TopBy::Cpu;
    let mut count = 10usize;
    let mut words: Vec<String> = Vec::new();
    let mut for_minutes: Option<u32> = None;
    let mut until_clock: Option<u16> = None;
    let mut display = false;
    let mut busy: Option<BusyKinds> = None;
    let mut while_pid: Option<u32> = None;
    let mut test = false;
    let mut repeat: Option<String> = None;
    let mut devices: Vec<String> = Vec::new();
    let mut job: Option<String> = None;
    let mut stay: Option<u32> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--all" => all = true,
            "--display" => display = true,
            "--test" => test = true,
            "--repeat" => {
                repeat = match it.next().map(String::as_str) {
                    Some(r @ ("once" | "daily" | "weekdays" | "weekends")) => Some(r.to_string()),
                    _ => return err("--repeat takes once, daily, weekdays or weekends."),
                }
            }
            "--device" => match it.next() {
                Some(d) if !d.starts_with('-') => devices.push(d.clone()),
                _ => return err("--device takes the name or hardware address of a saved computer."),
            },
            "--job" => {
                job = match it.next().map(String::as_str) {
                    Some(j @ ("backup" | "software")) => Some(j.to_string()),
                    _ => return err("--job takes backup or software."),
                }
            }
            "--stay" => {
                stay = match it.next().map(|n| parse_minutes(n)) {
                    Some(Some(n)) => Some(n),
                    _ => return err("--stay takes a length such as 30m or 2h."),
                }
            }
            "--for" => {
                for_minutes = match it.next().map(|n| parse_minutes(n)) {
                    Some(Some(n)) => Some(n),
                    _ => return err("--for takes a length such as 30m, 2h or 1h30m."),
                }
            }
            "--busy" => {
                busy = match it.next().map(|n| parse_busy(n)) {
                    Some(Some(b)) => Some(b),
                    _ => return err("--busy takes cpu, network and disk, separated by commas, for example --busy cpu,disk."),
                }
            }
            "--while-pid" => {
                while_pid = match it.next().map(|n| n.parse::<u32>()) {
                    Some(Ok(n)) if n > 0 => Some(n),
                    _ => return err("--while-pid takes the process id of a running program."),
                }
            }
            "--until" | "--at" => {
                until_clock = match it.next().map(|n| parse_clock(n)) {
                    Some(Some(n)) => Some(n),
                    _ => return err("--until takes a time of day such as 18:30."),
                }
            }
            "--by" => {
                by = match it.next().map(String::as_str) {
                    Some("cpu") => TopBy::Cpu,
                    Some("memory") => TopBy::Memory,
                    _ => return err("--by takes cpu or memory."),
                }
            }
            "-n" | "--count" => {
                count = match it.next().map(|n| n.parse::<usize>()) {
                    Some(Ok(n)) if (1..=100).contains(&n) => n,
                    _ => return err("-n takes a number from 1 to 100."),
                }
            }
            "-h" | "--help" | "help" => {
                return Ok(Invocation {
                    command: Command::Help,
                    json,
                })
            }
            "-V" | "--version" => {
                return Ok(Invocation {
                    command: Command::Version,
                    json,
                })
            }
            flag if flag.starts_with('-') && flag.len() > 1 => return err(format!("Unknown option {flag}. Try: dyno --help")),
            _ => words.push(a.clone()),
        }
    }
    let mut w = words.iter();
    let command = match w.next().map(String::as_str) {
        None => return err("Say what to show, for example: dyno status. Try: dyno --help"),
        Some("status") => Command::Status,
        Some("why-slow") => Command::WhySlow,
        Some("top") => Command::Top { by, count },
        Some("alarms") => Command::Alarms { all },
        Some("snapshot") => Command::Snapshot,
        Some("idle") => match (w.next().map(String::as_str), w.next()) {
            (None, None) | (Some("status"), None) => Command::Idle(IdleCmd::Status),
            (Some("cancel"), None) => Command::Idle(IdleCmd::Cancel),
            (Some("snooze"), Some(m)) => match m.parse::<u32>() {
                Ok(m) if (1..=1440).contains(&m) => Command::Idle(IdleCmd::Snooze(m)),
                _ => return err("snooze takes minutes from 1 to 1440, for example: dyno idle snooze 30"),
            },
            (Some("snooze"), None) => return err("Say how long to snooze, for example: dyno idle snooze 30"),
            _ => return err("idle takes status, cancel or snooze MINUTES. Try: dyno --help"),
        },
        Some("awake") => {
            let sub = w.next().map(String::as_str);
            let timed = for_minutes.is_some() || until_clock.is_some() || busy.is_some() || while_pid.is_some();
            match sub {
                None | Some("status") if !timed && !display => Command::Awake(AwakeCmd::Status),
                Some("on") => {
                    let named = [for_minutes.is_some(), until_clock.is_some(), busy.is_some(), while_pid.is_some()];
                    if named.iter().filter(|n| **n).count() > 1 {
                        return err("Say only one of --for, --until, --busy and --while-pid.");
                    }
                    let when = match (for_minutes, until_clock, busy, while_pid) {
                        (Some(n), ..) => AwakeWhen::ForMinutes(n),
                        (_, Some(m), ..) => AwakeWhen::AtMinute(m),
                        (_, _, Some(b), _) => AwakeWhen::WhileBusy(b),
                        (_, _, _, Some(pid)) => AwakeWhen::WhileProcess(pid),
                        _ => AwakeWhen::Indefinite,
                    };
                    Command::Awake(AwakeCmd::On { when, display })
                }
                Some("off") if !timed && !display => Command::Awake(AwakeCmd::Off),
                None | Some("status") | Some("off") => {
                    return err("--for, --until, --busy, --while-pid and --display belong to: dyno awake on")
                }
                Some(other) => return err(format!("awake takes on, off or status, not \"{other}\". Try: dyno --help")),
            }
        }
        Some("wake") => {
            let target = w.by_ref().map(String::as_str).collect::<Vec<_>>().join(" ");
            if target.trim().is_empty() {
                return err("Say which computer to wake, for example: dyno wake \"Media PC\" --test");
            }
            Command::Wake {
                target: target.trim().to_string(),
                test,
            }
        }
        Some("wake-schedule") => match w.next().map(String::as_str) {
            None | Some("list") => Command::WakeSchedule(WakeScheduleCmd::List),
            Some("add") => {
                let name = w.by_ref().map(String::as_str).collect::<Vec<_>>().join(" ");
                let Some(at) = until_clock else {
                    return err("Say when with --at 07:30, for example: dyno wake-schedule add Morning --at 07:30 --stay 30m");
                };
                if name.trim().is_empty() {
                    return err("Name the schedule, for example: dyno wake-schedule add Morning --at 07:30 --stay 30m");
                }
                Command::WakeSchedule(WakeScheduleCmd::Add {
                    name: name.trim().to_string(),
                    at,
                    repeat: repeat.unwrap_or_else(|| "daily".into()),
                    stay,
                    devices,
                    job,
                })
            }
            Some("cancel") => {
                let target = w.by_ref().map(String::as_str).collect::<Vec<_>>().join(" ");
                if target.trim().is_empty() {
                    return err("Say which schedule to cancel, by name or id: dyno wake-schedule cancel Morning");
                }
                Command::WakeSchedule(WakeScheduleCmd::Cancel {
                    target: target.trim().to_string(),
                })
            }
            Some(other) => {
                return err(format!(
                    "wake-schedule takes list, add or cancel, not \"{other}\". Try: dyno --help"
                ))
            }
        },
        Some("counters") => match w.next().map(String::as_str) {
            Some("list") => Command::CountersList {
                object: {
                    let rest: Vec<&str> = w.by_ref().map(String::as_str).collect();
                    (!rest.is_empty()).then(|| rest.join(" "))
                },
            },
            Some("search") => {
                let q = w.by_ref().map(String::as_str).collect::<Vec<_>>().join(" ");
                if q.trim().is_empty() {
                    return err("Type the words to look for, for example: dyno counters search disk queue");
                }
                Command::CountersSearch { query: q }
            }
            Some("get") => {
                let paths: Vec<String> = w.by_ref().cloned().collect();
                if paths.is_empty() {
                    return err("Give at least one counter path, for example: dyno counters get \"\\Memory\\Free\"");
                }
                Command::CountersGet { paths }
            }
            _ => return err("counters takes list, search or get. Try: dyno --help"),
        },
        Some(other) => return err(format!("Unknown command \"{other}\". Try: dyno --help")),
    };
    // Anything left over is a mistake worth saying: a typo must never silently change what a command does.
    let extra: Vec<&String> = w.collect();
    if !extra.is_empty() && !matches!(command, Command::CountersGet { .. } | Command::CountersSearch { .. }) {
        return err(format!("Unexpected \"{}\". Try: dyno --help", extra[0]));
    }
    Ok(Invocation { command, json })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Invocation, UsageError> {
        parse(&s.split_whitespace().map(String::from).collect::<Vec<_>>())
    }

    fn cmd(s: &str) -> Command {
        p(s).unwrap().command
    }

    #[test]
    fn commands_and_their_options() {
        assert_eq!(cmd("status"), Command::Status);
        assert!(p("status --json").unwrap().json);
        assert_eq!(cmd("why-slow"), Command::WhySlow);
        assert_eq!(cmd("top"), Command::Top { by: TopBy::Cpu, count: 10 });
        assert_eq!(
            cmd("top --by memory -n 3"),
            Command::Top {
                by: TopBy::Memory,
                count: 3
            }
        );
        assert_eq!(cmd("alarms --all"), Command::Alarms { all: true });
        assert_eq!(cmd("snapshot"), Command::Snapshot);
        assert_eq!(cmd("counters list"), Command::CountersList { object: None });
        assert_eq!(
            cmd("counters list Network Interface"),
            Command::CountersList {
                object: Some("Network Interface".into())
            }
        );
        assert_eq!(
            cmd("counters search disk queue"),
            Command::CountersSearch {
                query: "disk queue".into()
            }
        );
        assert_eq!(
            cmd("counters get \\Memory\\Free \\A\\B"),
            Command::CountersGet {
                paths: vec!["\\Memory\\Free".into(), "\\A\\B".into()]
            }
        );
    }

    #[test]
    fn awake_takes_on_off_status_with_a_length_or_a_time() {
        use AwakeWhen::*;
        let on = |when, display| Command::Awake(AwakeCmd::On { when, display });
        assert_eq!(cmd("awake"), Command::Awake(AwakeCmd::Status));
        assert_eq!(cmd("awake status"), Command::Awake(AwakeCmd::Status));
        assert_eq!(cmd("awake off"), Command::Awake(AwakeCmd::Off));
        assert_eq!(cmd("awake on"), on(Indefinite, false));
        assert_eq!(cmd("awake on --for 90m"), on(ForMinutes(90), false));
        assert_eq!(cmd("awake on --for 2h --display"), on(ForMinutes(120), true));
        assert_eq!(cmd("awake on --for 1h30m"), on(ForMinutes(90), false));
        assert_eq!(cmd("awake on --for 45"), on(ForMinutes(45), false));
        assert_eq!(cmd("awake on --until 18:30"), on(AtMinute(1110), false));
        assert_eq!(
            cmd("awake on --busy cpu,disk"),
            on(
                WhileBusy(BusyKinds {
                    cpu: true,
                    network: false,
                    disk: true
                }),
                false
            )
        );
        assert_eq!(
            cmd("awake on --busy net"),
            on(
                WhileBusy(BusyKinds {
                    network: true,
                    ..Default::default()
                }),
                false
            )
        );
        assert_eq!(cmd("awake on --while-pid 4242 --display"), on(WhileProcess(4242), true));
        assert!(p("awake --json").unwrap().json);
    }

    #[test]
    fn idle_shows_cancels_and_snoozes() {
        assert_eq!(cmd("idle"), Command::Idle(IdleCmd::Status));
        assert_eq!(cmd("idle status"), Command::Idle(IdleCmd::Status));
        assert_eq!(cmd("idle cancel"), Command::Idle(IdleCmd::Cancel));
        assert_eq!(cmd("idle snooze 30"), Command::Idle(IdleCmd::Snooze(30)));
        for bad in [
            "idle snooze",
            "idle snooze 0",
            "idle snooze 1441",
            "idle snooze x",
            "idle cancel now",
            "idle later",
            "idle status extra",
        ] {
            assert!(p(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn wake_schedule_takes_list_add_and_cancel() {
        assert_eq!(p("wake-schedule").unwrap().command, Command::WakeSchedule(WakeScheduleCmd::List));
        assert_eq!(
            p("wake-schedule add Morning run --at 07:30 --repeat weekdays --stay 30m --device NAS --job backup")
                .unwrap()
                .command,
            Command::WakeSchedule(WakeScheduleCmd::Add {
                name: "Morning run".into(),
                at: 450,
                repeat: "weekdays".into(),
                stay: Some(30),
                devices: vec!["NAS".into()],
                job: Some("backup".into()),
            })
        );
        assert_eq!(
            p("wake-schedule cancel 3").unwrap().command,
            Command::WakeSchedule(WakeScheduleCmd::Cancel { target: "3".into() })
        );
        for bad in [
            "wake-schedule add Morning",
            "wake-schedule add --at 07:30",
            "wake-schedule cancel",
            "wake-schedule frob",
            "wake-schedule add x --at 07:30 --repeat sometimes",
            "wake-schedule add x --at 07:30 --job rm",
        ] {
            assert!(p(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn wake_takes_a_name_or_address_and_an_optional_test() {
        assert_eq!(
            p("wake Media PC --test").unwrap().command,
            Command::Wake {
                target: "Media PC".into(),
                test: true
            }
        );
        assert_eq!(
            p("wake 02:00:5e:10:00:01").unwrap().command,
            Command::Wake {
                target: "02:00:5e:10:00:01".into(),
                test: false
            }
        );
        assert!(p("wake").is_err());
        assert!(p("wake --test").is_err());
    }

    #[test]
    fn awake_mistakes_say_what_to_type() {
        for bad in [
            "awake on --for",
            "awake on --for soon",
            "awake on --for 0m5",
            "awake on --for 0m",
            "awake on --for 99999999999h",
            "awake on --until 25:00",
            "awake on --until 9",
            "awake on --until",
            "awake on --for 1h --until 18:00",
            "awake off --for 1h",
            "awake --display",
            "awake status --until 10:00",
            "awake maybe",
            "awake on --busy",
            "awake on --busy gpu",
            "awake on --busy cpu,",
            "awake on --while-pid 0",
            "awake on --while-pid x",
            "awake on --busy cpu --for 1h",
            "awake off --busy cpu",
            "awake on extra",
        ] {
            assert!(p(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn help_and_version_win_over_a_missing_command() {
        assert_eq!(cmd("--help"), Command::Help);
        assert_eq!(cmd("status -h"), Command::Help);
        assert_eq!(cmd("--version"), Command::Version);
    }

    #[test]
    fn mistakes_are_reported_with_what_to_type() {
        for bad in [
            "",
            "frobnicate",
            "status --nope",
            "status extra",
            "end 4242",
            "cap 1 5",
            "undo abc",
            "top --by speed",
            "top -n 0",
            "top -n",
            "top --by",
            "counters",
            "counters search",
            "counters get",
            "counters nope",
        ] {
            let e = p(bad).unwrap_err();
            assert!(!e.0.is_empty(), "{bad}");
        }
        assert!(p("status --yes").is_err(), "there is nothing to confirm any more");
    }
}
