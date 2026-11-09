//! `pitwall-server`: the local LLM server. Runs without a window (the tray app manages it) and keeps serving when
//! the app closes. Binds 127.0.0.1 only.

use pitwall_server::app::AppState;
use pitwall_server::http;
use pitwall_server::paths::Dirs;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    if let Some(code) = pitwall_server::service::dispatch() {
        std::process::exit(code);
    }
    let mut args = std::env::args().skip(1);
    let (mut port, mut dir, mut fake, mut die_after) = (None::<u16>, None::<String>, false, None::<u64>);
    let mut ready_when = None::<std::path::PathBuf>;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => port = args.next().and_then(|p| p.parse().ok()),
            "--data-dir" => dir = args.next(),
            // Test fixture: act as an OpenAI-compatible engine with canned answers (used by the crash and restart tests).
            "--fake-engine" => fake = true,
            "--die-after-ms" => die_after = args.next().and_then(|p| p.parse().ok()),
            "--ready-when" => match pitwall_server::fake_engine::parse_ready_when(args.next()) {
                Ok(p) => ready_when = Some(p),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(2);
                }
            },
            "--host" | "--model" => {
                args.next();
            }
            "--version" => {
                println!("pitwall-server {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                println!("pitwall-server [--port N] [--data-dir PATH]\nServes OpenAI, Anthropic and Ollama compatible APIs on 127.0.0.1 (default port 11435).");
                return;
            }
            other => {
                eprintln!("unknown option {other}; try --help");
                std::process::exit(2);
            }
        }
    }
    if fake {
        if let Err(e) = pitwall_server::fake_engine::serve(port.unwrap_or(0), die_after, ready_when).await {
            eprintln!("fake engine: {e}");
            std::process::exit(1);
        }
        return;
    }
    let moved = dir.is_some() || std::env::var_os("PITWALL_HOME").is_some_and(|v| !v.is_empty());
    let dirs = dir.map_or_else(Dirs::for_user, Dirs::at);
    // First: every later line, the panic hook and the crash marker go to pitwall-server.<day>.log (ER-27).
    let _log = pitwall_server::logging::install(&dirs, moved);
    log::info!(target: "pitwall_server", "starting: version {}, pid {}, data folder moved {moved}", env!("CARGO_PKG_VERSION"), std::process::id());
    let st = AppState::open(dirs, None).await;
    let port = port.unwrap_or(st.cfg().server.port);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            log::error!(target: "pitwall_server", "cannot listen on {addr}: {e}");
            eprintln!("Pitwall cannot listen on {addr}: {e}. Is another Pitwall (or an Ollama on the same port) running? Change the port in Settings > Server or start with --port.");
            std::process::exit(1);
        }
    };
    let bound = listener.local_addr().map_or(port, |a| a.port());
    st.port.store(bound, std::sync::atomic::Ordering::SeqCst);
    st.write_discovery(bound);
    st.spawn_tasks();
    log::info!(target: "pitwall_server", "serving on port {bound}");
    eprintln!("Pitwall {} serving on http://127.0.0.1:{bound}", env!("CARGO_PKG_VERSION"));
    let app = http::router(st.clone());
    let shutdown = async {
        #[cfg(unix)]
        {
            if let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
                return;
            }
        }
        let _ = tokio::signal::ctrl_c().await;
    };
    if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(shutdown).await {
        log::error!(target: "pitwall_server", "stopped with an error: {e}");
        eprintln!("server stopped: {e}");
    } else {
        log::info!(target: "pitwall_server", "stopped: asked to end");
    }
    st.remove_discovery();
}
