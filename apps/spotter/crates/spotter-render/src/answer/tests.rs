//! The judged wait against a fake DevTools peer on loopback (never a browser) and a scripted [`Life`]: no wall-clock
//! duration decides any verdict; the peer answers only when the script says so.

use super::*;
use crate::cdp::Connection;
use serde_json::json;
use std::net::TcpListener;
use std::sync::mpsc::{channel, Receiver, Sender};
use tungstenite::Message;

/// A loopback DevTools peer for one command: it answers once told (`Some(())`), or closes the connection (`None`, or the
/// sender dropped). Returns the connection to it and its thread.
fn peer(told: Receiver<Option<()>>) -> (Connection, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let req = loop {
            match ws.read() {
                Ok(Message::Text(t)) => break serde_json::from_str::<Value>(t.as_str()).unwrap(),
                Ok(_) => {}
                Err(_) => return,
            }
        };
        if let Ok(Some(())) = told.recv() {
            let reply = json!({"id": req["id"], "result": {"targetId": "T1"}});
            let _ = ws.send(Message::text(reply.to_string()));
        } else {
            let _ = ws.close(None);
            let _ = ws.flush();
        }
        // Keeps the connection until the client goes.
        while ws.read().is_ok() {}
    });
    (Connection::connect(port, "/devtools/browser/fake").unwrap(), h)
}

/// A browser whose liveness and CPU time follow a script, look by look.
struct Scripted {
    looks: u32,
    /// From this look on the browser has ended (`None`: never).
    ends_at: Option<u32>,
    /// At this look the peer is told to answer (`None`: never).
    answer_at: Option<u32>,
    tell: Option<Sender<Option<()>>>,
    /// The CPU time grows at every reading (a working browser) or stays (an idle one).
    working: bool,
    cpu: Duration,
}

impl Scripted {
    fn new(tell: Sender<Option<()>>) -> Scripted {
        Scripted {
            looks: 0,
            ends_at: None,
            answer_at: None,
            tell: Some(tell),
            working: true,
            cpu: Duration::ZERO,
        }
    }
}

impl Life for Scripted {
    fn liveness(&mut self) -> Liveness {
        self.looks += 1;
        if self.answer_at == Some(self.looks) {
            if let Some(t) = &self.tell {
                let _ = t.send(Some(()));
            }
        }
        if self.ends_at.is_some_and(|e| self.looks >= e) {
            return Liveness::Exited(ExitInfo {
                status: Some(1),
                stderr_tail: Some("crashed".into()),
            });
        }
        Liveness::Running
    }
    fn pid(&self) -> Option<ProcessId> {
        None
    }
    fn cpu_time(&mut self) -> Option<Duration> {
        if self.working {
            self.cpu += Duration::from_millis(1);
        }
        Some(self.cpu)
    }
}

/// Short ticks and an idle patience of a few ticks: the judge counts looks, so the test needs no long wall time.
fn patience() -> Patience {
    Patience {
        spin: Duration::from_secs(3600),
        idle: Duration::from_millis(20),
        hang_guard: Duration::from_secs(3600),
        note_after: 0,
        tick: Duration::from_millis(1),
        cpu_every: 1,
    }
}

#[test]
fn a_working_browser_is_waited_for_however_many_looks_its_answer_takes() {
    let (tx, rx) = channel();
    let (conn, h) = peer(rx);
    let pending = conn.send(None, "Target.createTarget", json!({})).unwrap();
    let mut life = Scripted::new(tx);
    life.answer_at = Some(6000);
    let quick = Patience {
        tick: Duration::ZERO,
        ..patience()
    };
    let r = await_answer(&pending, &mut life, quick);
    assert_eq!(r.unwrap()["targetId"], "T1");
    assert!(life.looks >= 6000, "answered after {} looks", life.looks);
    drop(pending);
    drop(conn);
    h.join().unwrap();
}

#[test]
fn a_browser_that_ended_fails_at_the_next_look_while_its_connection_stays_open() {
    let (tx, rx) = channel();
    let (conn, h) = peer(rx);
    let pending = conn.send(None, "Target.createTarget", json!({})).unwrap();
    let mut life = Scripted::new(tx);
    life.ends_at = Some(3);
    let r = await_answer(&pending, &mut life, patience());
    assert!(matches!(r, Err(Error::Closed)), "{r:?}");
    assert_eq!(life.looks, 3, "it ends at the look that saw the browser gone");
    assert!(!conn.is_closed(), "the connection was still open: the process told");
    drop(life.tell.take());
    drop(pending);
    drop(conn);
    h.join().unwrap();
}

#[test]
fn an_idle_browser_that_does_not_answer_is_stuck_not_timed_out() {
    let (tx, rx) = channel();
    let (conn, h) = peer(rx);
    let pending = conn.send(None, "Target.createTarget", json!({})).unwrap();
    let mut life = Scripted::new(tx);
    life.working = false;
    let r = await_answer(&pending, &mut life, patience());
    assert!(matches!(&r, Err(Error::Timeout(m)) if m.contains("stopped working")), "{r:?}");
    // Idle for 20 ms of ticks of 1 ms: about twenty looks, counted, not timed.
    assert!((20..=25).contains(&life.looks), "{} looks", life.looks);
    drop(life.tell.take());
    drop(pending);
    drop(conn);
    h.join().unwrap();
}

#[test]
fn a_connection_that_closes_ends_the_wait_as_closed() {
    let (tx, rx) = channel();
    let (conn, h) = peer(rx);
    let pending = conn.send(None, "Target.createTarget", json!({})).unwrap();
    let mut life = Scripted::new(tx);
    // The peer closes the connection instead of answering.
    life.tell.as_ref().unwrap().send(None).unwrap();
    let r = await_answer(&pending, &mut life, patience());
    assert!(matches!(r, Err(Error::Closed)), "{r:?}");
    drop(pending);
    drop(conn);
    h.join().unwrap();
}
