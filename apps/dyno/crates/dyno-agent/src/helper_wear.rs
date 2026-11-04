//! SSD wear through the helper first, then on this computer (Dyno ADR 0027 section 7, amended 2026-11-04 by the user: helper first,
//! unelevated fallback). With the helper on, every disk is read through it (it holds the rights Windows withholds); with it off or
//! unable, the local reader uses any smartmontools on the PC (Dyno's own, or the person's own install) for the disks the OS allows
//! without administrator rights and names the others.

use dyno_core::hardware::DiskWear;
use dyno_core::helper_requests::SmartQuery;
use dyno_core::metrics::Reading;
use dyno_core::ports::{DiskWearSource, PlatformError};
use dyno_core::smart::{SmartDisk, SmartReply};

/// The product choice behind the fallback: when the helper is off or cannot read, still read what this account may.
/// `false` would make the helper the only way to read SMART (the earlier decision O2); every other line stays the same.
pub const READ_LOCALLY_WHEN_HELPER_OFF: bool = true;

/// The helper, as far as SMART is concerned.
pub trait SmartLink: Send {
    /// What the helper reads, or `None` when the helper is not ready (off, absent, not answering).
    fn read_smart(&self) -> Option<Result<SmartReply, PlatformError>>;
}

/// The helper of a running agent, read through its `Smart` request when it is ready.
pub struct SharedSmart(pub std::sync::Weak<crate::agent::Shared>);

impl SmartLink for SharedSmart {
    fn read_smart(&self) -> Option<Result<SmartReply, PlatformError>> {
        use dyno_core::helper_wire::HelperState;
        let shared = self.0.upgrade()?;
        if shared.helper.status().state != HelperState::Ready {
            return None;
        }
        Some(shared.helper.link().smart(SmartQuery::default()))
    }
}

/// Reads through the helper when it is ready, else (or when it reads nothing) through `fallback`.
pub struct HelperFirstWear {
    link: Box<dyn SmartLink>,
    fallback: Box<dyn DiskWearSource>,
    fallback_enabled: bool,
}

impl HelperFirstWear {
    /// A reader over `link` with a local `fallback`.
    pub fn new(link: Box<dyn SmartLink>, fallback: Box<dyn DiskWearSource>) -> HelperFirstWear {
        HelperFirstWear {
            link,
            fallback,
            fallback_enabled: READ_LOCALLY_WHEN_HELPER_OFF,
        }
    }

    /// Chooses whether the local fallback is used (tests, and the seam for the product choice).
    pub fn with_fallback(mut self, on: bool) -> HelperFirstWear {
        self.fallback_enabled = on;
        self
    }

    fn local(&mut self, why: &str) -> Reading<Vec<DiskWear>> {
        if !self.fallback_enabled {
            return Reading::unavailable(format!(
                "{why} Turn on Administrator readings (Settings) so the helper can read the disks."
            ));
        }
        log::info!(target: "dyno::hardware", "SSD wear: reading on this computer instead ({why})");
        self.fallback.read()
    }
}

impl DiskWearSource for HelperFirstWear {
    fn read(&mut self) -> Reading<Vec<DiskWear>> {
        match self.link.read_smart() {
            None => self.local("the helper is not on"),
            Some(Err(e)) => {
                log::warn!(target: "dyno::hardware", "SSD wear: the helper could not read SMART: {e}");
                self.local(&format!("the helper could not read SMART: {e}"))
            }
            Some(Ok(reply)) => {
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
                    let why = reply.skipped.first().map_or_else(
                        || "the helper found no SSD with wear counters.".to_string(),
                        |s| format!("the helper skipped {}: {}", s.device, s.reason),
                    );
                    self.local(&why)
                } else {
                    Reading::Available(wear)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::smart::SmartSkip;
    use dyno_platform::hardware::FakeWear;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    struct Link(Option<Result<SmartReply, PlatformError>>);
    impl SmartLink for Link {
        fn read_smart(&self) -> Option<Result<SmartReply, PlatformError>> {
            self.0.clone()
        }
    }

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

    fn reply(disks: Vec<SmartDisk>, skipped: Vec<SmartSkip>) -> SmartReply {
        SmartReply {
            disks,
            skipped,
            not_visited: 0,
        }
    }

    #[test]
    fn a_ready_helper_with_one_nvme_disk_makes_the_wear_reading_available_without_asking_the_local_reader() {
        let calls = Arc::new(AtomicU32::new(0));
        let mut r = HelperFirstWear::new(
            Box::new(Link(Some(Ok(reply(vec![nvme()], vec![]))))),
            Box::new(Counting(calls.clone())),
        );
        let got = r.read();
        let w = &got.value().expect("available")[0];
        assert_eq!(
            (w.id.as_str(), w.percent_used, w.power_on_hours, w.bytes_written),
            ("EXAMPLE NVME EX1", Some(4.0), Some(1200), Some(84_800_000_000_000))
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn without_the_helper_or_when_it_reads_nothing_the_local_reader_is_used() {
        for link in [
            Link(None),
            Link(Some(Err(PlatformError::Unsupported("On macOS Dyno reads SMART itself.".into())))),
            Link(Some(Ok(reply(
                vec![],
                vec![SmartSkip {
                    device: "/dev/sda".into(),
                    reason: "x".into(),
                }],
            )))),
            Link(Some(Ok(reply(vec![SmartDisk { wear_id: None, ..nvme() }], vec![])))),
        ] {
            let calls = Arc::new(AtomicU32::new(0));
            let mut r = HelperFirstWear::new(Box::new(link), Box::new(Counting(calls.clone())));
            assert!(r.read().value().is_none());
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn the_local_fallback_can_be_switched_off_and_then_the_answer_says_what_to_turn_on() {
        let calls = Arc::new(AtomicU32::new(0));
        let mut r = HelperFirstWear::new(Box::new(Link(None)), Box::new(Counting(calls.clone()))).with_fallback(false);
        assert!(matches!(r.read(), Reading::Unavailable(m) if m.contains("Administrator readings")));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let mut ok = HelperFirstWear::new(Box::new(Link(None)), Box::new(FakeWear(Reading::Available(vec![])))).with_fallback(true);
        assert!(ok.read().value().is_some());
    }
}
