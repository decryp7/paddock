//! A tiny HTTP/1.1 server on the loopback interface for the tests of the real wire.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

pub struct Server {
    pub port: u16,
    pub accepted: Arc<AtomicUsize>,
    pub requests: Arc<std::sync::Mutex<Vec<String>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

/// What the server answers to a request: the raw response, or `None` to hold the connection without answering.
pub type Handler = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub fn respond(status: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut out = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", body.len());
    for (n, v) in headers {
        out.push_str(&format!("{n}: {v}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(body);
    out
}

impl Server {
    pub fn start(handler: Handler) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let handler = Arc::new(handler);
        let (a, rq, st) = (accepted.clone(), requests.clone(), stop.clone());
        let handle = std::thread::spawn(move || {
            let mut conns = Vec::new();
            while !st.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((s, _)) => {
                        a.fetch_add(1, Ordering::SeqCst);
                        s.set_nonblocking(false).unwrap();
                        let (h, rq, st) = (handler.clone(), rq.clone(), st.clone());
                        conns.push(std::thread::spawn(move || serve(s, &*h, &rq, &st)));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::yield_now()
                    }
                    Err(_) => break,
                }
            }
            for c in conns {
                let _ = c.join();
            }
        });
        Server {
            port,
            accepted,
            requests,
            stop,
            handle: Some(handle),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn serve(
    s: std::net::TcpStream,
    handler: &(dyn Fn(&str) -> Option<String> + Send + Sync),
    requests: &std::sync::Mutex<Vec<String>>,
    stop: &std::sync::atomic::AtomicBool,
) {
    s.set_read_timeout(Some(std::time::Duration::from_millis(100)))
        .unwrap();
    let mut w = s.try_clone().unwrap();
    let mut r = BufReader::new(s);
    while !stop.load(Ordering::SeqCst) {
        let mut head = String::new();
        let mut len = 0usize;
        loop {
            let mut line = String::new();
            match r.read_line(&mut line) {
                Ok(0) => return,
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    continue;
                }
                Err(_) => return,
            }
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap_or(0);
            }
            let end = line == "\r\n";
            head.push_str(&line);
            if end {
                break;
            }
        }
        let mut body = vec![0u8; len];
        if r.read_exact(&mut body).is_err() {
            return;
        }
        head.push_str(&String::from_utf8_lossy(&body));
        requests.lock().unwrap().push(head.clone());
        match handler(&head) {
            Some(reply) => {
                if w.write_all(reply.as_bytes()).is_err() {
                    return;
                }
            }
            None => {
                // Hold the connection open, unanswered, until the client gives up.
                while !stop.load(Ordering::SeqCst) {
                    let mut b = [0u8; 1];
                    match r.read(&mut b) {
                        Ok(0) => return,
                        Ok(_) => {}
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Err(_) => return,
                    }
                }
                return;
            }
        }
    }
}
