//! The plugin side of the protocol: [`Plugin`] and [`serve`].

use crate::frame::{read_message, write_message, FrameError, FRAME_VERSION};
use crate::protocol::{ErrorKind, Manifest, Message, PluginError, Request, Response, WireRelease, MAGIC, MAX_RELEASES, PROTOCOL};
use crate::schema::Config;
use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::process::ExitCode;

/// What a plugin implements. Both methods may be called several times; `list` must not print to standard output
/// (that is the protocol's pipe; use standard error for diagnostics, which the host keeps for problem reports).
pub trait Plugin {
    /// Who the plugin is and the source kinds it offers.
    fn manifest(&self) -> Manifest;

    /// The releases of a source of `kind` configured with `config`, at most `limit`, newest first when the source
    /// knows an order. A panic here is caught and reported as a failure.
    fn list(&self, kind: &str, config: &Config, limit: usize) -> Result<Vec<WireRelease>, PluginError>;
}

/// Runs a plugin on standard input and output until the host shuts it down. Returns the process exit code: success
/// after a shutdown or when the host closes the pipe, failure when the host refuses the plugin or the pipe breaks.
pub fn serve<P: Plugin>(plugin: P) -> ExitCode {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    match serve_on(&plugin, &mut stdin.lock(), &mut stdout.lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// [`serve`] on any reader and writer (tests, other transports).
pub fn serve_on<P: Plugin, R: Read, W: Write>(plugin: &P, input: &mut R, output: &mut W) -> Result<(), String> {
    write_message(
        output,
        &Message::Hello {
            magic: MAGIC,
            frame: FRAME_VERSION,
            protocol: PROTOCOL,
        },
    )
    .map_err(|e| e.to_string())?;
    match read_message(input) {
        Ok(Message::Welcome { .. }) => {}
        Ok(Message::Refused { reason }) => return Err(format!("Spotter refused this plugin: {reason}")),
        Ok(other) => return Err(format!("expected a welcome, got {other:?}")),
        Err(FrameError::Closed) => return Ok(()),
        Err(e) => return Err(e.to_string()),
    }
    loop {
        let (id, request) = match read_message(input) {
            Ok(Message::Request { id, request }) => (id, request),
            Ok(other) => return Err(format!("expected a request, got {other:?}")),
            Err(FrameError::Closed) => return Ok(()),
            Err(e) => return Err(e.to_string()),
        };
        let shutdown = request == Request::Shutdown;
        let response = answer(plugin, request);
        write_message(output, &Message::Response { id, response }).map_err(|e| e.to_string())?;
        if shutdown {
            return Ok(());
        }
    }
}

fn answer<P: Plugin>(plugin: &P, request: Request) -> Response {
    let outcome = catch_unwind(AssertUnwindSafe(|| match request {
        Request::Describe => Response::Described(plugin.manifest()),
        Request::List { kind, config, limit } => {
            let limit = (limit as usize).min(MAX_RELEASES);
            match plugin.list(&kind, &config, limit) {
                Ok(mut releases) => {
                    releases.truncate(limit);
                    Response::Listed(releases)
                }
                Err(e) => Response::Failed(e),
            }
        }
        Request::Shutdown => Response::Bye,
    }));
    outcome.unwrap_or_else(|panic| {
        let what = panic
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into());
        Response::Failed(PluginError::new(ErrorKind::Failed, format!("the plugin crashed: {what}")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::encode;
    use crate::protocol::SourceKindSpec;
    use std::io::Cursor;

    struct Demo;

    impl Plugin for Demo {
        fn manifest(&self) -> Manifest {
            Manifest::new("demo", "Demo", "1.0").with_kind(SourceKindSpec::new("k", "K", "d"))
        }

        fn list(&self, kind: &str, config: &Config, _limit: usize) -> Result<Vec<WireRelease>, PluginError> {
            match kind {
                "panic" => panic!("boom"),
                "panic-string" => std::panic::panic_any(String::from("owned boom")),
                "panic-other" => std::panic::panic_any(42u8),
                "fail" => Err(PluginError::new(ErrorKind::Unreachable, "offline")),
                _ => Ok((0..10)
                    .map(|i| WireRelease::new(&format!("1.{i}"), config.get("n").unwrap_or("x"), "l"))
                    .collect()),
            }
        }
    }

    fn host_script(messages: &[Message]) -> Vec<u8> {
        messages.iter().flat_map(|m| encode(m).unwrap()).collect()
    }

    fn run(messages: &[Message]) -> (Result<(), String>, Vec<Message>) {
        let mut input = Cursor::new(host_script(messages));
        let mut output = Vec::new();
        let result = serve_on(&Demo, &mut input, &mut output);
        let mut out = Cursor::new(output);
        let mut seen = Vec::new();
        while let Ok(m) = read_message(&mut out) {
            seen.push(m);
        }
        (result, seen)
    }

    fn req(id: u32, request: Request) -> Message {
        Message::Request { id, request }
    }

    fn list(kind: &str, limit: u32) -> Request {
        Request::List {
            kind: kind.into(),
            config: Config::new().with("n", "name"),
            limit,
        }
    }

    #[test]
    fn a_session_answers_every_request_and_contains_panics() {
        let (result, out) = run(&[
            Message::Welcome { protocol: PROTOCOL },
            req(1, Request::Describe),
            req(2, list("k", 3)),
            req(3, list("fail", 3)),
            req(4, list("panic", 3)),
            req(5, list("panic-string", 3)),
            req(6, list("panic-other", 3)),
            req(7, Request::Shutdown),
            req(8, Request::Describe),
        ]);
        assert_eq!(result, Ok(()));
        assert!(matches!(&out[0], Message::Hello { magic, .. } if *magic == MAGIC));
        assert!(matches!(&out[1], Message::Response { id: 1, response: Response::Described(m) } if m.id == "demo"));
        assert!(matches!(&out[2], Message::Response { id: 2, response: Response::Listed(r) } if r.len() == 3 && r[0].name == "name"));
        assert!(matches!(&out[3], Message::Response { id: 3, response: Response::Failed(e) } if e.kind == ErrorKind::Unreachable));
        for (i, text) in [(4, "boom"), (5, "owned boom"), (6, "unknown panic")] {
            assert!(
                matches!(&out[i], Message::Response { response: Response::Failed(e), .. } if e.message.contains(text)),
                "{:?}",
                out[i]
            );
        }
        assert!(matches!(
            &out[7],
            Message::Response {
                id: 7,
                response: Response::Bye
            }
        ));
        assert_eq!(out.len(), 8, "nothing is answered after the shutdown");
    }

    #[test]
    fn refusal_closing_and_garbage_end_the_session() {
        let (r, _) = run(&[Message::Refused { reason: "old".into() }]);
        assert!(r.unwrap_err().contains("old"));
        assert_eq!(run(&[]).0, Ok(()), "the host closing before the welcome is a clean end");
        assert_eq!(run(&[Message::Welcome { protocol: PROTOCOL }]).0, Ok(()));
        assert!(run(&[req(1, Request::Describe)]).0.unwrap_err().contains("expected a welcome"));
        assert!(
            run(&[Message::Welcome { protocol: PROTOCOL }, Message::Welcome { protocol: PROTOCOL }])
                .0
                .unwrap_err()
                .contains("expected a request")
        );
        let mut input = Cursor::new(vec![1u8, 0]);
        assert!(serve_on(&Demo, &mut input, &mut Vec::new()).is_err());
        let mut input = Cursor::new([host_script(&[Message::Welcome { protocol: PROTOCOL }]), vec![1, 0]].concat());
        assert!(serve_on(&Demo, &mut input, &mut Vec::new()).is_err());
    }

    #[test]
    fn a_broken_output_pipe_is_an_error() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert!(serve_on(&Demo, &mut Cursor::new(Vec::new()), &mut Closed).is_err());
        let mut input = Cursor::new(host_script(&[Message::Welcome { protocol: PROTOCOL }, req(1, Request::Describe)]));
        let mut out = OneWrite(false);
        assert!(serve_on(&Demo, &mut input, &mut out).is_err());
    }

    /// Accepts the first message, then fails.
    struct OneWrite(bool);

    impl Write for OneWrite {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.0 {
                Err(std::io::ErrorKind::BrokenPipe.into())
            } else {
                Ok(buf.len())
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0 = true;
            Ok(())
        }
    }
}
