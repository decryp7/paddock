//! A fake plugin for the host's robustness tests (never bundled). `--id=<id>` names it (its manifest is built from
//! the arguments, so one binary plays every plugin) and `--mode=<mode>` picks a behaviour:
//!
//! - `normal` (default): a well-behaved filter, endpoint, tool and engine plugin. Tools: `echo`, `connect`
//!   (`{"addr": "ip:port"}`: tries an outbound TCP connection), `alloc` (`{"mb": n, "hold_ms": t}`: allocates and
//!   touches n MiB in 1 MiB pieces), `sleep` (`{"ms": n}`), `panic`, `exit`, `settings` (what Configure sent),
//!   `data` (writes a file into its data folder).
//! - `exit-at-start`: exits before connecting; `crash-on-call`: exits in the middle of any filter call;
//!   `hang`: never answers a filter call; `reject`: rejects every request with 451; `wrong-id`: describes itself
//!   with another id; `flood`: after the handshake sends frames for calls that do not exist, without end;
//!   `oversize`: after the handshake announces a 1 GiB frame; `garbage`: sends bytes that are not a frame.

use pitwall_plugin_api::prelude::*;
use std::sync::Mutex;
use std::time::Duration;

fn arg(name: &str) -> Option<String> {
    std::env::args().find_map(|a| a.strip_prefix(&format!("--{name}=")).map(String::from))
}

struct Fake {
    mode: String,
    id: String,
    settings: std::sync::Arc<Mutex<Value>>,
}

impl Filter for Fake {
    fn filter_request(&self, mut body: Value) -> Result<FilterVerdict, PluginError> {
        match self.mode.as_str() {
            "crash-on-call" => std::process::exit(101),
            "hang" => loop {
                std::thread::sleep(Duration::from_secs(3600));
            },
            "reject" => Ok(FilterVerdict::reject(451, format!("{} says no", self.id))),
            _ => {
                if let Some(seen) = body.get_mut("seen").and_then(Value::as_array_mut) {
                    seen.push(json!(self.id));
                } else if let Some(o) = body.as_object_mut() {
                    o.insert("seen".into(), json!([self.id]));
                }
                Ok(FilterVerdict::forward(body))
            }
        }
    }

    fn filter_response(&self, mut body: Value) -> Result<Value, PluginError> {
        if let Some(o) = body.as_object_mut() {
            o.insert("responseSeenBy".into(), json!(self.id));
        }
        Ok(body)
    }
}

impl Endpoints for Fake {
    fn endpoints(&self) -> Vec<EndpointInfo> {
        vec![EndpointInfo {
            method: "GET".into(),
            path: "/hello".into(),
            description: "says hello".into(),
        }]
    }

    fn handle(&self, r: EndpointRequest) -> Result<EndpointResponse, PluginError> {
        match r.path.as_str() {
            "/hello" => Ok(EndpointResponse::json(200, &json!({"hello": self.id, "query": r.query, "bytes": r.body.len()}))),
            _ => Ok(EndpointResponse::text(404, "no such endpoint")),
        }
    }
}

impl Tools for Fake {
    fn tools(&self) -> Vec<ToolInfo> {
        ["echo", "connect", "alloc", "sleep", "panic", "exit", "settings", "data"]
            .iter()
            .map(|n| ToolInfo {
                name: (*n).into(),
                description: format!("test tool {n}"),
                parameters: json!({"type": "object"}).into(),
            })
            .collect()
    }

    fn invoke(&self, tool: &str, args: Value) -> Result<Value, PluginError> {
        match tool {
            "echo" => Ok(args),
            "connect" => {
                let addr: std::net::SocketAddr = args["addr"]
                    .as_str()
                    .unwrap_or("")
                    .parse()
                    .map_err(|e| PluginError::invalid(format!("bad address: {e}")))?;
                Ok(match std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
                    Ok(_) => json!({"connected": true}),
                    Err(e) => json!({"connected": false, "error": e.to_string()}),
                })
            }
            "alloc" => {
                let mb = args["mb"].as_u64().unwrap_or(0);
                let mut held: Vec<Vec<u8>> = Vec::new();
                for _ in 0..mb {
                    held.push(vec![1u8; 1 << 20]);
                }
                std::thread::sleep(Duration::from_millis(args["hold_ms"].as_u64().unwrap_or(0)));
                Ok(json!({"held_mb": held.len()}))
            }
            "sleep" => {
                std::thread::sleep(Duration::from_millis(args["ms"].as_u64().unwrap_or(0)));
                Ok(json!("slept"))
            }
            "panic" => panic!("the fake plugin panicked on purpose"),
            "exit" => std::process::exit(7),
            "settings" => Ok(self.settings.lock().map(|s| s.clone()).unwrap_or(Value::Null)),
            "data" => {
                let dir = PluginContext::from_env().data_dir.ok_or_else(|| PluginError::failed("no data folder"))?;
                std::fs::write(dir.join("written.txt"), b"ok").map_err(|e| PluginError::failed(e.to_string()))?;
                Ok(json!("written"))
            }
            other => Err(PluginError::not_found(format!("no tool {other}"))),
        }
    }
}

impl Engine for Fake {
    fn load(&self, model: &str, _options: Value) -> Result<String, PluginError> {
        Ok(format!("loaded:{model}"))
    }

    fn unload(&self, _handle: &str) -> Result<(), PluginError> {
        Ok(())
    }

    fn generate(&self, handle: &str, request: Value, out: &mut dyn TokenSink) -> Result<Value, PluginError> {
        let n = request["tokens"].as_u64().unwrap_or(5);
        let delay = request["delay_ms"].as_u64().unwrap_or(0);
        for i in 0..n {
            if out.cancelled() {
                break;
            }
            std::thread::sleep(Duration::from_millis(delay));
            out.text(&format!("t{i} "))?;
        }
        Ok(json!({"finish": "stop", "tokens": n, "handle": handle}))
    }
}

/// Misbehaviours that need the raw connection: handshake, then send bytes that break the protocol.
fn misbehave(mode: &str) -> std::process::ExitCode {
    let Ok(bootstrap) = pitwall_ipc::child::read_bootstrap(std::io::stdin().lock()) else {
        return std::process::ExitCode::from(1);
    };
    let Ok(mut io) = pitwall_ipc::child::connect(&bootstrap, "Pitwall", pitwall_plugin_api::CONTRACT_VERSION) else {
        return std::process::ExitCode::from(1);
    };
    pitwall_ipc::child::lifeline();
    let mode = mode.to_string();
    let _ = pitwall_ipc::runtime::run(async move {
        use tokio::io::AsyncWriteExt;
        match mode.as_str() {
            "oversize" => {
                let _ = io.write_all(&(1u32 << 30).to_le_bytes()).await;
            }
            "garbage" => {
                let _ = io.write_all(&[5, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0xff]).await;
            }
            _ => {
                // flood: stray replies as fast as the host reads them.
                let mut id = 1_000_000u64;
                loop {
                    let f = pitwall_ipc::frame::Frame::Reply { id, body: vec![0; 64] };
                    let Ok(bytes) = pitwall_ipc::frame::encode(&f, pitwall_ipc::frame::DEFAULT_MAX_FRAME) else { break };
                    if io.write_all(&bytes).await.is_err() {
                        break;
                    }
                    id += 1;
                }
            }
        }
        let _ = io.flush().await;
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    std::process::ExitCode::SUCCESS
}

fn main() -> std::process::ExitCode {
    let mode = arg("mode").unwrap_or_else(|| "normal".into());
    let id = arg("id").unwrap_or_else(|| "fake".into());
    match mode.as_str() {
        "exit-at-start" => return std::process::ExitCode::from(3),
        "flood" | "oversize" | "garbage" => return misbehave(&mode),
        _ => {}
    }
    let described = if mode == "wrong-id" { format!("{id}-impostor") } else { id.clone() };
    let manifest = format!(
        "id = '{described}'\nname = 'Fake'\nversion = '0.0.1'\ncontract = '1.0'\nkinds = ['engine', 'endpoint', 'tool', 'filter']\nexecutable = 'pitwall-fake-plugin'\n"
    );
    let settings = std::sync::Arc::new(Mutex::new(Value::Null));
    let fake = || Fake {
        mode: mode.clone(),
        id: id.clone(),
        settings: settings.clone(),
    };
    let store = settings.clone();
    serve(
        Plugin::new(&manifest)
            .filter(fake())
            .endpoints(fake())
            .tools(fake())
            .engine(fake())
            .configure(move |s, _| {
                if let Ok(mut g) = store.lock() {
                    *g = s;
                }
                Ok(())
            }),
    )
}
