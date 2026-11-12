//! Per-program network traffic on Windows: the per-connection TCP statistics need administrator rights, so the helper reads them
//! (`Traffic`) and scopes them to the account that asks; without it Dyno asks the OS with the person's own rights and says why when
//! Windows refuses. Built on the seam ([`crate::helper_first`]); on macOS and Linux the local reader needs no helper and this is
//! never composed.

use crate::helper_first::{HelperCalls, HelperFirst, HelperPort, HelperReading, ReadClass};
use crate::helper_link::CallError;
use dyno_core::helper_policy::RequestKind;
use dyno_core::metrics::Reading;
use dyno_core::network_traffic::{TrafficAnswer, TrafficRead};
use dyno_core::ports::{PlatformError, ProcessTraffic};
use std::sync::Mutex;

/// The traffic of every program: the helper's answer for this caller, else this computer's own reader.
pub struct ProgramTraffic {
    local: Box<dyn ProcessTraffic>,
}

impl ProgramTraffic {
    /// A reading whose own-rights reader is `local`.
    pub fn new(local: Box<dyn ProcessTraffic>) -> ProgramTraffic {
        ProgramTraffic { local }
    }
}

impl HelperReading for ProgramTraffic {
    type Value = TrafficAnswer;
    const KINDS: &'static [RequestKind] = &[RequestKind::Traffic];

    fn name(&self) -> &'static str {
        "program traffic"
    }

    /// The Network page waits for it and it refreshes every few seconds.
    fn class(&self) -> ReadClass {
        ReadClass::Live
    }

    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<TrafficAnswer, CallError> {
        calls.traffic()
    }

    fn local(&mut self) -> Reading<TrafficAnswer> {
        match self.local.counters() {
            Ok(counters) => Reading::Available(TrafficAnswer { counters, others: None }),
            // The reason as a clause: the seam makes the sentence and adds what to do.
            Err(PlatformError::Unsupported(why) | PlatformError::AccessDenied(why)) => Reading::Unavailable(why),
            Err(e) => Reading::Unavailable(format!("Traffic per program could not be read ({e})")),
        }
    }
}

/// The traffic port the network centre reads: the seam over the helper, kept between reads (its strikes are per reading).
pub struct HelperTraffic {
    reading: Mutex<HelperFirst<ProgramTraffic>>,
}

impl HelperTraffic {
    /// Traffic over `port`, with `local` for when the helper cannot be asked.
    pub fn new(local: Box<dyn ProcessTraffic>, port: Box<dyn HelperPort>) -> HelperTraffic {
        HelperTraffic {
            reading: Mutex::new(HelperFirst::new(ProgramTraffic::new(local), port)),
        }
    }
}

impl ProcessTraffic for HelperTraffic {
    fn counters(&self) -> Result<Vec<dyno_core::network_traffic::TrafficCounter>, PlatformError> {
        self.read().answer.map(|a| a.counters)
    }

    fn read(&self) -> TrafficRead {
        let got = self.reading.lock().unwrap_or_else(std::sync::PoisonError::into_inner).read();
        let answer = match got.value {
            Reading::Available(a) => Ok(a),
            Reading::Unavailable(why) => Err(PlatformError::Unsupported(why)),
        };
        let via = got.via;
        TrafficRead { answer, via }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_first::testkit::{centre, port, Net};
    use crate::helper_link::TransportError;
    use dyno_core::helper_wire::HelperReply;
    use dyno_core::network_traffic::{OthersTraffic, TrafficCounter};
    use dyno_core::read_via::ReadVia;
    use paddock_jobs::time::FakeClock;

    struct Own(Result<Vec<TrafficCounter>, PlatformError>);
    impl ProcessTraffic for Own {
        fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
            self.0.clone()
        }
    }

    fn counter(pid: u32) -> TrafficCounter {
        TrafficCounter {
            pid,
            name: format!("p{pid}"),
            bytes_in: 1,
            bytes_out: 2,
        }
    }

    fn traffic(
        up: bool,
        answers: Vec<Result<HelperReply, TransportError>>,
        own: Result<Vec<TrafficCounter>, PlatformError>,
    ) -> HelperTraffic {
        let c = centre(&Net::new(up, answers), &FakeClock::new());
        HelperTraffic::new(Box::new(Own(own)), port(&c))
    }

    fn admin_only() -> PlatformError {
        PlatformError::Unsupported("Traffic per program needs administrator rights on Windows".into())
    }

    #[test]
    fn the_helper_answer_is_used_with_the_total_of_the_others() {
        let answer = TrafficAnswer {
            counters: vec![counter(10)],
            others: Some(OthersTraffic {
                programs: 2,
                bytes_in: 5,
                bytes_out: 6,
            }),
        };
        let t = traffic(true, vec![Ok(HelperReply::Traffic(answer.clone()))], Err(admin_only()));
        let got = t.read();
        assert_eq!(got.answer.unwrap(), answer);
        assert_eq!(got.via, ReadVia::Helper);
    }

    #[test]
    fn without_the_helper_windows_own_refusal_is_the_reason_and_the_button() {
        let got = traffic(false, vec![], Err(admin_only())).read();
        assert!(got.answer.is_err());
        assert!(
            matches!(&got.via, ReadVia::NeedsHelper { reason, .. } if reason.contains("administrator rights on Windows")),
            "{:?}",
            got.via
        );
    }

    #[test]
    fn where_the_os_reads_it_without_the_helper_it_is_local() {
        let got = traffic(false, vec![], Ok(vec![counter(1), counter(2)])).read();
        assert_eq!(got.answer.unwrap().counters.len(), 2);
        assert_eq!(got.via, ReadVia::Local);
    }

    #[test]
    fn a_helper_that_cannot_read_it_falls_back_to_the_own_reader() {
        let refused = TransportError::Remote {
            code: dyno_core::helper_wire::codes::FAILED.into(),
            message: "no".into(),
        };
        let got = traffic(true, vec![Err(refused)], Ok(vec![counter(1)])).read();
        assert_eq!(got.answer.unwrap().counters.len(), 1);
    }
}
