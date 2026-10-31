//! Executes the machine's commands through the ports (ADR 0025). It runs on the owner thread and never blocks: every slow
//! port (a look, a connect, a launch, ending a child, asking an agent to exit, a notification to the shell) runs on a worker
//! and answers with an event, so a quit is always taken at once.
//!
//! Workers: one *launcher* thread takes launch and end in order (an end never overtakes the launch it follows, so no child
//! is left behind); one *notifier* thread delivers the sink's notifications in order (the sink may block on the UI); a short
//! thread per look, connect, ask and connection watch. The connection is watched by waiting for its close, not by polling.

use super::machine::{Command, Event, Generation, Launch, PeerId, Purpose, Seq};
use super::ports::{self, now_ms};
use super::{AgentLink, Child, Stall, REQUEST_TIMEOUT};
use crate::dto::AgentStatus;
use crate::logging;
use dyno_agent::discovery::Discovery;
use dyno_core::protocol::{Message, Request};
use dyno_ipc::Peer;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a child gets to exit by itself when it is ended, before it is ended through its handle.
// cap: ADR 0031 class S. A quit must end, so this is the one deliberate time limit of the supervisor.
const END_GRACE: Duration = Duration::from_secs(3);

enum LauncherJob {
    Launch(Generation),
    End(Generation),
}

enum Notice {
    Agent(AgentStatus),
    Stalled(Option<Stall>),
}

type Peers = Arc<Mutex<HashMap<PeerId, Arc<Peer<Message>>>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The command executor of one link.
pub struct Executor {
    link: Arc<AgentLink>,
    events: Sender<Event>,
    launcher_jobs: Sender<LauncherJob>,
    notices: Sender<Notice>,
    peers: Peers,
    peer_ids: Arc<AtomicU64>,
    /// The generation the launcher holds, so a look tags its child view with it.
    held: Arc<Mutex<Option<Generation>>>,
    /// The discovery file seen by the last look, tagged with that look (what a connect and a removal may act on).
    seen: Arc<Mutex<Option<(Seq, Discovery)>>>,
    /// What the last told look showed, so a repeat is not told again.
    look_gate: Arc<Mutex<super::steps::ChangeGate>>,
    /// The last reason a connect attempt failed, so a repeat is not told again.
    connect_attempts: Arc<Mutex<super::steps::ConnectAttempts>>,
}

impl Executor {
    /// An executor for `link`; its workers answer on `events`.
    pub fn new(link: Arc<AgentLink>, events: Sender<Event>) -> std::io::Result<Executor> {
        let held = Arc::new(Mutex::new(None));
        let (launcher_jobs, jobs) = channel();
        let (notices, notices_rx) = channel();
        let (l, ev, h) = (link.clone(), events.clone(), held.clone());
        std::thread::Builder::new().name("dyno-launcher".into()).spawn(move || {
            while let Ok(job) = jobs.recv() {
                match job {
                    LauncherJob::Launch(child) => {
                        let at_ms = now_ms();
                        let launching = std::time::Instant::now();
                        let result = l.launcher.launch().map(|()| Launch {
                            at_ms,
                            pid: match l.launcher.child() {
                                Child::Running { pid } | Child::Stopped { pid } => Some(pid),
                                _ => None,
                            },
                        });
                        let launch_ms = u64::try_from(launching.elapsed().as_millis()).unwrap_or(u64::MAX);
                        match &result {
                            Ok(launch) => log::info!(Pid = launch.pid.unwrap_or(0), LaunchMs = launch_ms; "Agent launched"),
                            Err(e) => log::error!(LaunchMs = launch_ms, Error = e.as_str(); "Agent launch failed"),
                        }
                        if result.is_err() {
                            let mut held = lock(&h);
                            if *held == Some(child) {
                                *held = None;
                            }
                        }
                        let _ = ev.send(Event::Launched { child, result });
                    }
                    LauncherJob::End(child) => {
                        l.launcher.finish(END_GRACE);
                        let code = match l.launcher.child() {
                            Child::Exited { code } => code,
                            _ => None,
                        };
                        let mut held = lock(&h);
                        if *held == Some(child) {
                            *held = None;
                        }
                        drop(held);
                        let _ = ev.send(Event::ChildEnded { child, code });
                    }
                }
            }
        })?;
        let sink = link.sink.clone();
        std::thread::Builder::new().name("dyno-notify".into()).spawn(move || {
            while let Ok(n) = notices_rx.recv() {
                match n {
                    Notice::Agent(s) => sink.agent(s),
                    Notice::Stalled(s) => sink.stalled(s),
                }
            }
        })?;
        Ok(Executor {
            link,
            events,
            launcher_jobs,
            notices,
            peers: Arc::new(Mutex::new(HashMap::new())),
            peer_ids: Arc::new(AtomicU64::new(0)),
            held,
            seen: Arc::new(Mutex::new(None)),
            look_gate: Arc::new(Mutex::new(super::steps::ChangeGate::default())),
            connect_attempts: Arc::new(Mutex::new(super::steps::ConnectAttempts::new(std::time::Instant::now()))),
        })
    }

    /// Executes one command. Returns at once.
    pub fn execute(&mut self, command: Command) {
        match command {
            Command::Look {
                seq,
                service,
                cpu,
                lock: probe_lock,
                deciding,
            } => {
                let (link, events, held, seen) = (self.link.clone(), self.events.clone(), *lock(&self.held), self.seen.clone());
                let gate = self.look_gate.clone();
                spawn("dyno-look", move || {
                    let began = std::time::Instant::now();
                    let probe = if service { link.probe() } else { None };
                    let (look, found, service_ms) = ports::look(
                        seq,
                        &link.dirs,
                        &link.app_version,
                        link.launcher.as_ref(),
                        held,
                        ports::Ask {
                            probe: probe.as_ref(),
                            cpu,
                            lock: probe_lock,
                            deciding,
                        },
                    );
                    *lock(&seen) = found.map(|d| (seq, d));
                    let took = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
                    if let Some(line) = ports::describe_look(&look, deciding, took, service_ms) {
                        // Told when the state it shows changes (a slow world seen again and again is one line, not one per look).
                        // The key leaves out every number (ages and durations change on each look, the state does not).
                        let key: String = line
                            .split("; took ")
                            .next()
                            .unwrap_or(&line)
                            .chars()
                            .filter(|c| !c.is_ascii_digit())
                            .collect();
                        if lock(&gate).changed(&key) || deciding {
                            logging::info(&line);
                        }
                    }
                    let _ = events.send(Event::Looked(look));
                });
            }
            Command::Launch { child, .. } => {
                logging::info("starting the agent");
                *lock(&self.held) = Some(child);
                let _ = self.launcher_jobs.send(LauncherJob::Launch(child));
            }
            Command::EndHeldChild { child } => {
                logging::info("ending the agent this app started");
                let _ = self.launcher_jobs.send(LauncherJob::End(child));
            }
            Command::Connect { seq, look, purpose } => {
                let (link, events, peers, ids, seen) = (
                    self.link.clone(),
                    self.events.clone(),
                    self.peers.clone(),
                    self.peer_ids.clone(),
                    self.seen.clone(),
                );
                let attempts = self.connect_attempts.clone();
                spawn("dyno-connect", move || {
                    // A failure is told when its reason changes, not on every attempt (a wait retries on every look); the line
                    // carries which attempt it was and how long since the link began.
                    let failed = |why: &str| {
                        if let Some(f) = lock(&attempts).failed(why, std::time::Instant::now()) {
                            log::info!(Reason = f.reason.as_str(), Attempt = f.attempt, SinceBeginMs = f.since_begin_ms; "Connect attempt failed");
                        }
                    };
                    // The file of the look this connect is for; if that look was replaced, the file is read again (the agent may
                    // also have written a newer one since). Off the owner thread: it is file I/O.
                    let tagged = lock(&seen).clone().filter(|(s, _)| *s == look).map(|(_, d)| d);
                    let current = Discovery::read(&link.dirs.discovery()).or(tagged);
                    let Some(d) = current else {
                        failed("there is no discovery file");
                        let _ = events.send(Event::ConnectFailed {
                            seq,
                            why: "There is no discovery file.".into(),
                        });
                        return;
                    };
                    // A file of another build is never used, and one of this build is never asked to exit: the decision was
                    // taken from a file that may have been replaced.
                    if ports::hint_of(&d, &link.app_version).same_build != (purpose == Purpose::Use) {
                        failed("the discovery file changed");
                        let _ = events.send(Event::ConnectFailed {
                            seq,
                            why: "The discovery file changed.".into(),
                        });
                        return;
                    }
                    let handshake = std::time::Instant::now();
                    match ports::connect(&d, &link.sink) {
                        Ok(peer) => {
                            let done = lock(&attempts).succeeded(std::time::Instant::now());
                            log::info!(
                                Attempts = done.attempts,
                                SinceBeginMs = done.since_begin_ms,
                                AgentRun = paddock_log::fingerprint(&d.endpoint),
                                AgentPid = d.pid;
                                "{}",
                                ports::describe_connect(purpose, u64::try_from(handshake.elapsed().as_millis()).unwrap_or(u64::MAX))
                            );
                            let id = ids.fetch_add(1, Ordering::SeqCst) + 1;
                            lock(&peers).insert(id, peer.clone());
                            let watcher = (events.clone(), peer.clone());
                            if events
                                .send(Event::Connected {
                                    seq,
                                    peer: id,
                                    pid: Some(d.pid),
                                })
                                .is_err()
                            {
                                peer.close(); // the owner has ended
                                return;
                            }
                            let connected_at = std::time::Instant::now();
                            spawn("dyno-peer", move || {
                                watcher.1.wait_closed(None);
                                log::info!(
                                    ConnectedMs = u64::try_from(connected_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                                    "Agent connection closed"
                                );
                                let _ = watcher.0.send(Event::PeerClosed { peer: id });
                            });
                        }
                        Err(why) => {
                            failed(&why);
                            let _ = events.send(Event::ConnectFailed { seq, why });
                        }
                    }
                });
            }
            Command::AskToExit { peer } => {
                let peer = lock(&self.peers).remove(&peer);
                logging::info("asking the agent on this connection to exit");
                if let Some(p) = peer {
                    spawn("dyno-ask", move || {
                        let _ = p.call(Message::Request(Box::new(Request::Shutdown)), REQUEST_TIMEOUT);
                        p.close();
                    });
                }
            }
            Command::CloseConnection { peer } => {
                if let Some(p) = lock(&self.peers).remove(&peer) {
                    p.close();
                }
            }
            Command::RemoveHint { look } => {
                // Removed only if it is still that very file, and only the file of that look: a newer agent may have written
                // its own meanwhile. File I/O, so off the owner thread.
                let (link, seen) = (self.link.clone(), self.seen.clone());
                spawn("dyno-unhint", move || {
                    let tagged = lock(&seen).take().filter(|(s, _)| *s == look);
                    if let Some((_, d)) = tagged {
                        Discovery::remove_if_same(&link.dirs.discovery(), &d);
                    }
                });
            }
            Command::UsePeer(None) => *lock(&self.link.peer) = None,
            Command::UsePeer(Some(id)) => {
                let peer = lock(&self.peers).get(&id).cloned();
                *lock(&self.link.peer) = peer;
                logging::info("connected to the agent");
                let link = self.link.clone();
                // The shell reads its settings from the agent: off the owner thread, it makes a request.
                spawn("dyno-connected", move || link.on_connected());
            }
            Command::Report(status) => {
                let _ = self.notices.send(Notice::Agent(status));
            }
            Command::Stall { stall, message, .. } => {
                logging::info(&format!("still waiting: {message}"));
                let _ = self
                    .notices
                    .send(Notice::Agent(AgentStatus::new(crate::dto::AgentState::Starting, Some(message))));
                let _ = self.notices.send(Notice::Stalled(Some(stall)));
            }
            Command::ClearStall => {
                let _ = self.notices.send(Notice::Stalled(None));
            }
            Command::Done(outcome) => {
                // Closing never blocks (an atomic), so a launch queued before the quit sees it and does not run.
                self.link.launcher.close();
                logging::info(&format!(
                    "quitting; the agent {}",
                    if outcome.stopped_own {
                        "is stopped (this app started it)"
                    } else {
                        "keeps running"
                    }
                ));
            }
        }
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        // The owner ended: connections nobody uses are closed.
        for (_, p) in lock(&self.peers).drain() {
            p.close();
        }
    }
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new().name(name.into()).spawn(f);
}
