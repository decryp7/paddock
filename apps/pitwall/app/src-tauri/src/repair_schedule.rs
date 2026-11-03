//! When the app checks that the OS registration of its service is right (macOS, rule 24). A check soon after the start,
//! then one a minute for a bounded number of times, because the service may only fail later: it restarts itself onto a new
//! binary when it is idle. The waits are retry back-off, not waits for a state, so they are legitimate delays; the clock is
//! injected (`wait`) so the tests wait for nothing.

use paddock_service::{RepairOutcome, Started};
use std::time::Duration;

/// How many times a run of the app checks the registration.
pub const ATTEMPTS: u32 = 8;

/// The wait before check number `attempt`: soon after the start, then every minute.
pub fn delay(attempt: u32) -> Duration {
    Duration::from_secs(if attempt == 0 { 3 } else { 60 })
}

/// Whether the checks can stop: the service is back, or only the user can go on (the OS asks for the approval).
pub fn done(started: &Started) -> bool {
    match started {
        Started::Repaired(_, RepairOutcome::Recovered | RepairOutcome::NeedsApproval) | Started::Offered(_) => true,
        // Left alone, current, busy with another repair, or a repair that did not work: the next check looks again.
        Started::Left(_) | Started::Marked | Started::Busy | Started::Repaired(..) => false,
    }
}

/// Runs the checks: `wait` pauses for each delay, `check` asks the OS and repairs what is stale (`None`: nothing can be
/// checked here). Returns how many checks ran.
pub fn run(wait: &mut dyn FnMut(Duration), check: &mut dyn FnMut() -> Option<Started>) -> u32 {
    for attempt in 0..ATTEMPTS {
        wait(delay(attempt));
        if check().is_some_and(|s| done(&s)) {
            return attempt + 1;
        }
    }
    ATTEMPTS
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_service::{Hold, Reason};

    #[test]
    fn checks_soon_then_every_minute() {
        assert_eq!(delay(0).as_secs(), 3);
        assert_eq!(delay(1).as_secs(), 60);
        const { assert!(ATTEMPTS >= 5) };
    }

    #[test]
    fn it_stops_when_the_service_is_back_and_waits_only_through_the_injected_clock() {
        let mut waits = Vec::new();
        let mut answers = vec![
            Some(Started::Marked),
            None,
            Some(Started::Repaired(Reason::VersionChanged, RepairOutcome::Failed("no".into()))),
            Some(Started::Repaired(Reason::VersionChanged, RepairOutcome::Recovered)),
        ]
        .into_iter();
        let ran = run(&mut |d| waits.push(d), &mut || answers.next().expect("no check after the service is back"));
        assert_eq!(ran, 4);
        assert_eq!(waits, [3, 60, 60, 60].map(Duration::from_secs));
    }

    #[test]
    fn it_gives_up_after_the_bounded_number_of_checks() {
        let mut waits = 0;
        let ran = run(&mut |_| waits += 1, &mut || Some(Started::Left(Hold::Healthy)));
        assert_eq!((ran, waits), (ATTEMPTS, ATTEMPTS));
    }
}
