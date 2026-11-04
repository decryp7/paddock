//! Alarms for the two network events worth interrupting for: a program that starts listening for connections from the
//! network, and a program that connects to a host it had not talked to before. Both are off until the user turns them
//! on (observe-only by default); the events themselves are always recorded in the activity log.

use crate::alarms::feed::{Observation, Source};
use crate::health::Severity;
use crate::network::{Exposure, NetEvent};
use serde::{Deserialize, Serialize};

/// `Settings.networkAlerts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkAlerts {
    /// Raise an alarm when a program starts listening on every interface (or one network interface).
    pub new_exposed_listener: bool,
    /// Raise an alarm when a program connects to a host for the first time.
    pub new_remote_host: bool,
}

/// The alarm an event stands for under `rules`, or `None` when that rule is off or the event is not notable (a new
/// loopback listener is only visible on this computer).
pub fn observation(event: &NetEvent, rules: NetworkAlerts) -> Option<Observation> {
    match event {
        NetEvent::NewListener {
            program,
            port,
            proto,
            exposure,
        } if rules.new_exposed_listener && *exposure != Exposure::Loopback => Some(Observation {
            source: Source::Network,
            name: "newExposedListener".into(),
            subject: format!("{program}:{proto:?}:{port}"),
            severity: if *exposure == Exposure::AllInterfaces {
                Severity::Warning
            } else {
                Severity::Info
            },
            title: format!("{program} started listening on port {port}"),
            detail: event.alarm_detail(),
            value: None,
            metric: None,
            active: true,
            one_off: true,
        }),
        NetEvent::NewRemoteHost { program, host, .. } if rules.new_remote_host => Some(Observation {
            source: Source::Network,
            name: "newRemoteHost".into(),
            subject: format!("{program}:{host}"),
            severity: Severity::Info,
            title: format!("{program} connected to a new host"),
            detail: event.alarm_detail(),
            value: None,
            metric: None,
            active: true,
            one_off: true,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::Proto;

    fn listener(exposure: Exposure) -> NetEvent {
        NetEvent::NewListener {
            program: "node".into(),
            port: 3000,
            proto: Proto::Tcp,
            exposure,
        }
    }
    fn host() -> NetEvent {
        NetEvent::NewRemoteHost {
            program: "node".into(),
            host: "93.184.216.34".into(),
            port: 443,
        }
    }
    const ON: NetworkAlerts = NetworkAlerts {
        new_exposed_listener: true,
        new_remote_host: true,
    };

    #[test]
    fn both_rules_are_off_by_default() {
        let off = NetworkAlerts::default();
        assert!(!off.new_exposed_listener && !off.new_remote_host);
        assert!(observation(&listener(Exposure::AllInterfaces), off).is_none());
        assert!(observation(&host(), off).is_none());
    }

    #[test]
    fn an_exposed_listener_is_a_warning_and_a_loopback_one_is_not_an_alarm() {
        let o = observation(&listener(Exposure::AllInterfaces), ON).unwrap();
        assert_eq!(o.severity, Severity::Warning);
        assert_eq!(o.rule(), "network:newExposedListener");
        assert_eq!(o.key(), "network:newExposedListener:node:Tcp:3000");
        assert!(o.one_off && o.active);
        assert!(o.detail.contains("reachable from the network"));
        assert_eq!(observation(&listener(Exposure::Interface), ON).unwrap().severity, Severity::Info);
        assert!(observation(&listener(Exposure::Loopback), ON).is_none());
    }

    #[test]
    fn a_new_host_is_informational_and_each_rule_is_independent() {
        let o = observation(&host(), ON).unwrap();
        assert_eq!((o.severity, o.rule().as_str()), (Severity::Info, "network:newRemoteHost"));
        let only_hosts = NetworkAlerts {
            new_remote_host: true,
            ..Default::default()
        };
        assert!(observation(&listener(Exposure::AllInterfaces), only_hosts).is_none());
        assert!(observation(&host(), only_hosts).is_some());
        let json = serde_json::to_string(&ON).unwrap();
        assert_eq!(json, r#"{"newExposedListener":true,"newRemoteHost":true}"#);
        assert_eq!(serde_json::from_str::<NetworkAlerts>("{}").unwrap(), NetworkAlerts::default());
    }

    #[test]
    fn the_detail_adds_to_the_title_and_never_repeats_it() {
        for e in [listener(Exposure::AllInterfaces), listener(Exposure::Interface), host()] {
            let o = observation(&e, ON).unwrap();
            assert!(
                !o.detail.contains(&o.title),
                "detail repeats its title: {:?} / {:?}",
                o.title,
                o.detail
            );
            assert!(!o.detail.is_empty());
        }
        assert_eq!(observation(&host(), ON).unwrap().detail, "93.184.216.34:443");
    }
}
