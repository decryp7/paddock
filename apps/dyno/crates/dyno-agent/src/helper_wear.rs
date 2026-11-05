//! SSD wear through the helper first, then on this computer (Dyno ADR 0027 section 7, amended 2026-11-04 by the user: helper first,
//! unelevated fallback). With the helper on, every disk is read through it (it holds the rights Windows withholds); with it off or
//! unable, the local reader uses any smartmontools on the PC (Dyno's own, or the person's own install) for the disks the OS allows
//! without administrator rights and names the others.
//!
//! This is the first instance of the common seam ([`crate::helper_first`]); the policy lives there.

use crate::helper_first::{HelperCalls, HelperFirst, HelperReading, ReadClass};
use crate::helper_link::CallError;
use dyno_core::hardware::DiskWear;
use dyno_core::helper::HelperHost;
use dyno_core::helper_requests::SmartQuery;
use dyno_core::metrics::Reading;
use dyno_core::ports::{DiskWearSource, PlatformError};
use dyno_core::smart::SmartDisk;

/// The product choice behind the fallback: when the helper is off or cannot read, still read what this account may.
/// `false` would make the helper the only way to read SMART (the earlier decision O2); every other line stays the same.
pub const READ_LOCALLY_WHEN_HELPER_OFF: bool = true;

/// The wear reading: the helper's `Smart` request, or the agent's own reader.
pub struct SmartWear {
    local: Box<dyn DiskWearSource>,
    host: HelperHost,
}

impl SmartWear {
    /// The reading with `local` as the agent's own reader.
    pub fn new(local: Box<dyn DiskWearSource>) -> SmartWear {
        SmartWear {
            local,
            host: dyno_platform::hardware::this_host(),
        }
    }

    /// Reads as if on `host` (tests).
    pub fn with_host(mut self, host: HelperHost) -> SmartWear {
        self.host = host;
        self
    }
}

/// SSD wear through the seam.
pub type HelperFirstWear = HelperFirst<SmartWear>;

/// A wear reader over `port` with `local` as the fallback, switched as [`READ_LOCALLY_WHEN_HELPER_OFF`] says.
pub fn helper_first_wear(port: Box<dyn crate::helper_first::HelperPort>, local: Box<dyn DiskWearSource>) -> HelperFirstWear {
    helper_first_wear_on(dyno_platform::hardware::this_host(), port, local)
}

/// [`helper_first_wear`] as if on `host` (tests, so they read the same on every OS).
pub fn helper_first_wear_on(
    host: HelperHost,
    port: Box<dyn crate::helper_first::HelperPort>,
    local: Box<dyn DiskWearSource>,
) -> HelperFirstWear {
    HelperFirst::new(SmartWear::new(local).with_host(host), port).with_fallback(READ_LOCALLY_WHEN_HELPER_OFF)
}

impl HelperReading for SmartWear {
    type Value = Vec<DiskWear>;

    fn name(&self) -> &'static str {
        "SSD wear"
    }

    fn class(&self) -> ReadClass {
        // smartctl takes as long as it takes, and the transport gives the kind minutes: never the 2 s of a live reading.
        ReadClass::Bulk
    }

    fn helper_applies(&self) -> bool {
        // On macOS the helper has no smartctl: the agent's own copy runs as the person (the helper's `NotHere`).
        self.host != HelperHost::Macos
    }

    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<Vec<DiskWear>, CallError> {
        let reply = calls.smart(SmartQuery::default())?;
        // The helper's reply carries the id the history is kept under, so the history continues across both paths.
        let wear: Vec<DiskWear> = reply.disks.iter().filter_map(SmartDisk::to_wear).collect();
        log::info!(
            target: "dyno::hardware",
            "SSD wear: the helper read {} disk(s), {} with wear counters, {} skipped, {} not visited",
            reply.disks.len(),
            wear.len(),
            reply.skipped.len(),
            reply.not_visited
        );
        for s in &reply.skipped {
            log::info!(target: "dyno::hardware", "SSD wear: the helper skipped {}: {}", s.device, s.reason);
        }
        if wear.is_empty() {
            // Nothing to forecast from this answer: the agent's own reader gets its turn.
            return Err(PlatformError::Failed(reply.skipped.first().map_or_else(
                || "the helper found no SSD with wear counters.".to_string(),
                |s| format!("the helper skipped {}: {}", s.device, s.reason),
            ))
            .into());
        }
        Ok(wear)
    }

    fn local(&mut self) -> Reading<Vec<DiskWear>> {
        self.local.read()
    }
}

impl DiskWearSource for HelperFirstWear {
    fn read(&mut self) -> Reading<Vec<DiskWear>> {
        HelperFirst::read(self).value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_first::testkit::{centre, port, Net};
    use dyno_core::helper::HelperHost;
    use dyno_core::helper_wire::HelperReply;
    use dyno_core::read_via::ReadVia;
    use dyno_core::smart::{SmartReply, SmartSkip};
    use dyno_platform::hardware::FakeWear;
    use paddock_jobs::time::FakeClock;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    /// A fallback that counts how often it is asked.
    struct Counting(Arc<AtomicU32>);
    impl DiskWearSource for Counting {
        fn read(&mut self) -> Reading<Vec<DiskWear>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Reading::unavailable("local")
        }
    }

    fn nvme() -> SmartDisk {
        SmartDisk {
            device: "/dev/nvme0".into(),
            model: "EXAMPLE NVME".into(),
            capacity: Some(1_000_000_000_000),
            temperature_c: Some(40),
            percent_used: Some(4),
            media_errors: Some(0),
            power_on_hours: Some(1200),
            unsafe_shutdowns: Some(2),
            passed: Some(true),
            wear_id: Some("EXAMPLE NVME EX1".into()),
            bytes_written: Some(84_800_000_000_000),
            rated_bytes: None,
        }
    }

    fn reply(disks: Vec<SmartDisk>, skipped: Vec<SmartSkip>) -> HelperReply {
        HelperReply::Smart(SmartReply {
            disks,
            skipped,
            not_visited: 0,
        })
    }

    fn wear(up: bool, answers: Vec<Result<HelperReply, crate::helper_link::TransportError>>) -> (HelperFirstWear, Arc<AtomicU32>) {
        let calls = Arc::new(AtomicU32::new(0));
        let c = centre(&Net::new(up, answers), &FakeClock::new());
        (
            helper_first_wear_on(HelperHost::Linux, port(&c), Box::new(Counting(calls.clone()))),
            calls,
        )
    }

    #[test]
    fn a_ready_helper_with_one_nvme_disk_makes_the_wear_reading_available_without_asking_the_local_reader() {
        let (mut r, calls) = wear(true, vec![Ok(reply(vec![nvme()], vec![]))]);
        let got = DiskWearSource::read(&mut r);
        let w = &got.value().expect("available")[0];
        assert_eq!(
            (w.id.as_str(), w.percent_used, w.power_on_hours, w.bytes_written),
            ("EXAMPLE NVME EX1", Some(4.0), Some(1200), Some(84_800_000_000_000))
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn without_the_helper_or_when_it_reads_nothing_the_local_reader_is_used() {
        use crate::helper_link::TransportError;
        let skip = SmartSkip {
            device: "/dev/sda".into(),
            reason: "x".into(),
        };
        let cases: Vec<(bool, Vec<Result<HelperReply, TransportError>>)> = vec![
            (false, vec![]),
            (
                true,
                vec![Err(TransportError::Remote {
                    code: "unsupported".into(),
                    message: "On macOS Dyno reads SMART itself.".into(),
                })],
            ),
            (true, vec![Ok(reply(vec![], vec![skip]))]),
            (true, vec![Ok(reply(vec![SmartDisk { wear_id: None, ..nvme() }], vec![]))]),
        ];
        for (up, answers) in cases {
            let (mut r, calls) = wear(up, answers);
            assert!(DiskWearSource::read(&mut r).value().is_none());
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn the_local_fallback_can_be_switched_off_and_then_the_answer_says_what_to_turn_on() {
        let (r, calls) = wear(false, vec![]);
        let mut r = r.with_fallback(false);
        assert!(matches!(DiskWearSource::read(&mut r), Reading::Unavailable(m) if m.contains("Administrator readings")));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let c = centre(&Net::new(false, vec![]), &FakeClock::new());
        let mut ok = helper_first_wear_on(HelperHost::Linux, port(&c), Box::new(FakeWear(Reading::Available(vec![]))));
        assert!(DiskWearSource::read(&mut ok).value().is_some());
    }

    #[test]
    fn a_smart_read_gets_no_live_deadline_because_smartctl_takes_as_long_as_it_takes() {
        let net = Net::new(true, vec![Ok(reply(vec![nvme()], vec![]))]);
        let c = centre(&net, &FakeClock::new());
        let mut r = helper_first_wear_on(HelperHost::Linux, port(&c), Box::new(Counting(Arc::default())));
        assert_eq!(HelperFirst::read(&mut r).via, ReadVia::Helper);
        let maxes = net.maxes.lock().unwrap().clone();
        assert_eq!(maxes.last(), Some(&None), "the Smart call: {maxes:?}");
        assert!(
            maxes[..maxes.len() - 1].iter().all(|m| *m == Some(crate::helper_first::LIVE_WAIT)),
            "the readiness probe stays live"
        );
    }

    #[test]
    fn the_source_says_which_path_read_it() {
        let (mut r, _) = wear(true, vec![Ok(reply(vec![nvme()], vec![]))]);
        assert_eq!(HelperFirst::read(&mut r).via, ReadVia::Helper);
        let c = centre(&Net::new(false, vec![]), &FakeClock::new());
        let mut local = helper_first_wear_on(HelperHost::Linux, port(&c), Box::new(FakeWear(Reading::Available(vec![]))));
        assert_eq!(HelperFirst::read(&mut local).via, ReadVia::Local);
    }

    /// On macOS the agent runs its own smartctl as the person: the helper is never asked and no reason sends the person to turn it on.
    #[test]
    fn on_macos_the_helper_is_never_asked_and_no_reason_says_to_turn_it_on() {
        use crate::helper_first::testkit::{centre, port, Net};
        for up in [true, false] {
            let net = Net::new(up, vec![]);
            let c = centre(&net, &FakeClock::new());
            let local = FakeWear(Reading::unavailable("smartctl found no drives it can read."));
            let mut r = helper_first_wear_on(HelperHost::Macos, port(&c), Box::new(local));
            let got = HelperFirst::read(&mut r);
            let text = match &got.value {
                Reading::Unavailable(m) => m.clone(),
                other => panic!("{other:?}"),
            };
            assert!(text.contains("no drives"), "{text}");
            assert!(
                !text.contains("Administrator readings") && !text.contains("helper"),
                "helper up={up}: {text}"
            );
            assert!(
                !matches!(got.via, ReadVia::NeedsHelper { .. }) && !format!("{:?}", got.via).contains("OpenSettings"),
                "helper up={up}: {:?}",
                got.via
            );
        }
        let c = centre(&Net::new(true, vec![]), &FakeClock::new());
        let mut ok = helper_first_wear_on(HelperHost::Macos, port(&c), Box::new(FakeWear(Reading::Available(vec![]))));
        assert_eq!(HelperFirst::read(&mut ok).via, ReadVia::Local);
    }
}
