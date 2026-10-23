#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The `dyno` command against a real agent over the fake platform: a real IPC connection and discovery file in a
//! temp folder. Nothing here touches the machine: the processes acted on are the fake platform's.

use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_cli::{run, Env};
use dyno_core::process::ProcessAction;
use dyno_platform::fake::{process, Fake};

struct Harness {
    running: Option<Running>,
    fake: Fake,
    dirs: Dirs,
    env: Env,
    _tmp: tempfile::TempDir,
}

impl Harness {
    fn start() -> Harness {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        for (pid, name, cpu) in [(100, "busy", 60.0), (101, "calm", 1.0), (102, "launchd", 0.1)] {
            fake.add_process(process(pid, name, cpu));
        }
        let running = Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
        let env = Env {
            data_dir: dirs.root().to_path_buf(),
        };
        Harness {
            running: Some(running),
            fake,
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
    let args: Vec<String> = args.split_whitespace().map(String::from).collect();
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
    let (code, text, err) = h.dyno("counters list");
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
    let (code, json, _) = h.dyno(&format!("counters get --json {first}"));
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
    let (code, _, _) = h.dyno("end 100 --yes");
    assert_eq!(code, 3);
    assert!(h.fake.actions().is_empty());
}

#[test]
fn without_a_running_agent_it_says_so_and_a_damaged_file_does_too() {
    let tmp = tempfile::tempdir().unwrap();
    let env = Env {
        data_dir: tmp.path().to_path_buf(),
    };
    let (code, _, err) = dyno_in(&env, "status");
    assert_eq!(code, 4);
    assert!(err.contains("not running"));
    std::fs::write(env.discovery(), "garbage").unwrap();
    assert_eq!(dyno_in(&env, "top").0, 4);
    // A stale file naming an agent that is gone is a failure with advice, not a hang.
    std::fs::write(
        env.discovery(),
        format!(
            r#"{{"endpoint":"{}","token":"{}","pid":1,"started":1,"version":"0.0.1","protocol":{}}}"#,
            tmp.path().join("gone.sock").display(),
            "ab".repeat(32),
            dyno_core::protocol::PROTOCOL_VERSION
        ),
    )
    .unwrap();
    let (code, _, err) = dyno_in(&env, "status");
    assert_eq!(code, 1);
    assert!(err.contains("did not answer"), "{err}");
}

#[test]
fn usage_help_and_version_work_without_an_agent() {
    let tmp = tempfile::tempdir().unwrap();
    let env = Env {
        data_dir: tmp.path().to_path_buf(),
    };
    let (code, out, _) = dyno_in(&env, "--help");
    assert_eq!(code, 0);
    assert!(out.contains("nothing happens without --yes"));
    assert!(dyno_in(&env, "--version").1.starts_with("dyno "));
    let (code, _, err) = dyno_in(&env, "frobnicate");
    assert_eq!(code, 2);
    assert!(err.contains("Unknown command"));
}

#[test]
fn acting_commands_do_nothing_without_yes_even_with_no_agent_running() {
    let tmp = tempfile::tempdir().unwrap();
    let env = Env {
        data_dir: tmp.path().to_path_buf(),
    };
    for cmd in ["end 100", "cap 100 25", "undo abc"] {
        let (code, out, err) = dyno_in(&env, cmd);
        assert_eq!(code, 2, "{cmd}");
        assert!(out.is_empty());
        assert!(err.contains("Nothing was changed") && err.contains("--yes"), "{err}");
    }
    let h = Harness::start();
    let (code, _, err) = h.dyno("end 100");
    assert_eq!(code, 2);
    assert!(err.contains("process id 100"));
    assert!(h.fake.actions().is_empty());
}

#[test]
fn end_and_cap_go_through_the_agents_rules_and_cap_can_be_undone() {
    let h = Harness::start();
    // The protected list applies exactly as for a click.
    let (code, _, err) = h.dyno("end 102 --yes");
    assert_eq!(code, 1);
    assert!(!err.is_empty());
    assert!(h.fake.actions().is_empty(), "a protected process is never touched");
    // A process that is not there.
    let (code, _, err) = h.dyno("end 9999 --yes");
    assert_eq!(code, 1);
    assert!(err.contains("No process with id 9999"));
    // A cap, then its undo through the activity log.
    let (code, out, err) = h.dyno("cap 100 25 --yes");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Capped") && out.contains("dyno undo"), "{out}");
    assert_eq!(h.fake.actions().last().unwrap().1, ProcessAction::SetCpuCap(Some(25)));
    let id = out
        .split("dyno undo ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let (code, out, err) = h.dyno(&format!("undo {id} --yes"));
    assert_eq!(code, 0, "{err}");
    assert!(!out.is_empty());
    assert_eq!(h.fake.actions().last().unwrap().1, ProcessAction::SetCpuCap(None));
    let (code, _, err) = h.dyno("undo nosuchid --yes");
    assert_eq!(code, 1);
    assert!(!err.is_empty());
    // End.
    let (code, out, _) = h.dyno("end 101 --yes");
    assert_eq!(code, 0);
    assert!(out.contains("Ended"), "{out}");
    assert_eq!(h.fake.actions().last().unwrap().1, ProcessAction::End);
}
