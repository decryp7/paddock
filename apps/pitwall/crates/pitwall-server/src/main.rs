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
    let (mut port, mut dir, mut fake) = (None::<u16>, None::<String>, false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => port = args.next().and_then(|p| p.parse().ok()),
            "--data-dir" => dir = args.next(),
            // Test fixture: act as an OpenAI-compatible engine with canned answers (used by engine pack tests).
            "--fake-engine" => fake = true,
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
        if let Err(e) = pitwall_server::fake_engine::serve(port.unwrap_or(0)).await {
            eprintln!("fake engine: {e}");
            std::process::exit(1);
        }
        return;
    }
    let dirs = dir.map_or_else(Dirs::for_user, Dirs::at);
    let st = AppState::open(dirs, None).await;
    let port = port.unwrap_or(st.cfg().server.port);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Pitwall cannot listen on {addr}: {e}. Is another Pitwall (or an Ollama on the same port) running? Change the port in Settings > Server or start with --port.");
            std::process::exit(1);
        }
    };
    let bound = listener.local_addr().map_or(port, |a| a.port());
    st.port.store(bound, std::sync::atomic::Ordering::SeqCst);
    st.write_discovery(bound);
    st.spawn_tasks();
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
        eprintln!("server stopped: {e}");
    }
    st.remove_discovery();
}
