//! The parser worker program (ADR 0006). The host starts it and writes a bootstrap line to its stdin; it connects back, answers
//! parse calls, and exits when stdin closes. `--self-test` (rule 23) proves it loads and parses without touching anything.

fn main() {
    use paddock_proc::self_test::{Check, handle};
    let checks = [Check::new("parse", || {
        let req = pacenotes_worker::ParseRequest {
            path: "check.rs".into(),
            bytes: b"pub fn check() {}\n".to_vec(),
            max_nodes: 1000,
        };
        let reply = pacenotes_worker::serve::answer(&req, &|| false).map_err(|e| e.to_string())?;
        match reply.facts {
            Some(f) if f.symbols.iter().any(|s| s.name == "check") => Ok(()),
            other => Err(format!("unexpected facts {other:?}")),
        }
    })];
    if let Some(code) = handle("pacenotes-parse-worker", env!("CARGO_PKG_VERSION"), &checks) {
        std::process::exit(code);
    }
    if let Err(e) = pacenotes_worker::serve::run() {
        eprintln!("pacenotes-parse-worker: {e}");
        std::process::exit(2);
    }
}
