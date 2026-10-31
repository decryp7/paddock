//! The IPC server: accepts apps on the agent's private endpoint, answers their requests and feeds each subscriber
//! its own bounded queue (ADR 0002). Writes `agent.json` so the app can find the endpoint and token.
//!
//! # Local API versioning (design note, no behaviour here)
//!
//! The app protocol is [`PROTOCOL_VERSION`] plus the append-only rule in `dyno_core::protocol`: new `Request`, `Reply`
//! and `Message` variants go at the END of their enums, so an older peer reading a newer variant fails that one frame
//! (reported as a protocol error) instead of misreading others. Anything else (reordering, removing or changing a
//! variant, changing a field's type) bumps `PROTOCOL_VERSION`, which `connect` checks in the handshake.
//!
//! A command-line client (`dyno status`, `dyno capture`, `dyno alarms`...) is meant to be a second kind of peer on the
//! SAME endpoint: it reads the same discovery file (`agent.json`: endpoint, token, protocol version), connects with
//! the same token and speaks the same `Request`/`Reply` frames, so a request added for the app is available to the CLI
//! without new server code, and every handler keeps validating its arguments because it cannot know who is asking.
//! The CLI must therefore check `protocolVersion` in `agent.json` before connecting and say "update Dyno" when it is
//! newer than the CLI knows. The optional local web API (`Settings.webApi`) is a separate, opt-in adapter that maps
//! HTTP onto these same requests; it is not a second protocol.

use crate::agent::{lock, JobSlot, ScanSlot, Shared, Sub, VolumesSlot};
use crate::discovery::Discovery;
use crate::handlers::{handle, Conn};
use dyno_core::protocol::{AgentError, ErrorCode, Message, PROTOCOL_VERSION};
use dyno_ipc::{Host, IpcError, Peer, PeerOptions, RemoteError};
use std::sync::atomic::AtomicU8;
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

/// Frames waiting for a slow app, per connection.
const QUEUE: usize = 4;

/// Binds the endpoint and writes the discovery file. Returns the host and the file's content.
pub fn bind(shared: &Shared) -> std::io::Result<(Host, Discovery)> {
    let host = Host::listen()?;
    let d = Discovery {
        endpoint: host.name().to_string(),
        token: host.token().to_hex(),
        pid: std::process::id(),
        started: crate::agent::now().millis(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        protocol: PROTOCOL_VERSION,
    };
    d.write(&shared.dirs.discovery())?;
    Ok((host, d))
}

/// Accepts connections until the agent stops. Each connection gets a [`Peer`] whose handler answers requests.
pub fn serve(shared: &Arc<Shared>, mut host: Host) {
    let mut peers: Vec<(u64, Arc<Peer<Message>>, std::time::Instant)> = Vec::new();
    // The same fingerprint the app writes when it connects: a connect in the app's log and this run's lines are one story.
    let run = paddock_log::fingerprint(host.name());
    while !shared.stopped() {
        // A closed connection loses its subscription; dropping the sender ends its push thread.
        let gone: Vec<u64> = peers.iter().filter(|(_, p, _)| p.is_closed()).map(|(id, _, _)| *id).collect();
        if !gone.is_empty() {
            for (id, _, since) in peers.iter().filter(|(id, _, _)| gone.contains(id)) {
                log::info!(ConnectionId = *id, ConnectedMs = u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX); "Client disconnected");
            }
            peers.retain(|(id, _, _)| !gone.contains(id));
            lock(&shared.subs).retain(|s| !gone.contains(&s.id));
            shared.wake_sampler();
        }
        match host.accept(Duration::from_millis(400)) {
            Ok(conn) => match connect(shared, conn) {
                Ok((id, peer)) => {
                    log::info!(ConnectionId = id, AgentRun = run.as_str(), SinceStartMs = crate::logging::elapsed_ms(); "Client connected");
                    peers.push((id, peer, std::time::Instant::now()));
                }
                Err(e) => log::error!("dyno-agent: could not start a connection: {e}"),
            },
            Err(IpcError::Timeout(_)) => {}
            Err(e) => {
                // A client with the wrong token or version: refused, logged without content.
                log::error!("dyno-agent: refused a connection: {e}");
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    for (_, p, _) in &peers {
        p.close();
    }
    Discovery::remove_if_mine(&shared.dirs.discovery());
}

fn connect(shared: &Arc<Shared>, conn: dyno_ipc::Connection) -> Result<(u64, Arc<Peer<Message>>), IpcError> {
    let id = shared.new_conn_id();
    let detail = Arc::new(AtomicU8::new(0));
    let (tx, rx) = sync_channel::<Message>(QUEUE);
    let ctx = Conn { detail: detail.clone() };
    let s = shared.clone();
    let handler = move |m: Message| -> Result<Message, RemoteError> {
        match m {
            Message::Request(r) => Ok(Message::Reply(Box::new(handle(&s, &ctx, *r)))),
            _ => Err(RemoteError::new(
                dyno_ipc::wire::codes::UNSUPPORTED,
                "The agent only answers requests.",
            )),
        }
    };
    let peer = Arc::new(Peer::start(conn, PeerOptions::new().serve(handler).workers(4))?);
    let scans = Arc::new(ScanSlot::default());
    let volumes = Arc::new(VolumesSlot::default());
    let jobs = Arc::new(JobSlot::default());
    lock(&shared.subs).push(Sub {
        id,
        detail,
        tx,
        scans: scans.clone(),
        volumes: volumes.clone(),
        jobs: jobs.clone(),
    });
    // One thread per connection drains its queue; it ends when the app goes away or the Sub is dropped.
    let p = peer.clone();
    let s = shared.clone();
    std::thread::Builder::new()
        .name(format!("dyno-push-{id}"))
        .spawn(move || {
            pump(
                &rx,
                &Slots {
                    scans: &scans,
                    volumes: &volumes,
                    jobs: &jobs,
                },
                |m| p.push(m).is_ok(),
            );
            lock(&s.subs).retain(|x| x.id != id);
        })
        .map_err(|e| IpcError::io(e.kind(), format!("could not start the push thread: {e}")))?;
    Ok((id, peer))
}

/// What waits for one connection outside its queue.
pub(crate) struct Slots<'a> {
    pub scans: &'a ScanSlot,
    pub volumes: &'a VolumesSlot,
    pub jobs: &'a JobSlot,
}

/// Sends a connection's frames until the app goes away (`send` returns false) or the queue is closed. Disk scan statuses, the
/// volume list and job statuses do not travel through the queue: their messages there only wake this loop, and after every
/// message the newest of each is taken from its slot and sent.
pub(crate) fn pump(rx: &Receiver<Message>, slots: &Slots<'_>, mut send: impl FnMut(Message) -> bool) {
    while let Ok(m) = rx.recv() {
        if !matches!(m, Message::DiskScan(_) | Message::DiskVolumes(_) | Message::Job(_)) && !send(m) {
            return;
        }
        if let Some(v) = slots.volumes.take() {
            if !send(Message::DiskVolumes(v)) {
                return;
            }
        }
        for status in slots.scans.take() {
            if !send(Message::DiskScan(Box::new(status))) {
                return;
            }
        }
        for status in slots.jobs.take() {
            if !send(Message::Job(Box::new(status))) {
                return;
            }
        }
    }
}

/// The error a request gets when it can't be read at all.
pub fn unreadable() -> AgentError {
    AgentError::new(ErrorCode::BadRequest, "Dyno could not read that request.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::disk::scan::*;
    use std::sync::mpsc::sync_channel;

    fn status(scan: u64, seq: u64, state: ScanState) -> DiskScanStatus {
        DiskScanStatus {
            scan,
            seq,
            name: "x".into(),
            target: DiskTarget::Folder { path: "/x".into() },
            volume: None,
            state,
            phase: ScanPhase::ReadingFolders,
            progress: ScanProgress::default(),
            truncated: false,
            unreadable_first: vec![],
            needs_full_disk_access: false,
            items_per_second: None,
            eta_seconds: None,
            hidden_bytes: None,
            scanned_at: None,
        }
    }

    /// A slow app: its queue is full of other frames while a scan runs, ends and runs again. The final state must still arrive.
    #[test]
    fn a_final_scan_state_survives_a_full_queue() {
        let (tx, rx) = sync_channel::<Message>(QUEUE);
        let slot = ScanSlot::default();
        let alert = || Message::Request(Box::new(dyno_core::protocol::Request::Health));
        for _ in 0..QUEUE {
            tx.try_send(alert()).unwrap();
        }
        // Statuses arrive while the queue is full: their wake-ups are refused, as the real broadcast does.
        for (seq, state) in [
            (1, ScanState::Running),
            (2, ScanState::Running),
            (3, ScanState::Done),
            (4, ScanState::Running),
        ] {
            let s = status(7, seq, state);
            slot.put(s.clone());
            assert!(tx.try_send(Message::DiskScan(Box::new(s))).is_err(), "the queue is full");
        }
        slot.put(status(8, 1, ScanState::Cancelled));
        slot.put(status(7, 2, ScanState::Running)); // an older event arriving late changes nothing
        drop(tx);
        let mut got = Vec::new();
        pump(
            &rx,
            &Slots {
                scans: &slot,
                volumes: &VolumesSlot::default(),
                jobs: &JobSlot::default(),
            },
            |m| {
                got.push(m);
                true
            },
        );
        let scans: Vec<_> = got
            .iter()
            .filter_map(|m| {
                if let Message::DiskScan(s) = m {
                    Some((s.scan, s.seq))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(scans, vec![(7, 4), (8, 1)], "the newest event of each scan, once");
        assert_eq!(got.len(), QUEUE + 2, "the other frames are all there too");
    }

    #[test]
    fn the_final_state_wins_over_everything_before_it() {
        let slot = ScanSlot::default();
        slot.put(status(1, 5, ScanState::Done));
        slot.put(status(1, 4, ScanState::Running));
        let taken = slot.take();
        assert_eq!((taken.len(), taken[0].state.clone()), (1, ScanState::Done));
        assert!(slot.take().is_empty());
    }

    #[test]
    fn the_pump_stops_when_the_app_is_gone() {
        let (tx, rx) = sync_channel::<Message>(QUEUE);
        let slot = ScanSlot::default();
        slot.put(status(1, 1, ScanState::Done));
        tx.try_send(Message::DiskScan(Box::new(status(1, 1, ScanState::Done)))).unwrap();
        tx.try_send(Message::Request(Box::new(dyno_core::protocol::Request::Health)))
            .unwrap();
        pump(
            &rx,
            &Slots {
                scans: &slot,
                volumes: &VolumesSlot::default(),
                jobs: &JobSlot::default(),
            },
            |_| false,
        );
        slot.put(status(2, 1, ScanState::Done));
        tx.try_send(Message::Request(Box::new(dyno_core::protocol::Request::Health)))
            .unwrap();
        let mut sent = 0;
        pump(
            &rx,
            &Slots {
                scans: &slot,
                volumes: &VolumesSlot::default(),
                jobs: &JobSlot::default(),
            },
            |_| {
                sent += 1;
                false
            },
        );
        assert_eq!(sent, 1);
    }

    #[test]
    fn the_newest_volume_list_survives_a_full_queue_and_an_older_one_never_replaces_it() {
        let volume = |id: &str| dyno_core::disk::volume::Volume {
            id: id.into(),
            name: id.into(),
            mount: format!("/{id}"),
            fs: "apfs".into(),
            location: dyno_core::disk::volume::VolumeLocation::Internal,
            capacity: None,
            free: None,
            rotational: None,
            read_only: false,
            reachable: true,
            whole_volume: true,
        };
        let (tx, rx) = sync_channel::<Message>(QUEUE);
        let (scans, volumes) = (ScanSlot::default(), VolumesSlot::default());
        for _ in 0..QUEUE {
            tx.try_send(Message::Request(Box::new(dyno_core::protocol::Request::Health)))
                .unwrap();
        }
        volumes.put(3, vec![volume("new")]);
        volumes.put(2, vec![volume("older")]);
        assert!(
            tx.try_send(Message::DiskVolumes(Vec::new())).is_err(),
            "the queue is full: the wake-up is refused"
        );
        drop(tx);
        let mut got = Vec::new();
        pump(
            &rx,
            &Slots {
                scans: &scans,
                volumes: &volumes,
                jobs: &JobSlot::default(),
            },
            |m| {
                got.push(m);
                true
            },
        );
        let lists: Vec<Vec<String>> = got
            .iter()
            .filter_map(|m| {
                if let Message::DiskVolumes(v) = m {
                    Some(v.iter().map(|x| x.id.clone()).collect())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(lists, vec![vec!["new".to_string()]], "once, the newest listing's");
    }

    #[test]
    fn the_newest_status_of_each_job_survives_a_full_queue_and_a_final_state_is_never_dropped() {
        use paddock_jobs::wire::{WireSnapshot, WireState};
        let job = |id: u64, seq: u64, state: WireState| WireSnapshot {
            id,
            kind: "find-files".into(),
            title: "Finding files".into(),
            subject: None,
            state,
            seq,
            elapsed_ms: 0,
            eta_ms: None,
            cancellable: true,
            not_cancellable_reason: None,
        };
        let (tx, rx) = sync_channel::<Message>(QUEUE);
        for _ in 0..QUEUE {
            tx.try_send(Message::Request(Box::new(dyno_core::protocol::Request::Health)))
                .unwrap();
        }
        let jobs = JobSlot::default();
        for (seq, state) in [(1, WireState::Queued), (2, WireState::Running(None)), (3, WireState::Done)] {
            jobs.put(job(7, seq, state.clone()));
            assert!(
                tx.try_send(Message::Job(Box::new(job(7, seq, state)))).is_err(),
                "the queue is full"
            );
        }
        jobs.put(job(7, 2, WireState::Running(None))); // an older event arriving late changes nothing
        jobs.put(job(8, 1, WireState::Cancelled(paddock_jobs::CancelReason::User)));
        drop(tx);
        let mut got = Vec::new();
        pump(
            &rx,
            &Slots {
                scans: &ScanSlot::default(),
                volumes: &VolumesSlot::default(),
                jobs: &jobs,
            },
            |m| {
                got.push(m);
                true
            },
        );
        let seen: Vec<_> = got
            .iter()
            .filter_map(|m| {
                if let Message::Job(j) = m {
                    Some((j.id, j.seq, j.is_terminal()))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            seen,
            vec![(7, 3, true), (8, 1, true)],
            "the newest of each job, once, and the end of each"
        );
    }
}
