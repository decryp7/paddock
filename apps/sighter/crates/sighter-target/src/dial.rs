//! Where a VNC connection may go. VNC is unencrypted, so reaching a machine is an explicit choice (local first, family
//! rule 42): this computer is the default, a private network needs [`Scope::Lan`], a public address needs [`Scope::Public`].
//! The never-reachable classes (link-local, unspecified, multicast) and Offline mode are refused whatever the scope.
//!
//! All the checking is `paddock-net`'s: [`Egress::check_host_with`] refuses Offline before anything is resolved, resolves the
//! name through the egress's own resolver (a fake in tests, so no test queries DNS) and vets **every** address of the name
//! against the policy's reach, [`LanAccess`] and [`PublicAccess`]. The client dials only the addresses it returns, never the
//! name again, so a name cannot answer differently the second time. This module only maps a scope onto those two arguments.

use paddock_net::{Egress, LanAccess, PublicAccess};
use sighter_core::ports::TargetError;
use std::net::SocketAddr;
use std::sync::Arc;

/// The farthest a connection may reach.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    /// This computer only (VMs with a local console port). The default.
    #[default]
    Loopback,
    /// This computer and private networks.
    Lan,
    /// Anywhere that is not a never-reachable class.
    Public,
}

impl Scope {
    /// The `paddock-net` limits of this scope.
    fn limits(self) -> (LanAccess, PublicAccess) {
        match self {
            Scope::Loopback => (LanAccess::Deny, PublicAccess::Deny),
            Scope::Lan => (LanAccess::Policy, PublicAccess::Deny),
            Scope::Public => (LanAccess::Policy, PublicAccess::Policy),
        }
    }
}

/// The door of a VNC connection: the app's egress and the scope the user chose for this target.
#[derive(Clone)]
pub struct Dial {
    egress: Arc<Egress>,
    scope: Scope,
}

impl Dial {
    /// A door over `egress` (its policy's reach must be at least as far as `scope`) limited to `scope`.
    pub fn new(egress: Arc<Egress>, scope: Scope) -> Dial {
        Dial { egress, scope }
    }

    /// The checked addresses of `host`, or why the target may not be reached. Nothing is resolved when the app is offline.
    pub fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, TargetError> {
        let (lan, public) = self.scope.limits();
        let addrs = self
            .egress
            .check_host_with(host, port, lan, public)
            .map_err(|e| TargetError::Lost(e.to_string()))?;
        Ok(addrs.into_iter().map(|ip| SocketAddr::new(ip, port)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::time::FakeClock;
    use paddock_net::{FakeWire, Policy, Reach, StaticLookup};

    struct Rig {
        dial: Dial,
        egress: Arc<Egress>,
        lookup: Arc<StaticLookup>,
    }

    fn rig(reach: Reach, scope: Scope) -> Rig {
        let lookup = Arc::new(
            StaticLookup::new()
                .with("vm.test", &["127.0.0.1"])
                .with("nas.test", &["192.168.1.20"])
                .with("far.test", &["93.184.216.34"])
                .with("meta.test", &["169.254.169.254"])
                .with("rebind.test", &["127.0.0.1", "93.184.216.34"])
                .with("void.test", &[]),
        );
        let egress = Arc::new(Egress::new(
            Policy::default().reach(reach),
            Arc::new(FakeWire::new()),
            lookup.clone(),
            Arc::new(FakeClock::new()),
        ));
        Rig {
            dial: Dial::new(egress.clone(), scope),
            egress,
            lookup,
        }
    }

    fn lost(r: Result<Vec<SocketAddr>, TargetError>) -> String {
        match r {
            Err(TargetError::Lost(m)) => m,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn this_computer_is_the_default_and_nothing_farther() {
        let r = rig(Reach::Loopback, Scope::default());
        assert_eq!(r.dial.resolve("vm.test", 5901).unwrap(), vec!["127.0.0.1:5901".parse().unwrap()]);
        assert_eq!(r.dial.resolve("127.0.0.1", 5900).unwrap().len(), 1, "a literal needs no lookup");
        assert_eq!(r.lookup.asked(), vec!["vm.test"]);
        assert!(r.dial.resolve("nas.test", 5900).is_err());
        assert!(r.dial.resolve("far.test", 5900).is_err());
    }

    #[test]
    fn a_private_network_needs_the_lan_scope() {
        let r = rig(Reach::Loopback, Scope::Lan);
        assert_eq!(
            r.dial.resolve("nas.test", 5900).unwrap(),
            vec!["192.168.1.20:5900".parse().unwrap()]
        );
        assert!(r.dial.resolve("vm.test", 5900).is_ok());
        assert!(lost(r.dial.resolve("far.test", 5900)).contains("public address"));
        assert!(lost(rig(Reach::Loopback, Scope::Loopback).dial.resolve("far.test", 5900)).contains("public address"));
    }

    #[test]
    fn a_public_address_needs_the_public_scope() {
        let r = rig(Reach::Loopback, Scope::Public);
        assert_eq!(
            r.dial.resolve("far.test", 5900).unwrap(),
            vec!["93.184.216.34:5900".parse().unwrap()]
        );
        assert!(r.dial.resolve("nas.test", 5900).is_ok());
        // With the default egress (public only) this computer is out of reach, whatever the scope says.
        let strict = rig(Reach::Public, Scope::Public);
        assert!(strict.dial.resolve("vm.test", 5900).is_err());
        assert!(strict.dial.resolve("far.test", 5900).is_ok());
    }

    #[test]
    fn the_never_classes_and_rebinding_names_are_refused_in_every_scope() {
        for scope in [Scope::Loopback, Scope::Lan, Scope::Public] {
            let r = rig(Reach::Loopback, scope);
            assert!(r.dial.resolve("meta.test", 5900).is_err(), "{scope:?}");
            assert!(r.dial.resolve("169.254.169.254", 5900).is_err(), "{scope:?}");
            assert!(r.dial.resolve("void.test", 5900).is_err(), "{scope:?}");
            assert!(r.dial.resolve("unknown.test", 5900).is_err(), "{scope:?}");
        }
        // One bad address among good ones refuses the whole name.
        let r = rig(Reach::Loopback, Scope::Lan);
        assert!(r.dial.resolve("rebind.test", 5900).is_err());
    }

    #[test]
    fn offline_blocks_everything_before_a_name_is_resolved() {
        let r = rig(Reach::Loopback, Scope::Public);
        r.egress.set_offline(true);
        for host in ["vm.test", "127.0.0.1", "far.test"] {
            assert!(lost(r.dial.resolve(host, 5900)).contains("offline"), "{host}");
        }
        assert!(r.lookup.asked().is_empty());
        r.egress.set_offline(false);
        assert!(r.dial.resolve("vm.test", 5900).is_ok());
    }
}
