//! The plugin's side of the connection: a plugin's `main` is `dyno_plugin_api::serve(MyPlugin)`.

use crate::plugin::Plugin;
use crate::protocol::{codes, wire, PluginMessage, PluginReply, PluginRequest};
use paddock_ipc::{body_handler, child, RemoteError};
use std::process::ExitCode;
use std::sync::Arc;

/// Connects to the host (the bootstrap line on stdin), answers its calls until the connection or stdin closes, and
/// returns the exit code. A panic in a provider becomes a `panic` failure of that call (the host restarts the plugin).
pub fn serve<P: Plugin>(plugin: P) -> ExitCode {
    let plugin = Arc::new(plugin);
    let make = move |_: &paddock_ipc::Bootstrap| {
        body_handler(move |msg: PluginMessage| match msg {
            PluginMessage::Request(req) => handle(&*plugin, req).map(PluginMessage::Reply),
            PluginMessage::Reply(_) => Err(RemoteError::new(codes::BAD_REQUEST, "a plugin answers requests only")),
        })
    };
    match child::serve("Dyno", wire(), make, |_, _| {}) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("this is a Dyno plugin; Dyno starts it itself. It could not serve the connection: {e}");
            ExitCode::FAILURE
        }
    }
}

fn unsupported(what: &str) -> RemoteError {
    RemoteError::new(codes::UNSUPPORTED, format!("this plugin contributes no {what}"))
}

/// Answers one request with `plugin`. Pure dispatch, so it is testable without a connection.
pub fn handle(plugin: &dyn Plugin, req: PluginRequest) -> Result<PluginReply, RemoteError> {
    match req {
        PluginRequest::Describe => Ok(PluginReply::Manifest(plugin.manifest())),
        PluginRequest::Sensors => plugin
            .as_sensors()
            .map(|s| PluginReply::Sensors(s.sensors()))
            .ok_or_else(|| unsupported("sensors")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    struct Full;
    impl Plugin for Full {
        fn manifest(&self) -> Manifest {
            Manifest::new("full", "Full", "1")
        }
        fn as_sensors(&self) -> Option<&dyn SensorProvider> {
            Some(self)
        }
    }
    impl SensorProvider for Full {
        fn sensors(&self) -> Vec<SensorReading> {
            vec![SensorReading::new("s", "S", 1.0, "%")]
        }
    }

    struct Bare;
    impl Plugin for Bare {
        fn manifest(&self) -> Manifest {
            Manifest::new("bare", "Bare", "1")
        }
    }

    #[test]
    fn requests_reach_their_providers() {
        assert!(matches!(handle(&Full, PluginRequest::Describe), Ok(PluginReply::Manifest(m)) if m.id == "full"));
        assert!(matches!(handle(&Full, PluginRequest::Sensors), Ok(PluginReply::Sensors(s)) if s.len() == 1));
    }

    #[test]
    fn a_plugin_without_a_provider_says_so() {
        assert_eq!(handle(&Bare, PluginRequest::Sensors).unwrap_err().code, codes::UNSUPPORTED);
        assert!(handle(&Bare, PluginRequest::Describe).is_ok());
    }

    #[test]
    fn messages_roundtrip_through_postcard() {
        let msgs = [
            PluginMessage::Request(PluginRequest::Sensors),
            PluginMessage::Reply(PluginReply::Manifest(
                Manifest::new("x", "X", "1").contributing(Contribution::Sensors),
            )),
            PluginMessage::Reply(PluginReply::Sensors(vec![SensorReading::new("a", "b", f64::MAX, "u")])),
        ];
        for m in msgs {
            let bytes = postcard::to_stdvec(&m).unwrap();
            assert_eq!(postcard::from_bytes::<PluginMessage>(&bytes).unwrap(), m);
        }
    }
}
