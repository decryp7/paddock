//! The plugin's side of the connection: a plugin's `main` is `dyno_plugin_api::serve(MyPlugin)`.

use crate::plugin::Plugin;
use crate::protocol::{codes, PluginMessage, PluginReply, PluginRequest};
use dyno_ipc::{ChildLink, Peer, PeerOptions, RemoteError};
use std::process::ExitCode;
use std::sync::Arc;

/// Connects to the host (the bootstrap line on stdin), answers its calls until the connection or stdin closes, and
/// returns the exit code. A panic in a provider becomes a `panic` failure of that call (the host restarts the plugin).
pub fn serve<P: Plugin>(plugin: P) -> ExitCode {
    let conn = match ChildLink::from_stdin() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("this is a Dyno plugin; Dyno starts it itself. It could not connect: {e}");
            return ExitCode::FAILURE;
        }
    };
    let plugin = Arc::new(plugin);
    let handler = move |msg: PluginMessage| match msg {
        PluginMessage::Request(req) => handle(&*plugin, req).map(PluginMessage::Reply),
        PluginMessage::Reply(_) => Err(RemoteError::new(codes::BAD_REQUEST, "a plugin answers requests only")),
    };
    match Peer::start(conn, PeerOptions::new().serve(handler)) {
        Ok(peer) => {
            peer.wait_closed(None);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cannot serve the connection: {e}");
            ExitCode::FAILURE
        }
    }
}

fn unsupported(what: &str) -> RemoteError {
    RemoteError::new(codes::UNSUPPORTED, format!("this plugin contributes no {what}"))
}

fn failed(e: crate::PluginError) -> RemoteError {
    RemoteError::new(codes::FAILED, e.message)
}

/// Answers one request with `plugin`. Pure dispatch, so it is testable without a connection.
pub fn handle(plugin: &dyn Plugin, req: PluginRequest) -> Result<PluginReply, RemoteError> {
    match req {
        PluginRequest::Describe => Ok(PluginReply::Manifest(plugin.manifest())),
        PluginRequest::Sensors => plugin
            .as_sensors()
            .map(|s| PluginReply::Sensors(s.sensors()))
            .ok_or_else(|| unsupported("sensors")),
        PluginRequest::CleanupCategories => plugin
            .as_cleanup()
            .map(|c| PluginReply::Categories(c.categories()))
            .ok_or_else(|| unsupported("cleanup categories")),
        PluginRequest::Scan { category, limits } => {
            let cleanup = plugin.as_cleanup().ok_or_else(|| unsupported("cleanup categories"))?;
            if !cleanup.categories().iter().any(|c| c.id == category) {
                return Err(RemoteError::new(codes::NOT_FOUND, format!("no cleanup category {category:?}")));
            }
            cleanup.scan(&category, limits).map(PluginReply::Candidates).map_err(failed)
        }
        PluginRequest::Actions => plugin
            .as_actions()
            .map(|a| PluginReply::Actions(a.actions()))
            .ok_or_else(|| unsupported("actions")),
        PluginRequest::RunAction { action, event } => {
            let actions = plugin.as_actions().ok_or_else(|| unsupported("actions"))?;
            if !actions.actions().iter().any(|a| a.id == action) {
                return Err(RemoteError::new(codes::NOT_FOUND, format!("no action {action:?}")));
            }
            actions.run(&action, event).map(PluginReply::ActionDone).map_err(failed)
        }
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
        fn as_cleanup(&self) -> Option<&dyn CleanupProvider> {
            Some(self)
        }
        fn as_actions(&self) -> Option<&dyn ActionProvider> {
            Some(self)
        }
    }
    impl SensorProvider for Full {
        fn sensors(&self) -> Vec<SensorReading> {
            vec![SensorReading::new("s", "S", 1.0, "%")]
        }
    }
    impl CleanupProvider for Full {
        fn categories(&self) -> Vec<CategoryInfo> {
            vec![CategoryInfo {
                id: "c".into(),
                name: "C".into(),
                description: "d".into(),
                risk: Risk::Safe,
            }]
        }
        fn scan(&self, _: &str, limits: ScanLimits) -> Result<Vec<Candidate>, PluginError> {
            if limits.max_candidates == 0 {
                return Err(PluginError::new("no room"));
            }
            Ok(vec![Candidate {
                path: "/tmp/x".into(),
                bytes: 1,
                modified_ms: 0,
                reason: "r".into(),
            }])
        }
    }
    impl ActionProvider for Full {
        fn actions(&self) -> Vec<ActionInfo> {
            vec![ActionInfo {
                id: "a".into(),
                name: "A".into(),
                description: "d".into(),
            }]
        }
        fn run(&self, _: &str, event: RuleEvent) -> Result<String, PluginError> {
            Ok(format!("ran for {}", event.app))
        }
    }

    struct Bare;
    impl Plugin for Bare {
        fn manifest(&self) -> Manifest {
            Manifest::new("bare", "Bare", "1")
        }
    }

    fn event() -> RuleEvent {
        RuleEvent {
            rule: "r".into(),
            app: "app".into(),
            cpu: 90.0,
            at: 0,
        }
    }

    #[test]
    fn requests_reach_their_providers() {
        assert!(matches!(handle(&Full, PluginRequest::Describe), Ok(PluginReply::Manifest(m)) if m.id == "full"));
        assert!(matches!(handle(&Full, PluginRequest::Sensors), Ok(PluginReply::Sensors(s)) if s.len() == 1));
        assert!(matches!(handle(&Full, PluginRequest::CleanupCategories), Ok(PluginReply::Categories(c)) if c[0].risk == Risk::Safe));
        let scan = |category: &str, max| PluginRequest::Scan {
            category: category.into(),
            limits: ScanLimits {
                max_candidates: max,
                ..ScanLimits::default()
            },
        };
        assert!(matches!(handle(&Full, scan("c", 10)), Ok(PluginReply::Candidates(c)) if c.len() == 1));
        assert_eq!(handle(&Full, scan("c", 0)).unwrap_err().code, codes::FAILED);
        assert_eq!(handle(&Full, scan("zz", 10)).unwrap_err().code, codes::NOT_FOUND);
        assert!(matches!(handle(&Full, PluginRequest::Actions), Ok(PluginReply::Actions(a)) if a[0].id == "a"));
        let run = |action: &str| PluginRequest::RunAction {
            action: action.into(),
            event: event(),
        };
        assert_eq!(handle(&Full, run("a")), Ok(PluginReply::ActionDone("ran for app".into())));
        assert_eq!(handle(&Full, run("b")).unwrap_err().code, codes::NOT_FOUND);
    }

    #[test]
    fn a_plugin_without_a_provider_says_so() {
        for req in [
            PluginRequest::Sensors,
            PluginRequest::CleanupCategories,
            PluginRequest::Actions,
            PluginRequest::Scan {
                category: "c".into(),
                limits: ScanLimits::default(),
            },
            PluginRequest::RunAction {
                action: "a".into(),
                event: event(),
            },
        ] {
            assert_eq!(handle(&Bare, req).unwrap_err().code, codes::UNSUPPORTED);
        }
        assert!(handle(&Bare, PluginRequest::Describe).is_ok());
    }

    #[test]
    fn messages_roundtrip_through_postcard() {
        let msgs = [
            PluginMessage::Request(PluginRequest::RunAction {
                action: "a".into(),
                event: event(),
            }),
            PluginMessage::Reply(PluginReply::Manifest(
                Manifest::new("x", "X", "1").contributing(Contribution::Cleanup),
            )),
            PluginMessage::Reply(PluginReply::Sensors(vec![SensorReading::new("a", "b", f64::MAX, "u")])),
        ];
        for m in msgs {
            let bytes = postcard::to_stdvec(&m).unwrap();
            assert_eq!(postcard::from_bytes::<PluginMessage>(&bytes).unwrap(), m);
        }
    }
}
