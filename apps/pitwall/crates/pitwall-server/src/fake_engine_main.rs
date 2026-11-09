//! `pitwall-fake-engine`: the fake LLM engine the tests start as a child process (ADR 0073 / rule 73). Built only with
//! the `test-support` feature (a self dev-dependency), so no shipped build carries it or its flags.
//!
//! `pitwall-fake-engine [--port N] [--die-after-ms N] [--ready-when FILE]`

use pitwall_server::fake_engine;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let (mut port, mut die_after, mut ready_when) = (None::<u16>, None::<u64>, None::<std::path::PathBuf>);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => port = args.next().and_then(|p| p.parse().ok()),
            "--die-after-ms" => die_after = args.next().and_then(|p| p.parse().ok()),
            "--ready-when" => match fake_engine::parse_ready_when(args.next()) {
                Ok(p) => ready_when = Some(p),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(2);
                }
            },
            "--version" => {
                println!("pitwall-fake-engine {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            other => {
                eprintln!("unknown option {other}");
                std::process::exit(2);
            }
        }
    }
    if let Err(e) = fake_engine::serve(port.unwrap_or(0), die_after, ready_when).await {
        eprintln!("fake engine: {e}");
        std::process::exit(1);
    }
}
