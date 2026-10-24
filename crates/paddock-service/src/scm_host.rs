//! The Windows service host (plan X4b): what a [`Kind::SystemService`](crate::Kind) program does when the Service
//! Control Manager starts it with `--service` ([`crate::Verb::WindowsService`]). An app's data, GPU and keys belong to
//! the *user*, while a LocalSystem service lives in session 0 with another profile, so the service is a thin supervisor:
//! it finds the logged-on user's session, starts the program there with that user's token and environment (plus the
//! spec's supervised variable, so the program merges its `PATH` and watches for updates), starts it again when it exits
//! (after a pause, so a crash loop cannot spin) and moves it when the user's session changes. From Pitwall's
//! `service/windows.rs` and Pitcrew's `win_session.rs`.
//!
//! - **Which session.** [`pick_user_session`]: connected before disconnected (a disconnected Remote Desktop session
//!   keeps running its programs), the console before other sessions, then the lowest id; session 0 never. Over Remote
//!   Desktop the console shows the sign-in screen, which is why the console is not simply assumed.
//! - **Not LocalSystem** (development: the service runs as a user): `WTSQueryUserToken` is refused, the program starts
//!   as a plain child, which is in that user's session anyway ([`Target::NotSystem`]).
//! - **Stopping** asks the program first (its own `stop` verb, run in the user's session, bounded), waits a grace period
//!   and only then ends it, so work in flight can finish (Pitcrew's graceful stop, without a console trick).
//!
//! The loop is pure over the [`Sessions`] port and a clock, so every path is tested on every OS with a fake; the Win32
//! calls (`WTSEnumerateSessionsW`, `WTSQueryUserToken`, `CreateProcessAsUserW`, the SCM dispatcher) are the native glue
//! (`native::scm`, Windows only). Pitcrew's named-pipe SDDL is the app ↔ service link and belongs to `paddock-ipc` (X13).

use std::time::Duration;

/// A logged-on user's session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserSession {
    /// The session id.
    pub id: u32,
    /// Connected (active) rather than disconnected.
    pub active: bool,
}

/// The session the program runs in for the user (see the module documentation). `None` when nobody is logged on.
pub fn pick_user_session(console: Option<u32>, sessions: &[UserSession]) -> Option<u32> {
    sessions
        .iter()
        .filter(|s| s.id != 0)
        .min_by_key(|s| (!s.active, Some(s.id) != console, s.id))
        .map(|s| s.id)
}

/// Adds `NAME=value` to a Windows environment block (UTF-16 `A=1\0B=2\0\0`), keeping the rest; an entry of the same
/// name is replaced.
pub fn env_block_with(block: &[u16], entry: &str) -> Vec<u16> {
    let name: Vec<u16> = entry
        .split('=')
        .next()
        .unwrap_or("")
        .encode_utf16()
        .chain([u16::from(b'=')])
        .collect();
    let mut out = Vec::with_capacity(block.len() + entry.len() + 2);
    let mut i = 0;
    while i < block.len() && block[i] != 0 {
        let end = block[i..]
            .iter()
            .position(|&c| c == 0)
            .map_or(block.len(), |p| i + p);
        let var = &block[i..end];
        let same = var.len() >= name.len()
            && var[..name.len()]
                .iter()
                .zip(&name)
                .all(|(a, b)| lower(*a) == lower(*b));
        if !same {
            out.extend_from_slice(var);
            out.push(0);
        }
        i = end + 1;
    }
    out.extend(entry.encode_utf16());
    out.push(0);
    out.push(0);
    out
}

fn lower(c: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&c) {
        c + 32
    } else {
        c
    }
}

/// Where the program should run now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Nobody is logged on: run nothing.
    Nobody,
    /// The service is not LocalSystem (development): a plain child.
    NotSystem,
    /// This user session.
    Session(u32),
}

/// A program the host started.
pub trait HostedChild: Send {
    /// The session it runs in (`None` for a plain child).
    fn session(&self) -> Option<u32>;
    /// Whether it has exited.
    fn exited(&mut self) -> bool;
    /// Asks it to stop (its `stop` verb, bounded); `false` when the request could not be made.
    fn ask_stop(&mut self) -> bool;
    /// Ends it now.
    fn kill(&mut self);
}

/// The OS as the host sees it: sessions, starting the program, waiting.
pub trait Sessions: Send {
    /// Where the program should run now.
    fn target(&mut self) -> Target;
    /// Starts the program for `target` (never [`Target::Nobody`]).
    fn start(&mut self, target: Target) -> Result<Box<dyn HostedChild>, String>;
    /// Waits for the next control event or `timeout`, whichever comes first.
    fn next_event(&mut self, timeout: Duration) -> Option<Control>;
    /// Waits (stopping: the grace period's polls).
    fn pause(&mut self, how_long: Duration);
}

/// What the Service Control Manager tells the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Stop or shut down.
    Stop,
    /// A session was logged on, off, connected or disconnected.
    SessionChange,
}

/// The host's waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostTimings {
    /// How often the child and the sessions are looked at when nothing happens.
    pub poll: Duration,
    /// The pause between an exit (or a failed start) and the next start.
    pub restart_pause: Duration,
    /// How long a child asked to stop gets before it is ended.
    pub stop_grace: Duration,
}

impl Default for HostTimings {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(500),
            restart_pause: Duration::from_secs(10),
            stop_grace: Duration::from_secs(20),
        }
    }
}

/// What happened, for the service log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    /// Started for this target.
    Started(Target),
    /// The start failed; tried again after the pause.
    StartFailed(String),
    /// The program exited; started again after the pause.
    Exited,
    /// The user's session changed; the program is moved.
    Moved,
    /// Stopping: asked the program to stop.
    Stopping,
    /// It did not stop within the grace period and was ended.
    Killed,
}

/// Supervises the program until the SCM says stop (see the module documentation). `say` gets each event for the log.
/// Time is the sum of the waits it made, so the restart pause is measured on the port's clock.
pub fn supervise(sys: &mut dyn Sessions, t: &HostTimings, say: &mut dyn FnMut(HostEvent)) {
    let mut child: Option<Box<dyn HostedChild>> = None;
    let mut wait_left = Duration::ZERO;
    loop {
        let event = sys.next_event(t.poll);
        if event == Some(Control::Stop) {
            break;
        }
        wait_left = wait_left.saturating_sub(t.poll);
        let target = sys.target();
        if let Some(c) = child.as_mut() {
            let moved = match (target, c.session()) {
                (Target::Session(now), Some(was)) => now != was,
                (Target::Nobody, _) => true,
                _ => false,
            };
            if c.exited() {
                say(HostEvent::Exited);
                child = None;
                wait_left = t.restart_pause;
            } else if moved {
                say(HostEvent::Moved);
                stop(c.as_mut(), sys, t, say);
                child = None;
                wait_left = Duration::ZERO;
            }
        }
        if child.is_none() && wait_left.is_zero() && target != Target::Nobody {
            match sys.start(target) {
                Ok(c) => {
                    say(HostEvent::Started(target));
                    child = Some(c);
                }
                Err(e) => {
                    say(HostEvent::StartFailed(e));
                    wait_left = t.restart_pause;
                }
            }
        }
    }
    if let Some(mut c) = child {
        stop(c.as_mut(), sys, t, say);
    }
}

/// Asks the child to stop, waits up to the grace period, then ends it.
fn stop(
    c: &mut dyn HostedChild,
    sys: &mut dyn Sessions,
    t: &HostTimings,
    say: &mut dyn FnMut(HostEvent),
) {
    say(HostEvent::Stopping);
    if c.ask_stop() {
        let mut waited = Duration::ZERO;
        while waited < t.stop_grace {
            if c.exited() {
                return;
            }
            sys.pause(t.poll);
            waited += t.poll;
        }
    }
    if !c.exited() {
        say(HostEvent::Killed);
        c.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_remote_desktop_user_is_found_when_the_console_shows_the_sign_in_screen() {
        let s = |id, active| UserSession { id, active };
        assert_eq!(pick_user_session(Some(1), &[s(2, true)]), Some(2));
        assert_eq!(pick_user_session(None, &[s(2, true)]), Some(2));
        assert_eq!(pick_user_session(Some(1), &[]), None);
        assert_eq!(
            pick_user_session(Some(5), &[s(3, false), s(4, true)]),
            Some(4)
        );
        assert_eq!(
            pick_user_session(Some(5), &[s(4, true), s(5, true)]),
            Some(5)
        );
        assert_eq!(
            pick_user_session(Some(9), &[s(7, true), s(6, true)]),
            Some(6)
        );
        assert_eq!(pick_user_session(None, &[s(8, false)]), Some(8));
        assert_eq!(pick_user_session(Some(0), &[s(0, true)]), None);
    }

    #[test]
    fn an_environment_block_gains_one_entry_and_replaces_its_name() {
        let w = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        let text = |v: Vec<u16>| String::from_utf16_lossy(&v);
        assert_eq!(
            text(env_block_with(&w("A=1\0BB=22\0\0"), "PITWALL_SERVICE=1")),
            "A=1\0BB=22\0PITWALL_SERVICE=1\0\0"
        );
        assert_eq!(
            text(env_block_with(
                &w("pitwall_service=0\0B=2\0\0"),
                "PITWALL_SERVICE=1"
            )),
            "B=2\0PITWALL_SERVICE=1\0\0"
        );
        assert_eq!(text(env_block_with(&[0, 0], "X=1")), "X=1\0\0");
        assert_eq!(text(env_block_with(&[], "X=1")), "X=1\0\0");
        assert_eq!(
            text(env_block_with(&w("XY=1\0\0"), "X=1")),
            "XY=1\0X=1\0\0",
            "a longer name is another variable"
        );
        assert_eq!(
            text(env_block_with(&w("A=1"), "X=1")),
            "A=1\0X=1\0\0",
            "an unterminated block"
        );
    }

    /// One scripted world: the targets in turn (the last repeats), events in turn, start results, the children's exits.
    #[derive(Default)]
    struct World {
        targets: VecDeque<Target>,
        events: VecDeque<Option<Control>>,
        start_fails: u32,
        log: Arc<Mutex<Vec<String>>>,
        /// Polls after which each new child exits on its own (`None`: never).
        child_lives: Option<u32>,
        /// Whether children obey `stop`.
        obey_stop: bool,
        /// Whether the stop verb can be run.
        can_ask: bool,
        clock: Duration,
    }

    struct Kid {
        session: Option<u32>,
        log: Arc<Mutex<Vec<String>>>,
        lives: Option<u32>,
        asked: bool,
        obey: bool,
        can_ask: bool,
        dead: bool,
    }

    impl HostedChild for Kid {
        fn session(&self) -> Option<u32> {
            self.session
        }
        fn exited(&mut self) -> bool {
            if let Some(n) = self.lives.as_mut() {
                if *n == 0 {
                    self.dead = true;
                } else {
                    *n -= 1;
                }
            }
            self.dead || (self.asked && self.obey)
        }
        fn ask_stop(&mut self) -> bool {
            self.log.lock().unwrap().push("ask stop".into());
            self.asked = self.can_ask;
            self.can_ask
        }
        fn kill(&mut self) {
            self.log.lock().unwrap().push("kill".into());
            self.dead = true;
        }
    }

    impl Sessions for World {
        fn target(&mut self) -> Target {
            if self.targets.len() > 1 {
                self.targets.pop_front().unwrap_or(Target::Nobody)
            } else {
                self.targets.front().copied().unwrap_or(Target::Nobody)
            }
        }
        fn start(&mut self, target: Target) -> Result<Box<dyn HostedChild>, String> {
            if self.start_fails > 0 {
                self.start_fails -= 1;
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("start failed {target:?}"));
                return Err("no token".into());
            }
            self.log
                .lock()
                .unwrap()
                .push(format!("start {target:?} at {}", self.clock.as_millis()));
            Ok(Box::new(Kid {
                session: match target {
                    Target::Session(s) => Some(s),
                    _ => None,
                },
                log: self.log.clone(),
                lives: self.child_lives,
                asked: false,
                obey: self.obey_stop,
                can_ask: self.can_ask,
                dead: false,
            }))
        }
        fn next_event(&mut self, timeout: Duration) -> Option<Control> {
            self.clock += timeout;
            self.events.pop_front().unwrap_or(Some(Control::Stop))
        }
        fn pause(&mut self, how_long: Duration) {
            self.clock += how_long;
        }
    }

    fn run(w: &mut World) -> (Vec<String>, Vec<HostEvent>) {
        let mut said = Vec::new();
        supervise(w, &HostTimings::default(), &mut |e| said.push(e));
        let log = w.log.lock().unwrap().clone();
        (log, said)
    }

    fn polls(n: usize) -> VecDeque<Option<Control>> {
        std::iter::repeat_n(None, n).collect()
    }

    #[test]
    fn the_program_starts_in_the_users_session_and_stops_gracefully_with_the_service() {
        let mut w = World {
            targets: [Target::Session(2)].into(),
            events: polls(3),
            obey_stop: true,
            can_ask: true,
            ..World::default()
        };
        let (log, said) = run(&mut w);
        assert_eq!(log, ["start Session(2) at 500", "ask stop"]);
        assert_eq!(
            said,
            [HostEvent::Started(Target::Session(2)), HostEvent::Stopping]
        );
    }

    #[test]
    fn a_program_that_ignores_stop_is_ended_after_the_grace_period() {
        let mut w = World {
            targets: [Target::Session(2)].into(),
            events: polls(1),
            can_ask: true,
            ..World::default()
        };
        let (log, said) = run(&mut w);
        assert_eq!(log, ["start Session(2) at 500", "ask stop", "kill"]);
        assert!(said.contains(&HostEvent::Killed));
        assert!(
            w.clock >= HostTimings::default().stop_grace,
            "the grace period was given"
        );
        // The stop verb cannot even be run: ended at once.
        let mut w = World {
            targets: [Target::Session(2)].into(),
            events: polls(1),
            ..World::default()
        };
        let before = Duration::from_millis(1000);
        let (log, _) = run(&mut w);
        assert_eq!(log, ["start Session(2) at 500", "ask stop", "kill"]);
        assert_eq!(w.clock, before, "no grace wait without a stop request");
    }

    #[test]
    fn an_exit_is_restarted_after_the_pause_so_a_crash_loop_cannot_spin() {
        let t = HostTimings::default();
        let n = (t.restart_pause.as_millis() / t.poll.as_millis()) as usize;
        let mut w = World {
            targets: [Target::Session(3)].into(),
            events: polls(n + 4),
            child_lives: Some(0),
            obey_stop: true,
            can_ask: true,
            ..World::default()
        };
        let (log, said) = run(&mut w);
        let starts: Vec<&String> = log.iter().filter(|l| l.starts_with("start ")).collect();
        assert_eq!(starts.len(), 2, "{log:?}");
        let at = |s: &str| s.rsplit(' ').next().unwrap().parse::<u64>().unwrap();
        assert!(at(starts[1]) - at(starts[0]) >= t.restart_pause.as_millis() as u64);
        assert!(said.contains(&HostEvent::Exited));
    }

    #[test]
    fn a_session_change_moves_the_program_and_nobody_logged_on_runs_nothing() {
        let mut w = World {
            targets: [Target::Session(1), Target::Session(1), Target::Session(4)].into(),
            events: [None, Some(Control::SessionChange), None].into(),
            obey_stop: true,
            can_ask: true,
            ..World::default()
        };
        let (log, said) = run(&mut w);
        assert_eq!(
            log,
            [
                "start Session(1) at 500",
                "ask stop",
                "start Session(4) at 1500",
                "ask stop"
            ]
        );
        assert!(said.contains(&HostEvent::Moved));
        let mut w = World {
            targets: [Target::Session(1), Target::Nobody].into(),
            events: polls(4),
            obey_stop: true,
            can_ask: true,
            ..World::default()
        };
        let (log, _) = run(&mut w);
        assert_eq!(
            log,
            ["start Session(1) at 500", "ask stop"],
            "logged off: stopped, nothing started"
        );
    }

    #[test]
    fn a_failed_start_is_retried_after_the_pause_and_a_development_service_starts_a_plain_child() {
        let t = HostTimings::default();
        let n = (t.restart_pause.as_millis() / t.poll.as_millis()) as usize;
        let mut w = World {
            targets: [Target::NotSystem].into(),
            events: polls(n + 2),
            start_fails: 1,
            obey_stop: true,
            can_ask: true,
            ..World::default()
        };
        let (log, said) = run(&mut w);
        assert_eq!(
            log.first().map(String::as_str),
            Some("start failed NotSystem")
        );
        assert!(
            log.iter().any(|l| l.starts_with("start NotSystem")),
            "{log:?}"
        );
        assert!(matches!(&said[0], HostEvent::StartFailed(e) if e == "no token"));
        // Stop arrives before anything ran: nothing to stop.
        let mut w = World::default();
        assert_eq!(run(&mut w), (vec![], vec![]));
        assert_eq!(HostTimings::default().poll, Duration::from_millis(500));
    }
}
