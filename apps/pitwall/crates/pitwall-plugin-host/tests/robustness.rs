//! The plugin host against real plugin processes (the fake plugin, copied into a plugin folder per test): every
//! kind is served; a plugin that crashes, hangs, floods frames, sends an oversize frame or garbage, exits at start,
//! lies about itself or exceeds its memory cap fails its calls with an error attributed to it, is restarted with
//! backoff and quarantined after repeated failures, and never affects the host or other plugins; a default plugin
//! cannot open an outbound TCP connection.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use pitwall_plugin_host::{
    EndpointRequest, ErrorKind, EventKind, FilterFailure, GenerateEvent, HostConfig, PluginHost, PluginSettings, State,
};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const FAKE: &str = env!("CARGO_BIN_EXE_pitwall-fake-plugin");

/// A plugins folder and a data folder; removes the test's sandbox profiles (Windows) when dropped.
struct Lab {
    root: tempfile::TempDir,
    ids: Vec<String>,
}

impl Lab {
    fn new() -> Lab {
        Lab {
            root: tempfile::tempdir().unwrap(),
            ids: Vec::new(),
        }
    }

    fn plugins(&self) -> PathBuf {
        self.root.path().join("plugins")
    }

    /// Installs the fake plugin as `id` behaving as `mode`, with extra manifest lines.
    fn install(&mut self, id: &str, mode: &str, extra: &str) -> &mut Lab {
        let dir = self.plugins().join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(Path::new(FAKE).file_name().unwrap());
        std::fs::copy(FAKE, exe).unwrap();
        let kinds = if extra.contains("kinds") { "" } else { "kinds = ['engine', 'endpoint', 'tool', 'filter']\n" };
        std::fs::write(
            dir.join("pitwall-plugin.toml"),
            format!(
                "id = '{id}'\nname = 'Fake {id}'\nversion = '0.0.1'\ncontract = '1.0'\n{kinds}executable = 'pitwall-fake-plugin'\nargs = ['--id={id}', '--mode={mode}']\n{extra}"
            ),
        )
        .unwrap();
        self.ids.push(id.to_string());
        self
    }

    fn config(&self) -> HostConfig {
        let mut c = HostConfig::new(self.plugins(), self.root.path().join("data"));
        c.sandbox_prefix = "pitwall.test".into();
        c.policy.backoff_base = Duration::from_millis(10);
        c.policy.backoff_max = Duration::from_millis(40);
        c.timeouts.filter = Duration::from_secs(5);
        c.timeouts.tool = Duration::from_secs(10);
        c
    }

    fn host(&self, tweak: impl FnOnce(&mut HostConfig)) -> PluginHost {
        let mut c = self.config();
        tweak(&mut c);
        let host = PluginHost::new(c, PluginSettings::default());
        assert!(host.rescan().is_empty(), "{:?}", host.problems());
        host
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        #[cfg(windows)]
        for id in &self.ids {
            let _ = pitwall_ipc::contain::Container::delete(&format!("pitwall.test.{id}"));
        }
    }
}

fn state(host: &PluginHost, id: &str) -> State {
    host.plugins().into_iter().find(|p| p.id == id).unwrap().state
}

/// Waits out the restart backoff of the test policy.
async fn backoff() {
    tokio::time::sleep(Duration::from_millis(60)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_well_behaved_plugin_serves_every_kind() {
    let mut lab = Lab::new();
    lab.install("good-all", "normal", "");
    let host = lab.host(|_| {});
    let mut events = host.subscribe();
    let mut settings = PluginSettings::default();
    settings.config.insert("good-all".into(), json!({"level": 3}));
    host.apply_settings(settings);

    let filtered = host.filter_request(json!({"messages": []})).await.unwrap();
    assert_eq!(filtered["seen"], json!(["good-all"]));
    let response = host.filter_response(json!({"choices": []})).await.unwrap();
    assert_eq!(response["responseSeenBy"], "good-all");
    let r = host
        .call_endpoint(
            "good-all",
            EndpointRequest {
                method: "GET".into(),
                path: "/hello".into(),
                query: "a=1".into(),
                content_type: None,
                body: b"xyz".to_vec(),
            },
        )
        .await
        .unwrap();
    assert_eq!(r.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(body, json!({"hello": "good-all", "query": "a=1", "bytes": 3}));
    assert_eq!(host.invoke_tool("good-all", "echo", json!({"x": [1, "é"]})).await.unwrap(), json!({"x": [1, "é"]}));
    assert_eq!(host.invoke_tool("good-all", "settings", json!({})).await.unwrap(), json!({"level": 3}));
    assert_eq!(host.invoke_tool("good-all", "data", json!({})).await.unwrap(), json!("written"));
    assert!(lab.root.path().join("data").join("good-all").join("written.txt").is_file(), "its data folder is writable");
    // A typed error of the plugin is not a failure.
    let e = host.invoke_tool("good-all", "nope", json!({})).await.unwrap_err();
    assert_eq!((e.kind.clone(), e.http_status()), (ErrorKind::Plugin { code: 1404 }, 404));

    // Engine: load, stream, unload.
    let model = host.engine_load("good-all", "tiny.gguf", json!({})).await.unwrap();
    assert_eq!(model.handle, "loaded:tiny.gguf");
    let mut stream = host.engine_generate(&model, json!({"tokens": 20})).unwrap();
    let mut text = String::new();
    let summary = loop {
        match stream.next().await.unwrap().unwrap() {
            GenerateEvent::Text(t) => text.push_str(&t),
            GenerateEvent::Done(s) => break s,
        }
    };
    assert!(text.starts_with("t0 t1 ") && text.ends_with("t19 "), "{text}");
    assert_eq!(summary["tokens"], 20);
    assert!(stream.next().await.is_none());
    host.engine_unload(&model).await.unwrap();

    let status = host.plugins().pop().unwrap();
    assert_eq!(status.state, State::Running);
    assert!(status.pid.is_some() && status.failures == 0 && !status.network_granted);
    let offers = status.offers.unwrap();
    assert_eq!(offers.endpoints[0].path, "/hello");
    assert!(offers.tools.iter().any(|t| t.name == "connect"));
    assert!(status.sandbox.iter().any(|n| n.message.contains("network blocked")), "{:?}", status.sandbox);
    assert!(matches!(events.recv().await.unwrap().kind, EventKind::Started { .. }));
    // Changing its settings restarts it with them.
    let mut settings = PluginSettings::default();
    settings.config.insert("good-all".into(), json!({"level": 4}));
    host.apply_settings(settings);
    assert_eq!(host.invoke_tool("good-all", "settings", json!({})).await.unwrap(), json!({"level": 4}));
    host.stop_all();
    assert_eq!(state(&host, "good-all"), State::Stopped);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crashing_filter_is_attributed_restarted_and_quarantined_without_hurting_others() {
    let mut lab = Lab::new();
    lab.install("a-good-filter", "normal", "").install("crashy", "crash-on-call", "");
    // Fail closed (the default): the request is refused, naming the plugin.
    let host = lab.host(|_| {});
    let mut events = host.subscribe();
    let r = host.filter_request(json!({})).await.unwrap_err();
    assert_eq!((r.plugin.as_str(), r.status), ("crashy", 503));
    assert!(r.message.contains("plugin \"crashy\" crashed"), "{}", r.message);
    backoff().await;
    let _ = host.filter_request(json!({})).await;
    backoff().await;
    let _ = host.filter_request(json!({})).await;
    assert_eq!(state(&host, "crashy"), State::Quarantined);
    let r = host.filter_request(json!({})).await.unwrap_err();
    assert!(r.message.contains("failed 3 times"), "{}", r.message);
    // The other plugin and the host are fine.
    assert_eq!(host.invoke_tool("a-good-filter", "echo", json!(1)).await.unwrap(), json!(1));
    assert_eq!(state(&host, "a-good-filter"), State::Running);
    let mut kinds = Vec::new();
    while let Ok(e) = events.try_recv() {
        if e.plugin == "crashy" {
            kinds.push(e.kind);
        }
    }
    assert_eq!(kinds.iter().filter(|k| matches!(k, EventKind::Failed { .. })).count(), 3, "{kinds:?}");
    assert!(kinds.iter().any(|k| matches!(k, EventKind::Quarantined { .. })));
    // Reset lifts the quarantine.
    host.reset("crashy").unwrap();
    assert_eq!(state(&host, "crashy"), State::Stopped);

    // Fail open: the failing filter is skipped and the good one still runs.
    let host = lab.host(|c| c.filter_failure = FilterFailure::Skip);
    let body = host.filter_request(json!({})).await.unwrap();
    assert_eq!(body["seen"], json!(["a-good-filter"]));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hanging_plugin_times_out_and_is_stopped() {
    let mut lab = Lab::new();
    lab.install("hangs", "hang", "");
    let host = lab.host(|c| c.timeouts.filter = Duration::from_millis(300));
    let started = Instant::now();
    let r = host.filter_request(json!({})).await.unwrap_err();
    assert!(r.message.contains("did not answer in time"), "{}", r.message);
    assert!(started.elapsed() < Duration::from_secs(8), "{:?}", started.elapsed());
    let status = host.plugins().pop().unwrap();
    assert_eq!((status.state, status.failures), (State::Stopped, 1));
    // A slow tool past its deadline is a timeout too.
    let host = lab.host(|c| c.timeouts.tool = Duration::from_millis(200));
    let e = host.invoke_tool("hangs", "sleep", json!({"ms": 5000})).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Timeout);
    assert_eq!(e.http_status(), 504);
}

#[tokio::test(flavor = "multi_thread")]
async fn protocol_violations_are_failures_attributed_to_the_plugin() {
    let mut lab = Lab::new();
    lab.install("floods", "flood", "")
        .install("oversized", "oversize", "")
        .install("garbled", "garbage", "")
        .install("liar", "wrong-id", "");
    let host = lab.host(|_| {});
    for id in ["floods", "oversized", "garbled", "liar"] {
        let e = host.invoke_tool(id, "echo", json!(1)).await.unwrap_err();
        assert_eq!((e.plugin.as_str(), &e.kind), (id, &ErrorKind::Protocol), "{id}: {e:?}");
        assert!(e.message.contains(&format!("plugin \"{id}\"")), "{e:?}");
    }
    let e = host.invoke_tool("liar", "echo", json!(1)).await;
    assert!(e.is_err());
    assert!(host.plugins().iter().all(|p| p.failures >= 1 && p.state != State::Running));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_that_exits_at_start_or_panics_is_a_crash() {
    let mut lab = Lab::new();
    lab.install("quits", "exit-at-start", "").install("panics", "normal", "");
    let host = lab.host(|_| {});
    let e = host.invoke_tool("quits", "echo", json!(1)).await.unwrap_err();
    assert!(matches!(e.kind, ErrorKind::Crashed), "{e:?}");
    assert!(e.message.contains("quits"), "{e:?}");
    // A panic in a handler is contained in the plugin, reported as a crash, and the plugin restarted.
    let e = host.invoke_tool("panics", "panic", json!({})).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Crashed);
    assert!(e.message.contains("panicked"), "{e:?}");
    backoff().await;
    assert_eq!(host.invoke_tool("panics", "echo", json!(2)).await.unwrap(), json!(2));
    let status = host.plugins().into_iter().find(|p| p.id == "panics").unwrap();
    assert_eq!((status.failures, status.restarts), (1, 1));
    // A process exit mid-call too.
    let e = host.invoke_tool("panics", "exit", json!({})).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Crashed);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_over_its_memory_cap_is_stopped_and_says_so() {
    let mut lab = Lab::new();
    lab.install("greedy", "normal", "memory_mb = 64\n");
    let host = lab.host(|_| {});
    assert_eq!(host.invoke_tool("greedy", "alloc", json!({"mb": 8})).await.unwrap(), json!({"held_mb": 8}));
    let e = host.invoke_tool("greedy", "alloc", json!({"mb": 512, "hold_ms": 3000})).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::MemoryCap, "{e:?}");
    assert!(e.message.contains("memory cap of 64 MB"), "{e:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn host_side_refusals_are_not_failures() {
    let mut lab = Lab::new();
    lab.install("only-filter", "normal", "kinds = ['filter']\n").install("off", "normal", "");
    let host = lab.host(|c| c.max_frame = 64 * 1024);
    let e = host.invoke_tool("missing", "echo", json!(1)).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::NotFound);
    let e = host.invoke_tool("only-filter", "echo", json!(1)).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Unsupported);
    let mut settings = PluginSettings::default();
    settings.disabled.insert("off".into());
    settings.disabled.insert("only-filter".into());
    host.apply_settings(settings);
    let e = host.invoke_tool("off", "echo", json!(1)).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Disabled);
    assert_eq!(state(&host, "off"), State::Disabled);
    // Disabled filters are not in the chain.
    assert_eq!(host.filter_request(json!({"a": 1})).await.unwrap(), json!({"a": 1}));
    host.apply_settings(PluginSettings::default());
    let e = host.invoke_tool("off", "echo", json!("x".repeat(100_000))).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::TooLarge);
    assert!(host.plugins().iter().all(|p| p.failures == 0));
    // A removed plugin is gone after a rescan; a broken one is a problem.
    std::fs::remove_dir_all(lab.plugins().join("off")).unwrap();
    std::fs::create_dir_all(lab.plugins().join("broken")).unwrap();
    let problems = host.rescan();
    assert_eq!(problems.len(), 1);
    assert_eq!(host.plugins().len(), 1);
    assert_eq!(host.invoke_tool("off", "echo", json!(1)).await.unwrap_err().kind, ErrorKind::NotFound);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_engine_restart_makes_model_handles_stale_and_dropping_a_stream_cancels() {
    let mut lab = Lab::new();
    lab.install("engine", "normal", "");
    let host = lab.host(|_| {});
    let model = host.engine_load("engine", "m", json!({})).await.unwrap();
    let mut stream = host.engine_generate(&model, json!({"tokens": 1000, "delay_ms": 5})).unwrap();
    assert!(matches!(stream.next().await, Some(Ok(GenerateEvent::Text(_)))));
    drop(stream);
    // The plugin is still healthy after a cancelled generation.
    assert_eq!(host.invoke_tool("engine", "echo", json!(3)).await.unwrap(), json!(3));
    let _ = host.invoke_tool("engine", "exit", json!({})).await;
    backoff().await;
    let e = host.engine_unload(&model).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::StaleHandle);
    let mut stream = host.engine_generate(&model, json!({})).unwrap();
    assert_eq!(stream.next().await.unwrap().unwrap_err().kind, ErrorKind::StaleHandle);
}

/// Privacy: a plugin gets no network unless its manifest asks for it and the user granted it. Enforced on Windows
/// (AppContainer without network capabilities), Linux (seccomp, plus a network namespace where allowed) and macOS
/// (sandbox-exec profile).
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
#[tokio::test(flavor = "multi_thread")]
async fn a_default_plugin_cannot_open_an_outbound_tcp_connection() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    // Control: the address is reachable from outside the sandbox.
    assert!(std::net::TcpStream::connect(&addr).is_ok());
    let mut lab = Lab::new();
    lab.install("offline", "normal", "").install("asks-network", "normal", "[permissions]\nnetwork = true\n");
    let host = lab.host(|_| {});
    for id in ["offline", "asks-network"] {
        let r = host.invoke_tool(id, "connect", json!({ "addr": addr })).await.unwrap();
        assert_eq!(r["connected"], false, "{id} connected without a network grant: {r}");
        eprintln!("{id}: {r}");
    }
    // A grant without the manifest asking for it changes nothing.
    let mut settings = PluginSettings::default();
    settings.network.insert("offline".into());
    host.apply_settings(settings.clone());
    let r = host.invoke_tool("offline", "connect", json!({ "addr": addr })).await.unwrap();
    assert_eq!(r["connected"], false, "{r}");
    // Asked for and granted: allowed (Windows keeps loopback blocked for AppContainers, so this is checked elsewhere).
    settings.network.insert("asks-network".into());
    host.apply_settings(settings);
    let status = host.plugins().into_iter().find(|p| p.id == "asks-network").unwrap();
    assert!(status.network_granted);
    #[cfg(not(windows))]
    {
        let r = host.invoke_tool("asks-network", "connect", json!({ "addr": addr })).await.unwrap();
        assert_eq!(r["connected"], true, "{r}");
    }
}
