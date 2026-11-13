#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The `dyno` command against a real agent over the fake platform: a real IPC connection and discovery file in a
//! temp folder. Nothing here touches the machine: the processes are the fake platform's.

use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_cli::{run, Env};
use dyno_platform::fake::{process, Fake};

struct Harness {
    running: Option<Running>,
    dirs: Dirs,
    env: Env,
    _tmp: paddock_testkit::PrivateTemp,
}

impl Harness {
    fn start() -> Harness {
        let tmp = paddock_testkit::private_tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        for (pid, name, cpu) in [(100, "busy", 60.0), (101, "calm", 1.0), (102, "launchd", 0.1)] {
            fake.add_process(process(pid, name, cpu));
        }
        let running = Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
        let env = Env {
            data_dir: dirs.root().to_path_buf(),
            supplied: false,
        };
        Harness {
            running: Some(running),
            dirs,
            env,
            _tmp: tmp,
        }
    }

    /// Runs `dyno <args>`; returns (exit code, stdout, stderr).
    fn dyno(&self, args: &str) -> (i32, String, String) {
        dyno_in(&self.env, args)
    }
}

fn dyno_in(env: &Env, args: &str) -> (i32, String, String) {
    dyno_argv(env, args.split_whitespace().map(String::from).collect())
}

/// Like `dyno_in` for arguments that contain spaces (a counter path such as `\Memory\Available Bytes`).
fn dyno_argv(env: &Env, args: Vec<String>) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(&args, env, &mut out, &mut err);
    (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

#[test]
fn the_cli_reads_the_discovery_file_the_agent_writes() {
    let h = Harness::start();
    let theirs = dyno_agent::discovery::Discovery::read(&h.dirs.discovery()).unwrap();
    let ours = dyno_cli::connection::Discovery::read(&h.env).unwrap();
    assert_eq!((ours.endpoint, ours.token, ours.pid), (theirs.endpoint, theirs.token, theirs.pid));
    assert_eq!(ours.protocol, dyno_core::protocol::PROTOCOL_VERSION);
}

#[test]
fn status_top_why_slow_alarms_and_snapshot_print_text_and_stable_json() {
    let h = Harness::start();
    let (code, text, _) = h.dyno("status");
    assert_eq!(code, 0);
    assert!(text.contains("CPU") && text.contains("Memory"));
    let (_, json, _) = h.dyno("status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["agent"]["pid"].as_u64().unwrap() > 0);
    assert!(v["cpuPercent"].is_number() && v["memoryTotalBytes"].as_u64().unwrap() > 0);

    let (code, text, _) = h.dyno("top -n 3");
    assert_eq!(code, 0);
    assert!(text.to_lowercase().contains("busy"), "{text}");
    let (_, json, _) = h.dyno("top --json --by memory");
    assert!(serde_json::from_str::<serde_json::Value>(&json).unwrap().is_array());

    let (code, text, _) = h.dyno("why-slow");
    assert_eq!(code, 0);
    assert!(!text.trim().is_empty());
    let (_, json, _) = h.dyno("why-slow --json");
    assert!(serde_json::from_str::<serde_json::Value>(&json).unwrap()["headline"].is_string());

    let (code, text, _) = h.dyno("alarms");
    assert_eq!(code, 0);
    assert!(text.contains("No alarms") || text.contains('#'));
    let (_, json, _) = h.dyno("alarms --all --json");
    assert!(serde_json::from_str::<serde_json::Value>(&json).unwrap().is_array());

    let (code, json, _) = h.dyno("snapshot");
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v["system"]["cpu"].is_object());
    assert!(v["processes"].as_array().unwrap().iter().any(|p| p["name"] == "busy"));
}

#[test]
fn counters_list_search_and_get_work_against_this_machines_counters() {
    let h = Harness::start();
    // Listing every counter can take longer than the agent waits on a busy machine (Windows CI run 7166); it then says to
    // try again, and the retry joins the listing still running. Run again as the message says (a bounded hang guard).
    let (code, text, err) = (0..5)
        .map(|_| h.dyno("counters list"))
        .find(|(code, _, err)| *code == 0 || !err.contains("took too long"))
        .expect("the counters were listed");
    assert_eq!(code, 0, "{err}");
    assert!(text.contains("Memory"));
    let (code, text, _) = h.dyno("counters list Memory");
    assert_eq!(code, 0);
    let first = text
        .lines()
        .find(|l| l.trim_start().starts_with("\\Memory\\"))
        .unwrap()
        .trim()
        .split("  ")
        .next()
        .unwrap()
        .to_string();
    let (code, text, _) = h.dyno(&format!("counters search {}", first.trim_start_matches("\\Memory\\")));
    assert_eq!(code, 0);
    assert!(text.contains(&first), "{text}");
    let (code, json, _) = dyno_argv(&h.env, ["counters", "get", "--json", first.as_str()].map(String::from).to_vec());
    assert_eq!(code, 0);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&json).unwrap()[0]["path"], first.as_str());
    let (code, _, err) = h.dyno("counters get nonsense");
    assert_eq!(code, 1);
    assert!(err.contains("nonsense"), "{err}");
    let (code, _, err) = h.dyno("counters list No Such Object");
    assert_eq!(code, 1);
    assert!(err.contains("no counter object"));
}

#[test]
fn an_agent_of_another_protocol_is_refused_with_a_message_and_no_request_is_sent() {
    let h = Harness::start();
    let path = h.dirs.discovery();
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    v["protocol"] = 99.into();
    std::fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
    let (code, out, err) = h.dyno("status");
    assert_eq!(code, 3);
    assert!(out.is_empty());
    assert!(err.contains("newer protocol") && err.contains("Update the dyno command"), "{err}");
    let (code, _, _) = h.dyno("top");
    assert_eq!(code, 3);
}

#[test]
fn without_a_running_agent_it_says_so_and_a_damaged_file_does_too() {
    let tmp = paddock_testkit::private_tempdir().unwrap();
    let env = Env {
        data_dir: tmp.path().to_path_buf(),
        supplied: false,
    };
    let (code, _, err) = dyno_in(&env, "status");
    assert_eq!(code, 4);
    assert!(err.contains("not running"));
    std::fs::write(env.discovery(), "garbage").unwrap();
    assert_eq!(dyno_in(&env, "top").0, 4);
    // A stale file naming an agent that is gone is a failure with advice, not a hang.
    // Built as JSON so a Windows path's backslashes are escaped.
    let stale = serde_json::json!({
        "endpoint": tmp.path().join("gone.sock").to_string_lossy(),
        "token": "ab".repeat(32),
        "pid": 1,
        "started": 1,
        "version": "0.0.1",
        "protocol": dyno_core::protocol::PROTOCOL_VERSION,
    });
    std::fs::write(env.discovery(), serde_json::to_vec(&stale).unwrap()).unwrap();
    let (code, _, err) = dyno_in(&env, "status");
    assert_eq!(code, 1);
    assert!(err.contains("did not answer"), "{err}");
}

#[test]
fn usage_help_and_version_work_without_an_agent() {
    let tmp = paddock_testkit::private_tempdir().unwrap();
    let env = Env {
        data_dir: tmp.path().to_path_buf(),
        supplied: false,
    };
    let (code, out, _) = dyno_in(&env, "--help");
    assert_eq!(code, 0);
    assert!(out.contains("These commands act:") && out.contains("`idle cancel` and `idle snooze`"));
    assert!(dyno_in(&env, "--version").1.starts_with("dyno "));
    let (code, _, err) = dyno_in(&env, "frobnicate");
    assert_eq!(code, 2);
    assert!(err.contains("Unknown command"));
}

#[test]
fn commands_that_used_to_change_something_are_gone() {
    let tmp = paddock_testkit::private_tempdir().unwrap();
    let env = Env {
        data_dir: tmp.path().to_path_buf(),
        supplied: false,
    };
    for cmd in ["end 100 --yes", "cap 100 25 --yes", "undo abc --yes", "status --yes"] {
        let (code, out, _) = dyno_in(&env, cmd);
        assert_eq!(code, 2, "{cmd}");
        assert!(out.is_empty());
    }
}

#[test]
fn awake_holds_shows_and_lets_go_through_the_agent_and_never_touches_this_machine() {
    // The fake platform's power port records the hold and calls no OS (ER-14).
    let h = Harness::start();
    let (code, out, _) = h.dyno("awake");
    assert_eq!((code, out.as_str()), (0, "Not keeping the computer awake.\n"));
    let (code, out, err) = h.dyno("awake on --for 30m --display");
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "Keeping the computer awake for 30 min, display kept on (fake#1).\n");
    assert!(h.dyno("awake status").1.starts_with("Keeping the computer awake for 30 min"));
    let (_, json, _) = h.dyno("awake status --json");
    assert!(json.contains("\"holding\"") && json.contains("\"osRef\": \"fake#1\""), "{json}");
    let (code, out, _) = h.dyno("awake on --until 23:59");
    assert_eq!(code, 0);
    assert!(out.starts_with("Keeping the computer awake until 23:59"), "{out}");
    let (code, out, _) = h.dyno("awake off");
    assert_eq!((code, out.as_str()), (0, "Not keeping the computer awake.\n"));
}

#[test]
fn awake_shows_the_agents_refusal_with_what_to_fix() {
    let h = Harness::start();
    let (code, out, err) = h.dyno("awake on --for 99999m");
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert!(err.contains("Keep awake for 1 minute up to 7 days."), "{err}");
}

#[test]
fn awake_while_busy_uses_the_limits_from_settings_and_a_program_needs_a_running_pid() {
    let h = Harness::start();
    let (code, out, err) = h.dyno("awake on --busy cpu,network");
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("Keeping the computer awake while the computer is busy"), "{out}");
    let (code, _, err) = h.dyno("awake on --while-pid 999999");
    assert_eq!(code, 1);
    assert!(err.contains("not running"), "{err}");
}

#[test]
fn idle_says_the_rules_are_off_and_cancel_is_harmless() {
    let h = Harness::start();
    assert_eq!(h.dyno("idle").1, "Sleep and shut down rules are off.\n");
    let (code, out, _) = h.dyno("idle cancel");
    assert_eq!((code, out.as_str()), (0, "Sleep and shut down rules are off.\n"));
    let (code, _, err) = h.dyno("idle snooze 0");
    assert_eq!(code, 2, "{err}");
}

#[test]
fn wake_schedule_lists_adds_and_cancels_through_the_agent_and_sets_nothing_real() {
    // The fake platform's wake timer records and sets nothing (ER-14).
    let h = Harness::start();
    let (code, _, err) = h.dyno("wake-schedule add Morning --at 07:30 --stay 30m");
    assert_eq!(code, 1);
    assert!(err.contains("turned off in Settings"), "{err}");
    let c = dyno_agent::cli::connect(&h.dirs).unwrap().0;
    let dyno_core::protocol::Reply::Settings(s) = dyno_agent::cli::ask(&c, dyno_core::protocol::Request::Settings).unwrap() else {
        panic!("settings")
    };
    let mut s = *s;
    s.power.wake.enabled = true;
    dyno_agent::cli::ask(&c, dyno_core::protocol::Request::SaveSettings { settings: Box::new(s) }).unwrap();
    let (code, out, err) = h.dyno("wake-schedule add Morning --at 07:30 --stay 30m --repeat weekdays");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Morning") && out.contains("07:30") && out.contains("armed"), "{out}");
    assert!(h.dyno("wake-schedule list").1.contains("Morning"));
    let (code, _, err) = h.dyno("wake-schedule add Bad --at 07:30");
    assert_eq!(code, 1);
    assert!(err.contains("what happens"), "{err}");
    let (code, out, _) = h.dyno("wake-schedule cancel morning");
    assert_eq!(code, 0);
    assert!(out.contains("No scheduled wakes."), "{out}");
    assert!(h.dyno("wake-schedule cancel Morning").2.contains("No schedule is called"));
}

#[test]
fn wake_finds_the_computer_by_name_or_address_and_says_how_it_ended_without_sending_anything_real() {
    use dyno_agent::cli::{ask, connect};
    use dyno_core::protocol::Request;
    use dyno_core::wol::{SecureOnEdit, WolDeviceInput};
    // The fake platform's sockets only record (ER-14); its fake clock makes the test wake's 120 s instant.
    let h = Harness::start();
    let c = connect(&h.dirs).unwrap().0;
    ask(
        &c,
        Request::WolSave {
            device: WolDeviceInput {
                id: None,
                name: "Media PC".into(),
                mac: "02:00:5e:10:00:01".into(),
                last_ip: Some("192.0.2.40".into()),
                interface: None,
                secure_on: SecureOnEdit::Keep,
            },
        },
    )
    .unwrap();
    let (code, out, err) = h.dyno("wake media pc");
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("Wake packets sent."), "{out}");
    let (code, out, _) = h.dyno("wake 02:00:5e:10:00:01 --test");
    assert_eq!(code, 0);
    assert!(out.starts_with("No answer in 120 s"), "{out}");
    let (code, _, err) = h.dyno("wake Printer");
    assert_eq!(code, 1);
    assert!(err.contains("No saved computer is called \"Printer\""), "{err}");
}
