//! What keeps a Windows computer awake needs an administrator (`powercfg /requests`), so the helper reads it (`Extras`); without it Dyno tries the same tools with the person's own rights
//! and says why when Windows refuses. Both are built on the seam ([`crate::helper_first`]) and both parse the tool's text with
//! one parser, in `dyno_core::helper_extras` (rule 55).

use crate::helper_first::{Availability, HelperCalls, HelperFirst, HelperPort, HelperReading, ReadClass};
use crate::helper_link::CallError;
use dyno_core::extras::sleep::{blockers_from_requests, SleepBlocker};
use dyno_core::helper_extras::ExtrasAnswer;
use dyno_core::helper_policy::RequestKind;
use dyno_core::helper_requests::ExtrasKind;
use dyno_core::metrics::Reading;
use dyno_core::ports::PlatformError;
use std::sync::Arc;
use std::time::Duration;

/// Reads the blockers with this computer's own rights (the platform's `powercfg /requests` run).
pub type LocalWakes = Box<dyn Fn() -> Result<Vec<SleepBlocker>, PlatformError> + Send + Sync>;

fn none_set() -> PlatformError {
    PlatformError::Unsupported("This computer's own reader was not given.".into())
}

/// The wake requests: the helper's, else this computer's.
pub struct WakeRequests {
    local: LocalWakes,
    refused: fn(&PlatformError) -> String,
}

impl WakeRequests {
    /// A reading whose local reader is `local`; `refused` words its refusal.
    pub fn new(local: LocalWakes, refused: fn(&PlatformError) -> String) -> WakeRequests {
        WakeRequests { local, refused }
    }

    /// Gives the reading the local reader of the next read (the runner can change between reads).
    pub fn set_local(&mut self, local: LocalWakes) {
        self.local = local;
    }
}

impl HelperReading for WakeRequests {
    type Value = Vec<SleepBlocker>;
    const KINDS: &'static [RequestKind] = &[RequestKind::Extras];

    fn name(&self) -> &'static str {
        "wake requests"
    }

    /// Slow tool, read by a job with its own progress: the request kind's own wait.
    fn class(&self) -> ReadClass {
        ReadClass::Bulk
    }

    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<Vec<SleepBlocker>, CallError> {
        match calls.extras(ExtrasKind::WakeRequests)? {
            ExtrasAnswer::WakeRequests { items: Some(items), .. } => Ok(blockers_from_requests(&items)),
            ExtrasAnswer::WakeRequests { count, items: None } => Err(PlatformError::AccessDenied(format!(
                "{count} request{} keep the computer awake; only the account that turned on the helper, or an administrator, sees which",
                if count == 1 { "" } else { "s" }
            ))
            .into()),
        }
    }

    fn local(&mut self) -> Reading<Vec<SleepBlocker>> {
        match (self.local)() {
            Ok(v) => Reading::Available(v),
            Err(e) => Reading::Unavailable((self.refused)(&e)),
        }
    }
}

/// A port kept by several readings.
struct Handle(Arc<dyn HelperPort>);

impl HelperPort for Handle {
    fn availability(&self) -> Availability {
        self.0.availability()
    }
    fn epoch(&self) -> u64 {
        self.0.epoch()
    }
    fn calls(&self, class: ReadClass) -> Option<Arc<dyn HelperCalls>> {
        self.0.calls(class)
    }
    fn now(&self) -> Duration {
        self.0.now()
    }
}

/// Both readings over one port, kept by the extras centre between reads (the strikes of a slow helper are kept per reading).
pub struct HelperExtras {
    /// What keeps the computer awake.
    pub wakes: HelperFirst<WakeRequests>,
}

impl HelperExtras {
    /// The readings over `port`, with the platform's wording of a refusal.
    pub fn new(port: Arc<dyn HelperPort>, wake_refused: fn(&PlatformError) -> String) -> HelperExtras {
        HelperExtras {
            wakes: HelperFirst::new(
                WakeRequests::new(Box::new(|| Err(none_set())), wake_refused),
                Box::new(Handle(port.clone())),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_first::testkit::{centre, port, Net};
    use crate::helper_link::TransportError;
    use dyno_core::extras::sleep::Prevents;
    use dyno_core::helper_extras::parse_requests;
    use dyno_core::helper_wire::HelperReply;
    use dyno_core::read_via::ReadVia;
    use paddock_jobs::time::FakeClock;

    const REQUESTS: &str = "SYSTEM:\n[PROCESS] \\Device\\HarddiskVolume3\\Tools\\player.exe\nPlaying video\n\nDISPLAY:\nNone.\n";

    fn refused(e: &PlatformError) -> String {
        format!("Windows only lets an administrator list this ({e}).")
    }

    fn denied() -> Result<Vec<SleepBlocker>, PlatformError> {
        Err(PlatformError::AccessDenied("Access is denied.".into()))
    }

    fn wakes(up: bool, answers: Vec<Result<HelperReply, TransportError>>, local: LocalWakes) -> HelperFirst<WakeRequests> {
        let c = centre(&Net::new(up, answers), &FakeClock::new());
        HelperFirst::new(WakeRequests::new(local, refused), port(&c))
    }

    #[test]
    fn wake_requests_come_from_the_helper_when_powercfg_is_refused() {
        let answer = ExtrasAnswer::WakeRequests {
            count: 1,
            items: Some(parse_requests(REQUESTS)),
        };
        let got = wakes(true, vec![Ok(HelperReply::Extras(answer))], Box::new(denied)).read();
        let list = got.value.value().expect("the helper's list");
        assert_eq!(
            (list.len(), list[0].name.as_str(), list[0].prevents),
            (1, "player.exe", Prevents::Sleep)
        );
        assert_eq!(got.via, ReadVia::Helper);
    }

    #[test]
    fn without_the_helper_a_refusal_is_the_reason_and_the_next_step() {
        let got = wakes(false, vec![], Box::new(denied)).read();
        assert!(got.value.value().is_none());
        assert!(
            matches!(&got.via, ReadVia::NeedsHelper { reason, .. } if reason.contains("administrator list this")),
            "{:?}",
            got.via
        );
    }

    #[test]
    fn without_the_helper_the_own_rights_still_read_what_they_can() {
        let local: LocalWakes = Box::new(|| Ok(blockers_from_requests(&parse_requests(REQUESTS))));
        let got = wakes(false, vec![], local).read();
        assert_eq!(got.value.value().map(Vec::len), Some(1));
        assert_eq!(got.via, ReadVia::Local);
    }

    #[test]
    fn a_counts_only_caller_gets_the_count_and_is_told_why_the_list_is_missing() {
        let counted = ExtrasAnswer::WakeRequests { count: 2, items: None };
        let got = wakes(true, vec![Ok(HelperReply::Extras(counted))], Box::new(denied)).read();
        assert!(got.value.value().is_none());
        let reason = got.via.reason().unwrap_or_default();
        assert!(
            reason.contains("2 requests") && reason.contains("administrator list this"),
            "{reason}"
        );
    }

    /// A runner whose Windows tools all refuse, as a standard account sees them.
    struct Refusing;
    impl dyno_platform::runner::ToolRunner for Refusing {
        fn run_tool(&self, _: &dyno_platform::runner::Tool, _: Duration) -> Result<dyno_platform::runner::ToolOutput, PlatformError> {
            Err(PlatformError::AccessDenied("Access is denied.".into()))
        }
    }

    fn windows_centre(up: bool, answers: Vec<Result<HelperReply, TransportError>>) -> crate::extras::ExtrasCentre {
        let x = crate::extras::ExtrasCentre::new(false);
        x.use_os("windows");
        x.use_tools(Arc::new(Refusing));
        let c = centre(&Net::new(up, answers), &FakeClock::new());
        x.use_helper(Arc::new(crate::helper_first::CentrePort(c)));
        x
    }

    #[test]
    fn the_sleep_report_lists_the_requesters_the_helper_read_and_says_so() {
        let answer = ExtrasAnswer::WakeRequests {
            count: 1,
            items: Some(parse_requests(REQUESTS)),
        };
        let report = windows_centre(true, vec![Ok(HelperReply::Extras(answer))]).sleep_report();
        assert_eq!(report.blockers.len(), 1);
        assert_eq!(report.blockers_via, ReadVia::Helper);
        assert!(report.blockers_unavailable.is_none());
    }

    #[test]
    fn the_sleep_report_keeps_todays_sentence_and_asks_for_the_helper_when_it_is_off() {
        let report = windows_centre(false, vec![]).sleep_report();
        assert!(report.blockers.is_empty());
        let why = report.blockers_unavailable.unwrap_or_default();
        assert!(why.contains("Windows only lets an administrator list what keeps it awake"), "{why}");
        assert!(
            matches!(report.blockers_via, ReadVia::NeedsHelper { .. }),
            "{:?}",
            report.blockers_via
        );
    }

    #[test]
    fn another_os_is_read_as_before_and_is_local() {
        let x = crate::extras::ExtrasCentre::new(false);
        x.use_os("linux");
        x.use_tools(Arc::new(Refusing));
        x.use_helper(Arc::new(crate::helper_first::OffPort));
        assert_eq!(x.sleep_report().blockers_via, ReadVia::Local);
    }
}
