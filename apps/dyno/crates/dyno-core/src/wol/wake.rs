//! Sending a wake and waiting for the answer (ADR 0032 section 3), pure over ports: the sockets, the neighbour table, the ping,
//! the clock and Cancel are passed in, so the logic is tested with fakes and no datagram leaves the test (ER-14).
//!
//! ```text
//! Test only:  Checking --answers--> Done("already awake", nothing sent)
//! Sending (3 rounds, 1 s apart, every planned datagram) --> [Waiting for an answer, probe every 2 s] --> Done | TimedOut
//! Cancel in any phase --> Cancelled (what was sent is reported)
//! ```

use super::mac::{Mac, SecureOn};
use super::neighbours::{Neighbour, NeighbourState};
use super::packet::magic_packet;
use super::plan::SendPlan;
use crate::ports::PlatformError;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

/// How many times the whole plan is sent.
pub const ROUNDS: u8 = 3;
/// Seconds between rounds.
pub const ROUND_GAP_SECS: u32 = 1;
/// Seconds between probes while waiting for an answer.
pub const PROBE_EVERY_SECS: u32 = 2;
/// The shortest and longest wait of a test wake, in seconds.
pub const TEST_TIMEOUT_RANGE: (u32, u32) = (10, 600);

/// Sends one UDP datagram from a card's own address to a broadcast address.
pub trait UdpBroadcast: Send + Sync {
    /// The concrete adapter behind this port (for the wiring test and the agent's start log). Nobody overrides it.
    fn adapter(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
    /// Sends `payload` to `dest:port` from a socket bound to `source`. The error is a message for the report.
    fn send(&self, source: Ipv4Addr, dest: Ipv4Addr, port: u16, payload: &[u8]) -> Result<(), String>;
}

/// Reads the neighbour tables.
pub trait NeighbourSource: Send + Sync {
    /// The concrete adapter behind this port (for the wiring test and the agent's start log). Nobody overrides it.
    fn adapter(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
    /// Every entry with a usable hardware address; a refused or missing read is an error that says what would help.
    fn neighbours(&self) -> Result<Vec<Neighbour>, PlatformError>;
}

/// An unprivileged echo to one address.
pub trait Prober: Send + Sync {
    /// The concrete adapter behind this port (for the wiring test and the agent's start log). Nobody overrides it.
    fn adapter(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
    /// Whether `ip` answered one echo (bounded by a short timeout inside); `Err` when the OS cannot ping.
    fn ping(&self, ip: &str) -> Result<bool, String>;
}

/// The wake's clock and Cancel, and where its progress goes.
pub trait Pacer {
    /// Waits `secs` seconds; `false` when the wake was cancelled meanwhile.
    fn pause(&mut self, secs: u32) -> bool;
    /// Seconds since the wake began.
    fn elapsed_secs(&self) -> u32;
    /// The phase and its count: sends done of `total` while sending, seconds waited (no total) while waiting.
    fn progress(&mut self, phase: WakePhase, done: u32, total: Option<u32>);
}

/// The phases of a wake, each on its own scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakePhase {
    /// Looking whether it already answers.
    Checking,
    /// Sending datagrams.
    Sending,
    /// Waiting for the computer to answer.
    Waiting,
}

impl WakePhase {
    /// The id the job shows.
    pub fn id(self) -> &'static str {
        match self {
            WakePhase::Checking => "checking",
            WakePhase::Sending => "sending",
            WakePhase::Waiting => "waiting",
        }
    }

    /// The label the person reads.
    pub fn label(self) -> &'static str {
        match self {
            WakePhase::Checking => "Checking whether it is already awake",
            WakePhase::Sending => "Sending the wake packets",
            WakePhase::Waiting => "Waiting for an answer",
        }
    }
}

/// What one planned datagram did over all rounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    /// The card.
    pub interface: String,
    /// The destination address.
    pub dest: String,
    /// The payload size.
    pub bytes: u32,
    /// How many rounds went out.
    pub sent: u8,
    /// The last error, when a round failed.
    pub error: Option<String>,
}

/// How a woken computer showed itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnswerHow {
    /// It answered an echo.
    Ping,
    /// Its hardware address became reachable in the neighbour table.
    Neighbour,
}

/// An answer from the woken computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    /// How it answered.
    pub how: AnswerHow,
    /// Seconds after the wake began (0 when it answered before anything was sent).
    pub after_secs: u32,
    /// Its address now, when it differs from the one saved (DHCP moved it).
    pub new_ip: Option<String>,
}

/// What a wake did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WolOutcome {
    /// Every planned datagram.
    pub sent: Vec<SendResult>,
    /// The answer, for a test that got one.
    pub answered: Option<Answer>,
    /// A test wake (it waited for an answer) rather than a plain wake.
    pub tested: bool,
    /// Seconds from start to end.
    pub elapsed_secs: u32,
    /// The wait ran out.
    pub timed_out: bool,
    /// Cancel ended it.
    pub cancelled: bool,
    /// The result in words.
    pub message: String,
}

/// What a wake needs.
#[derive(Debug, Clone)]
pub struct WakeRequest {
    /// The computer's hardware address.
    pub mac: Mac,
    /// Its SecureOn password.
    pub secure_on: Option<SecureOn>,
    /// The address it had last time.
    pub last_ip: Option<String>,
    /// The datagrams to send.
    pub plan: Vec<SendPlan>,
    /// The UDP port.
    pub port: u16,
    /// `Some(seconds)` waits that long for an answer.
    pub test_timeout_secs: Option<u32>,
}

/// The ports a wake runs over.
pub struct WakePorts<'a> {
    /// The sockets.
    pub udp: &'a dyn UdpBroadcast,
    /// The neighbour tables.
    pub neighbours: &'a dyn NeighbourSource,
    /// The echo.
    pub prober: &'a dyn Prober,
}

/// What one look found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observation {
    /// The address at which its hardware address is `REACHABLE`.
    pub reachable_ip: Option<String>,
    /// An echo to its address was answered.
    pub ping_ok: bool,
    /// The address it appears under in the table, for a changed address.
    pub seen_ip: Option<String>,
}

/// The verdict on a look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// It answered.
    Answered(Answer),
    /// Not yet; keep waiting.
    Waiting,
    /// The time is up.
    TimedOut,
}

/// Judges a look `elapsed` seconds in. A ping beats a neighbour entry as evidence (it is the answer itself).
pub fn judge(elapsed: u32, timeout: u32, obs: &Observation, last_ip: Option<&str>) -> Verdict {
    let how = if obs.ping_ok {
        Some(AnswerHow::Ping)
    } else if obs.reachable_ip.is_some() {
        Some(AnswerHow::Neighbour)
    } else {
        None
    };
    match how {
        Some(how) => {
            let now_ip = obs.reachable_ip.as_deref().or(obs.seen_ip.as_deref()).or(last_ip);
            let new_ip = now_ip.filter(|ip| Some(*ip) != last_ip).map(str::to_string);
            Verdict::Answered(Answer {
                how,
                after_secs: elapsed,
                new_ip,
            })
        }
        None if elapsed >= timeout => Verdict::TimedOut,
        None => Verdict::Waiting,
    }
}

/// One look: the table first (it also gives a moved address), then an echo to the best address known.
fn observe(ports: &WakePorts<'_>, req: &WakeRequest) -> Observation {
    let mut obs = Observation::default();
    // A refused table is no evidence either way: the echo below still looks.
    if let Ok(all) = ports.neighbours.neighbours() {
        let mine: Vec<&Neighbour> = all.iter().filter(|n| n.mac == req.mac).collect();
        obs.reachable_ip = mine.iter().find(|n| n.state == NeighbourState::Reachable).map(|n| n.ip.clone());
        obs.seen_ip = mine.first().map(|n| n.ip.clone());
    }
    let target = req.last_ip.clone().or_else(|| obs.seen_ip.clone());
    if let Some(ip) = target {
        // No echo available (an error): the table alone decides.
        if let Ok(ok) = ports.prober.ping(&ip) {
            obs.ping_ok = ok;
        }
    }
    obs
}

/// Runs a wake. A plain wake only sends; a test also checks first (no packets when it already answers) and waits.
pub fn run_wake(req: &WakeRequest, ports: &WakePorts<'_>, pacer: &mut dyn Pacer) -> WolOutcome {
    let finish = |sent: Vec<SendResult>, answered: Option<Answer>, timed_out: bool, cancelled: bool, pacer: &dyn Pacer| {
        let elapsed_secs = pacer.elapsed_secs();
        let mut o = WolOutcome {
            sent,
            answered,
            tested: req.test_timeout_secs.is_some(),
            elapsed_secs,
            timed_out,
            cancelled,
            message: String::new(),
        };
        o.message = outcome_message(&o);
        o
    };
    let timeout = req.test_timeout_secs;
    if let Some(timeout) = timeout {
        pacer.progress(WakePhase::Checking, 0, None);
        let obs = observe(ports, req);
        if let Verdict::Answered(a) = judge(0, timeout, &obs, req.last_ip.as_deref()) {
            return finish(Vec::new(), Some(a), false, false, pacer);
        }
    }
    let payload = magic_packet(&req.mac, req.secure_on.as_ref());
    let mut results: Vec<SendResult> = req
        .plan
        .iter()
        .map(|p| SendResult {
            interface: p.interface.clone(),
            dest: p.dest.to_string(),
            bytes: payload.len() as u32,
            sent: 0,
            error: None,
        })
        .collect();
    let total = (req.plan.len() as u32) * u32::from(ROUNDS);
    let mut done = 0;
    pacer.progress(WakePhase::Sending, 0, Some(total));
    for round in 0..ROUNDS {
        for (i, p) in req.plan.iter().enumerate() {
            match ports.udp.send(p.source, p.dest, req.port, &payload) {
                Ok(()) => results[i].sent += 1,
                Err(e) => results[i].error = Some(e),
            }
            done += 1;
            pacer.progress(WakePhase::Sending, done, Some(total));
        }
        if round + 1 < ROUNDS && !pacer.pause(ROUND_GAP_SECS) {
            return finish(results, None, false, true, pacer);
        }
    }
    let Some(timeout) = timeout else {
        return finish(results, None, false, false, pacer);
    };
    if results.iter().all(|r| r.sent == 0) {
        // Nothing left any card: waiting would only run out the clock.
        return finish(results, None, false, false, pacer);
    }
    loop {
        let waited = pacer.elapsed_secs();
        pacer.progress(WakePhase::Waiting, waited, None);
        let obs = observe(ports, req);
        match judge(waited, timeout, &obs, req.last_ip.as_deref()) {
            Verdict::Answered(a) => return finish(results, Some(a), false, false, pacer),
            Verdict::TimedOut => return finish(results, None, true, false, pacer),
            Verdict::Waiting => {}
        }
        if !pacer.pause(PROBE_EVERY_SECS) {
            return finish(results, None, false, true, pacer);
        }
    }
}

/// The result in words, with the next step when it did not work.
pub fn outcome_message(o: &WolOutcome) -> String {
    if o.cancelled {
        return "Cancelled.".into();
    }
    if let Some(a) = &o.answered {
        let how = match a.how {
            AnswerHow::Ping => "answered ping",
            AnswerHow::Neighbour => "showed up on the network",
        };
        let mut s = if o.sent.is_empty() {
            format!("Already awake ({how}); nothing was sent.")
        } else {
            format!("Awake after {} s ({how}).", a.after_secs)
        };
        if a.new_ip.is_some() {
            s.push_str(" Its address changed; Dyno saved the new one.");
        }
        return s;
    }
    let sent: u32 = o.sent.iter().map(|r| u32::from(r.sent)).sum();
    if sent == 0 {
        return match o.sent.first().and_then(|r| r.error.as_deref()) {
            Some(e) => format!("Nothing was sent: {e}. Check that this computer is on the same network, then try again."),
            None => "Nothing was sent: no network card is connected. Connect to the network, then try again.".into(),
        };
    }
    if o.timed_out {
        return format!(
            "No answer in {} s: it may be off at the wall, have Wake-on-LAN off, or block ping.",
            o.elapsed_secs
        );
    }
    let failed = o.sent.iter().filter(|r| r.error.is_some()).count();
    if failed > 0 {
        format!(
            "Sent, but {failed} of {} sends failed. Dyno cannot tell whether it woke.",
            o.sent.len()
        )
    } else {
        "Wake packets sent. Dyno cannot tell whether it woke; use Test wake to find out.".into()
    }
}

#[cfg(test)]
mod tests {
    use super::super::fake::*;
    use super::*;

    const MAC: Mac = Mac([2, 0, 0x5e, 0x10, 0, 1]);

    fn plan() -> Vec<SendPlan> {
        vec![
            SendPlan {
                interface: "en0".into(),
                source: Ipv4Addr::new(192, 0, 2, 17),
                dest: Ipv4Addr::BROADCAST,
            },
            SendPlan {
                interface: "en0".into(),
                source: Ipv4Addr::new(192, 0, 2, 17),
                dest: Ipv4Addr::new(192, 0, 2, 255),
            },
        ]
    }

    fn req(test: Option<u32>) -> WakeRequest {
        WakeRequest {
            mac: MAC,
            secure_on: None,
            last_ip: Some("192.0.2.40".into()),
            plan: plan(),
            port: 9,
            test_timeout_secs: test,
        }
    }

    fn neighbour(state: NeighbourState, ip: &str) -> Neighbour {
        Neighbour {
            name: None,
            mac: MAC,
            ip: ip.into(),
            interface: "en0".into(),
            state,
        }
    }

    #[test]
    fn a_plain_wake_sends_every_planned_datagram_three_times_one_second_apart() {
        let (udp, nb, pr) = (FakeUdp::default(), FakeNeighbours::default(), FakeProber::default());
        let mut pacer = ManualPacer::default();
        let o = run_wake(
            &req(None),
            &WakePorts {
                udp: &udp,
                neighbours: &nb,
                prober: &pr,
            },
            &mut pacer,
        );
        let sent = udp.sent();
        assert_eq!(sent.len(), 6);
        assert!(sent.iter().all(|s| s.port == 9 && s.len == 102));
        assert_eq!(sent[0].dest, Ipv4Addr::BROADCAST);
        assert_eq!(sent[1].dest, Ipv4Addr::new(192, 0, 2, 255));
        assert_eq!(pacer.pauses, [1, 1]);
        assert_eq!(o.sent.len(), 2);
        assert!(o.sent.iter().all(|r| r.sent == 3 && r.error.is_none()));
        assert!(!o.tested && o.answered.is_none() && !o.timed_out);
        assert!(o.message.starts_with("Wake packets sent."));
        assert_eq!(pacer.last_progress, Some((WakePhase::Sending, 6, Some(6))));
        assert_eq!(pr.calls(), 0, "a plain wake never pings");
    }

    #[test]
    fn a_failing_card_is_reported_alone() {
        let udp = FakeUdp::failing_for(Ipv4Addr::new(192, 0, 2, 255), "network is unreachable");
        let (nb, pr) = (FakeNeighbours::default(), FakeProber::default());
        let o = run_wake(
            &req(None),
            &WakePorts {
                udp: &udp,
                neighbours: &nb,
                prober: &pr,
            },
            &mut ManualPacer::default(),
        );
        assert_eq!((o.sent[0].sent, o.sent[1].sent), (3, 0));
        assert_eq!(o.sent[1].error.as_deref(), Some("network is unreachable"));
        assert!(o.message.contains("1 of 2 sends failed"));
    }

    #[test]
    fn nothing_sent_says_why_and_does_not_wait() {
        let udp = FakeUdp::failing_for(Ipv4Addr::UNSPECIFIED, "x");
        let (nb, pr) = (FakeNeighbours::default(), FakeProber::default());
        let mut r = req(Some(120));
        r.plan.clear();
        let mut pacer = ManualPacer::default();
        let o = run_wake(
            &r,
            &WakePorts {
                udp: &udp,
                neighbours: &nb,
                prober: &pr,
            },
            &mut pacer,
        );
        assert!(o.message.starts_with("Nothing was sent: no network card"));
        assert!(!o.timed_out && pacer.elapsed_secs() < 10);
        let udp = FakeUdp::failing_all("permission denied");
        let o = run_wake(
            &req(Some(120)),
            &WakePorts {
                udp: &udp,
                neighbours: &nb,
                prober: &pr,
            },
            &mut ManualPacer::default(),
        );
        assert!(o.message.starts_with("Nothing was sent: permission denied"), "{}", o.message);
        assert!(!o.timed_out);
    }

    #[test]
    fn a_test_that_already_answers_sends_nothing() {
        let (udp, nb) = (FakeUdp::default(), FakeNeighbours::default());
        let pr = FakeProber::scripted(&[true]);
        let o = run_wake(
            &req(Some(120)),
            &WakePorts {
                udp: &udp,
                neighbours: &nb,
                prober: &pr,
            },
            &mut ManualPacer::default(),
        );
        assert!(udp.sent().is_empty());
        assert_eq!(
            o.answered,
            Some(Answer {
                how: AnswerHow::Ping,
                after_secs: 0,
                new_ip: None
            })
        );
        assert_eq!(o.message, "Already awake (answered ping); nothing was sent.");
    }

    #[test]
    fn a_test_waits_probing_every_two_seconds_until_a_ping_answers() {
        let udp = FakeUdp::default();
        let nb = FakeNeighbours::default();
        // check: no; waiting looks at 2 s (after 2 s of sends), 4 s, 6 s...: answer on the fourth look.
        let pr = FakeProber::scripted(&[false, false, false, false, true]);
        let mut pacer = ManualPacer::default();
        let o = run_wake(
            &req(Some(120)),
            &WakePorts {
                udp: &udp,
                neighbours: &nb,
                prober: &pr,
            },
            &mut pacer,
        );
        let a = o.answered.clone().unwrap();
        assert_eq!(a.how, AnswerHow::Ping);
        assert_eq!(a.after_secs, 2 + 2 * 3);
        assert_eq!(o.message, "Awake after 8 s (answered ping).");
        assert!(o.tested && !o.timed_out);
        assert_eq!(pacer.last_progress.map(|p| p.0), Some(WakePhase::Waiting));
        assert_eq!(pacer.last_progress.map(|p| p.2), Some(None));
    }

    #[test]
    fn a_reachable_neighbour_with_a_new_address_is_an_answer_and_saves_the_address() {
        let udp = FakeUdp::default();
        let nb = FakeNeighbours::scripted(vec![vec![], vec![], vec![neighbour(NeighbourState::Reachable, "192.0.2.77")]]);
        let pr = FakeProber::default();
        let o = run_wake(
            &req(Some(120)),
            &WakePorts {
                udp: &udp,
                neighbours: &nb,
                prober: &pr,
            },
            &mut ManualPacer::default(),
        );
        let a = o.answered.unwrap();
        assert_eq!(a.how, AnswerHow::Neighbour);
        assert_eq!(a.new_ip.as_deref(), Some("192.0.2.77"));
        assert!(o.message.contains("Its address changed"));
    }

    #[test]
    fn a_stale_entry_is_not_an_answer_and_the_wait_times_out_with_the_next_step() {
        let udp = FakeUdp::default();
        let nb = FakeNeighbours::scripted(vec![vec![neighbour(NeighbourState::Stale, "192.0.2.40")]]);
        let pr = FakeProber::default();
        let mut pacer = ManualPacer::default();
        let o = run_wake(
            &req(Some(30)),
            &WakePorts {
                udp: &udp,
                neighbours: &nb,
                prober: &pr,
            },
            &mut pacer,
        );
        assert!(o.timed_out && o.answered.is_none());
        assert!(o.elapsed_secs >= 30);
        assert!(o.message.starts_with("No answer in "));
        assert!(o.message.contains("off at the wall"));
    }

    #[test]
    fn cancel_works_while_sending_and_while_waiting() {
        let (udp, nb, pr) = (FakeUdp::default(), FakeNeighbours::default(), FakeProber::default());
        let mut pacer = ManualPacer::cancelling_at_pause(1);
        let o = run_wake(
            &req(Some(120)),
            &WakePorts {
                udp: &udp,
                neighbours: &nb,
                prober: &pr,
            },
            &mut pacer,
        );
        assert!(o.cancelled);
        assert_eq!(udp.sent().len(), 2, "only the first round went out");
        assert_eq!(o.message, "Cancelled.");
        let mut pacer = ManualPacer::cancelling_at_pause(3);
        let o = run_wake(
            &req(Some(120)),
            &WakePorts {
                udp: &udp,
                neighbours: &nb,
                prober: &pr,
            },
            &mut pacer,
        );
        assert!(o.cancelled && o.answered.is_none());
    }

    #[test]
    fn judge_prefers_ping_and_keeps_the_saved_address_when_nothing_moved() {
        let obs = Observation {
            reachable_ip: Some("192.0.2.40".into()),
            ping_ok: true,
            seen_ip: None,
        };
        assert_eq!(
            judge(5, 60, &obs, Some("192.0.2.40")),
            Verdict::Answered(Answer {
                how: AnswerHow::Ping,
                after_secs: 5,
                new_ip: None
            })
        );
        assert_eq!(judge(5, 60, &Observation::default(), None), Verdict::Waiting);
        assert_eq!(judge(60, 60, &Observation::default(), None), Verdict::TimedOut);
    }

    #[test]
    fn a_refused_neighbour_read_or_ping_falls_back_to_what_works() {
        let udp = FakeUdp::default();
        let nb = FakeNeighbours::failing();
        let pr = FakeProber::erroring();
        let o = run_wake(
            &req(Some(10)),
            &WakePorts {
                udp: &udp,
                neighbours: &nb,
                prober: &pr,
            },
            &mut ManualPacer::default(),
        );
        assert!(o.timed_out);
    }
}
