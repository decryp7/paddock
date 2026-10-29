//! Golden files of every rendered OS file (`tests/golden/`), for the family's real services. A change to a renderer
//! shows up as a diff in review; `PADDOCK_WRITE_TS=1 cargo test -p paddock-service golden` rewrites them.

use crate::render::{
    LaunchdProgram, launchd_plist, nsis_hooks, sc_script, scm_config, systemd_unit,
    windows_command_line,
};
use crate::spec::{Kind, ProcessType, Restart, ServiceSpec};
use paddock_commands::testing::{WriteMode, sync_generated};
use std::path::PathBuf;

fn pitwall() -> ServiceSpec {
    ServiceSpec::builder(
        "net.decryptology.pitwall.service",
        "Pitwall",
        "pitwall-server",
    )
    .bundle_id("net.decryptology.pitwall")
    .display_name("Pitwall local LLM server")
    .description("Serves local language models to the apps on this PC (127.0.0.1 only).")
    .supervised_env("PITWALL_SERVICE")
    .process_type(ProcessType::Interactive)
    .docs_url("https://dev.decryptology.net/decryp7/pitwall")
    .build()
    .unwrap()
}

fn pitwall_windows() -> ServiceSpec {
    ServiceSpec::builder(
        "net.decryptology.pitwall.service",
        "Pitwall",
        "pitwall-server",
    )
    .display_name("Pitwall local LLM server")
    .description("Serves local language models to the apps on this PC (127.0.0.1 only).")
    .kind(Kind::SystemService)
    .build()
    .unwrap()
}

fn pitcrew() -> ServiceSpec {
    ServiceSpec::builder(
        "net.decryptology.pitcrew.service",
        "Pitcrew",
        "pitcrew-service",
    )
    .bundle_id("net.decryptology.pitcrew")
    .args(["run"])
    .build()
    .unwrap()
}

fn dyno() -> ServiceSpec {
    ServiceSpec::builder("net.decryptology.dyno.agent", "Dyno", "dyno-agent")
        .unit("dyno-agent")
        .bundle_id("net.decryptology.dyno")
        .display_name("Dyno agent (measures and keeps your PC's history)")
        .args(["run"])
        .process_type(ProcessType::Standard)
        .low_priority()
        .memory_high_mb(256)
        .restart(Restart::Always)
        .throttle_secs(5)
        .docs_url("https://dev.decryptology.net/decryp7/dyno")
        .build()
        .unwrap()
}

fn debrief_agent() -> ServiceSpec {
    ServiceSpec::builder(
        "net.decryptology.debrief-agent",
        "debrief-agent",
        "debrief-agent",
    )
    .display_name("Debrief Agent")
    .description("Debrief agent: scheduled and triggered diagnostics collection (local files only)")
    .args(["run", "--config", "/etc/debrief-agent.toml"])
    .kind(Kind::SystemService)
    .restart(Restart::OnFailure)
    .process_type(ProcessType::Background)
    .build()
    .unwrap()
}

fn check(name: &str, text: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if let Err(e) = sync_generated(&path, text, WriteMode::from_env()) {
        panic!("{e}\n--- generated ---\n{text}");
    }
}

#[test]
fn golden_bundled_launch_agents() {
    for (name, spec) in [
        ("pitwall", pitwall()),
        ("pitcrew", pitcrew()),
        ("dyno", dyno()),
    ] {
        let text = launchd_plist(&spec, LaunchdProgram::Bundled);
        plist::Value::from_reader_xml(text.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
        check(&format!("{}.plist", spec.label()), &text);
        assert!(
            text.contains(&format!(
                "<string>Contents/MacOS/{}</string>",
                spec.program()
            )),
            "{name}"
        );
    }
}

#[test]
fn golden_launchd_files() {
    let text = launchd_plist(
        &debrief_agent(),
        LaunchdProgram::File {
            exe: "/usr/local/bin/debrief-agent",
            working_dir: None,
            log: Some("/var/log/debrief-agent/launchd.log"),
        },
    );
    plist::Value::from_reader_xml(text.as_bytes()).unwrap();
    check("debrief-agent.daemon.plist", &text);
    let text = launchd_plist(
        &pitcrew(),
        LaunchdProgram::File {
            exe: "/Users/ana/dev/pitcrew/target/debug/pitcrew-service",
            working_dir: Some("/Users/ana/Library/Application Support/Pitcrew"),
            log: Some("/Users/ana/Library/Logs/Pitcrew/service.log"),
        },
    );
    plist::Value::from_reader_xml(text.as_bytes()).unwrap();
    check("pitcrew.agent-file.plist", &text);
}

#[test]
fn golden_systemd_units() {
    check(
        "pitwall.service",
        &systemd_unit(
            &pitwall(),
            "/home/ana/.local/share/pitwall/bin/pitwall-server",
            Some("/home/ana/.local/share/pitwall"),
        ),
    );
    check(
        "dyno-agent.service",
        &systemd_unit(&dyno(), "/opt/Dyno App/dyno-agent", None),
    );
    check(
        "debrief-agent.service",
        &systemd_unit(&debrief_agent(), "/usr/bin/debrief-agent", None),
    );
}

#[test]
fn golden_windows() {
    check(
        "dyno.run-value.txt",
        &format!(
            "{}\n",
            windows_command_line(
                "C:\\Users\\Ana\\AppData\\Local\\Dyno\\dyno-agent.exe",
                dyno().args()
            )
        ),
    );
    check(
        "pitwall.sc.cmd",
        &sc_script(&scm_config(
            &pitwall_windows(),
            "C:\\Program Files\\Pitwall\\pitwall-server.exe",
        )),
    );
    check(
        "pitwall.installer-hooks.nsh",
        &nsis_hooks(&pitwall_windows()),
    );
    check("dyno.installer-hooks.nsh", &nsis_hooks(&dyno()));
}
