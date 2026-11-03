//! A scripted run of the disk-health tool against a REAL running `dyno-agent` inside a disposable VM (ER-14): it asks for the offer,
//! starts the pinned download and install as a job, waits for it, reads the SSD wear answer, removes the own copy and asks again.
//! One JSON line per step. Never run it on a developer machine: it downloads the pinned file and writes into the user's profile.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout, clippy::print_stderr)]

use dyno_agent::cli::{ask, ask_within, connect};
use dyno_agent::paths::Dirs;
use dyno_core::helper_requests::ToolKind;
use dyno_core::jobs::JobSpec;
use dyno_core::protocol::{Reply, Request};
use std::time::{Duration, Instant};

fn show(step: &str, r: &Result<Reply, String>) {
    match r {
        Ok(reply) => println!("{}", serde_json::json!({ "step": step, "reply": reply })),
        Err(e) => println!("{}", serde_json::json!({ "step": step, "error": e })),
    }
}

fn main() {
    if std::env::var_os("DYNO_VM_RUN").is_none() {
        eprintln!("refusing: set DYNO_VM_RUN=1 inside a disposable VM (ER-14)");
        std::process::exit(2);
    }
    let (peer, _) = connect(&Dirs::current()).expect("agent");
    let go = |step: &str, r: Request| {
        let a = ask_within(&peer, r, Duration::from_secs(30));
        show(step, &a);
    };
    go("offer-before", Request::ToolOffer);
    go("start", Request::StartJob { spec: JobSpec::InstallTool { tool: ToolKind::Smartmontools } });
    let t = Instant::now();
    loop {
        let jobs = ask(&peer, Request::Jobs);
        let active = matches!(&jobs, Ok(Reply::Jobs(j)) if j.iter().any(|s| !s.is_terminal()));
        if !active || t.elapsed() > Duration::from_secs(300) {
            show("jobs-end", &jobs);
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    go("offer-after", Request::ToolOffer);
    // The agent re-reads the disks on its own schedule; look a few times.
    for i in 0..6 {
        go(&format!("lifespan-{i}"), Request::Lifespan);
        std::thread::sleep(Duration::from_secs(10));
    }
    go("remove", Request::RemoveTool);
    go("offer-removed", Request::ToolOffer);
}
