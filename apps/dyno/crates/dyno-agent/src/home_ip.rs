//! Finding the user's own public address for the map's "from" end (user decision 2026-11-03, `0031-amendments.md`). On by default
//! with its own Settings switch; the first-run note names the services. Only the address is asked for: Cloudflare's trace page
//! (`https://1.1.1.1/cdn-cgi/trace`, the `ip=` line) first, `https://api.ipify.org` as the fallback. Both hosts are pinned in the
//! egress policy, answers are capped at 4 KiB, no redirect is followed, and the text is parsed strictly into an [`IpAddr`] that must
//! be public. The address is then located **locally** with the downloaded database; no third-party geolocation service is used.
//! The result is kept in memory only, at most an hour. Logs say which service answered and how long it took, never the address
//! (ER-27). A manual location in Settings always wins, and the region setting is the fallback the page labels as such.

use dyno_core::ip_lookup::{is_public, parse_ip};
use paddock_jobs::time::Clock;
use paddock_jobs::CancelToken;
use paddock_net::{Egress, Policy, Reach, Request, SystemLookup};
use std::net::IpAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How long an answer is reused.
pub const REUSE: Duration = Duration::from_secs(3600);
/// Largest answer read.
pub const MAX_BODY: u64 = 4096;

/// A service that echoes the caller's address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    /// `https://1.1.1.1/cdn-cgi/trace`: lines of `key=value`, one of them `ip=`.
    Cloudflare,
    /// `https://api.ipify.org`: the address as the whole body.
    Ipify,
}

impl Service {
    /// The address asked.
    pub fn url(self) -> &'static str {
        match self {
            Service::Cloudflare => "https://1.1.1.1/cdn-cgi/trace",
            Service::Ipify => "https://api.ipify.org",
        }
    }

    /// The name the page shows ("from your public IP (via Cloudflare)").
    pub fn name(self) -> &'static str {
        match self {
            Service::Cloudflare => "Cloudflare",
            Service::Ipify => "ipify",
        }
    }

    /// Reads the answer strictly: the address, public, or `None`.
    pub fn parse(self, body: &str) -> Option<IpAddr> {
        let text = match self {
            Service::Cloudflare => body.lines().find_map(|l| l.strip_prefix("ip="))?,
            Service::Ipify => body.trim(),
        };
        let ip = parse_ip(text.trim()).ok()?;
        is_public(&ip).then_some(ip)
    }
}

/// The policy of the lookup door: the two hosts, public addresses, small answers, no redirect.
pub fn policy() -> Policy {
    let mut p = Policy::default()
        .reach(Reach::Public)
        .max_redirects(0)
        .max_body(MAX_BODY)
        .timeout(Duration::from_secs(10))
        .allow_hosts(&["1.1.1.1", "api.ipify.org"]);
    p.user_agent = format!("Dyno/{}", env!("CARGO_PKG_VERSION"));
    p
}

/// What was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    /// The public address.
    pub ip: IpAddr,
    /// Which service said so.
    pub via: Service,
}

/// Asks the services and remembers the answer for an hour (in memory only).
pub struct HomeIp {
    egress: Arc<Egress>,
    clock: Arc<dyn Clock>,
    cached: Mutex<Option<(Duration, Found)>>,
}

impl HomeIp {
    /// Over a given door (tests pass a fake wire).
    pub fn new(egress: Arc<Egress>, clock: Arc<dyn Clock>) -> HomeIp {
        HomeIp {
            egress,
            clock,
            cached: Mutex::new(None),
        }
    }

    /// The real door: the system resolver and the real wire.
    pub fn real(clock: Arc<dyn Clock>) -> Result<HomeIp, String> {
        let p = policy();
        let wire = paddock_net::UreqWire::new(&p, paddock_download::ProxyMode::System).map_err(|e| e.to_string())?;
        Ok(HomeIp::new(
            Arc::new(Egress::new(p, Arc::new(wire), Arc::new(SystemLookup), clock.clone())),
            clock,
        ))
    }

    /// Forgets the answer (a network change: the address may differ now).
    pub fn forget(&self) {
        *self.cached.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// The public address, from memory when it is under an hour old, else from the services in order. `None` when neither answers
    /// usably (the page falls back to the region setting). Blocks: call it from a job.
    pub fn find(&self, cancel: &CancelToken) -> Option<Found> {
        let now = self.clock.monotonic();
        if let Some((at, found)) = *self.cached.lock().unwrap_or_else(PoisonError::into_inner) {
            if now.saturating_sub(at) < REUSE {
                return Some(found);
            }
        }
        for service in [Service::Cloudflare, Service::Ipify] {
            let started = self.clock.monotonic();
            let outcome = self.egress.send(&Request::get(service.url()).without_lan(), cancel);
            let ms = self.clock.monotonic().saturating_sub(started).as_millis();
            match outcome {
                Ok(r) if r.ok() => match service.parse(&r.text()) {
                    Some(ip) => {
                        log::info!(target: "dyno::geo", "own address found: service={} duration_ms={ms}", service.name());
                        let found = Found { ip, via: service };
                        *self.cached.lock().unwrap_or_else(PoisonError::into_inner) = Some((now, found));
                        return Some(found);
                    }
                    None => log::warn!(target: "dyno::geo", "own address lookup: service={} gave an unusable answer", service.name()),
                },
                Ok(r) => log::warn!(target: "dyno::geo", "own address lookup: service={} status={}", service.name(), r.status),
                Err(e) => log::warn!(target: "dyno::geo", "own address lookup: service={} failed ({e})", service.name()),
            }
            if cancel.is_cancelled() {
                return None;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::time::FakeClock;
    use paddock_net::{FakeWire, Reply, StaticLookup};

    const TRACE: &str = "fl=1f1\nh=1.1.1.1\nip=8.8.4.4\nts=1.5\nvisit_scheme=https\n";

    fn rig(replies: Vec<Reply>) -> (HomeIp, Arc<FakeWire>, FakeClock) {
        let clock = FakeClock::new();
        let mut wire = FakeWire::new();
        for r in replies {
            wire = wire.then(r);
        }
        let wire = Arc::new(wire);
        let lookup = Arc::new(StaticLookup::new().with("api.ipify.org", &["104.26.12.205"]));
        let egress = Egress::new(policy(), wire.clone(), lookup, Arc::new(clock.clone()));
        (HomeIp::new(Arc::new(egress), Arc::new(clock.clone())), wire, clock)
    }

    #[test]
    fn answers_are_parsed_strictly() {
        let ip = |s: &str| Some(s.parse::<IpAddr>().unwrap());
        assert_eq!(Service::Cloudflare.parse(TRACE), ip("8.8.4.4"));
        assert_eq!(Service::Ipify.parse("2606:4700::1111\n"), ip("2606:4700::1111"));
        for bad in ["", "ip=", "ip=not-an-ip", "ip=10.0.0.1", "ip=8.8.4.4 evil", "h=1.1.1.1"] {
            assert_eq!(Service::Cloudflare.parse(bad), None, "{bad}");
        }
        for bad in ["<html>", "8.8.4.4/32", "192.168.1.1", "8.8.4.4 8.8.8.8", "::1"] {
            assert_eq!(Service::Ipify.parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_first_service_answers_and_the_result_is_reused_for_an_hour() {
        // 1.1.1.1 is an address, not a name: the resolver is not asked for it.
        let (h, wire, clock) = rig(vec![Reply::ok(TRACE.as_bytes()), Reply::ok(TRACE.as_bytes())]);
        let f = h.find(&CancelToken::new()).unwrap();
        assert_eq!((f.ip, f.via), ("8.8.4.4".parse().unwrap(), Service::Cloudflare));
        clock.advance(Duration::from_secs(1800));
        assert_eq!(h.find(&CancelToken::new()), Some(f));
        assert_eq!(wire.sends(), 1);
        clock.advance(Duration::from_secs(1800));
        h.find(&CancelToken::new()).unwrap();
        assert_eq!(wire.sends(), 2, "after an hour it asks again");
        h.forget();
        h.find(&CancelToken::new());
        assert!(wire.sends() > 2, "a network change makes it ask again");
    }

    #[test]
    fn the_fallback_is_used_when_the_first_service_fails_or_answers_nonsense() {
        let (h, wire, _) = rig(vec![Reply::status(500), Reply::ok(b"8.8.8.8")]);
        let f = h.find(&CancelToken::new()).unwrap();
        assert_eq!((f.ip, f.via), ("8.8.8.8".parse().unwrap(), Service::Ipify));
        assert_eq!(wire.seen()[1].url, "https://api.ipify.org/");
        let (h, _, _) = rig(vec![Reply::ok(b"ip=10.1.1.1\n"), Reply::ok(b"hello")]);
        assert_eq!(
            h.find(&CancelToken::new()),
            None,
            "neither usable: the page uses the region setting"
        );
    }

    #[test]
    fn a_redirect_is_not_followed() {
        let (h, wire, _) = rig(vec![
            Reply::redirect(302, "https://evil.example/"),
            Reply::redirect(302, "https://evil.example/"),
        ]);
        assert_eq!(h.find(&CancelToken::new()), None);
        assert_eq!(wire.sends(), 2, "one request per service, nothing further");
    }
}
