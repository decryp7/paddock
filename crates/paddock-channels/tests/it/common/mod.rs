//! Shared helpers for the integration tests: a clock that records waits, channel builders, an in-process HTTP server and SMTP
//! server on the loopback interface (no real network).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use paddock_channels::*;
use paddock_jobs::CancelToken;
use paddock_jobs::time::Clock;
use paddock_secrets::{MemorySecrets, Name, Secret, Secrets};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// A clock whose waits return at once and are recorded, so back-off is asserted without waiting.
#[derive(Default)]
pub struct RecordingClock {
    pub waits: Mutex<Vec<Duration>>,
}

impl RecordingClock {
    pub fn waits(&self) -> Vec<Duration> {
        self.waits.lock().unwrap().clone()
    }
}

impl Clock for RecordingClock {
    fn monotonic(&self) -> Duration {
        Duration::from_secs(
            self.waits
                .lock()
                .unwrap()
                .iter()
                .map(Duration::as_secs)
                .sum(),
        )
    }
    fn wall(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH
    }
    fn wait(&self, d: Duration, cancel: &CancelToken) -> Result<(), paddock_jobs::Cancelled> {
        cancel.check()?;
        self.waits.lock().unwrap().push(d);
        Ok(())
    }
}

pub fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

pub fn webhook(id: &str, auth: bool) -> ChannelConfig {
    ChannelConfig {
        id: id.into(),
        name: String::new(),
        enabled: true,
        events: vec!["diskLow".into()],
        min_severity: Severity::Info,
        transport: Transport::Webhook {
            url: "https://hooks.example.org/in?key=abc".into(),
            auth,
        },
    }
}

pub fn ntfy(id: &str, auth: bool) -> ChannelConfig {
    ChannelConfig {
        transport: Transport::Ntfy {
            server: "https://ntfy.example.org/".into(),
            topic: "alerts".into(),
            auth,
        },
        ..webhook(id, false)
    }
}

pub fn email(id: &str, username: Option<&str>) -> ChannelConfig {
    ChannelConfig {
        transport: Transport::Email {
            host: "mail.example.org".into(),
            port: 587,
            security: Security::StartTls,
            username: username.map(str::to_string),
            from: "bot@example.org".into(),
            to: vec!["a@example.org".into(), "b@example.org".into()],
        },
        ..webhook(id, false)
    }
}

pub fn message(kind: &str, severity: Severity) -> Message {
    Message::new(kind, severity, "Disk low", "C: has 2 GB left", "runner-1")
}

pub fn store() -> Arc<MemorySecrets> {
    Arc::new(MemorySecrets::new("TestApp"))
}

pub fn put(secrets: &MemorySecrets, cfg: &ChannelConfig, value: &str) {
    secrets
        .set(&cfg.secret_name().unwrap(), &Secret::new(value).unwrap())
        .unwrap();
}

pub struct Rig {
    pub dispatcher: Dispatcher,
    pub poster: Arc<FakePoster>,
    pub mailer: Arc<FakeMail>,
    pub clock: Arc<RecordingClock>,
    pub secrets: Arc<MemorySecrets>,
}

pub fn rig(
    channels: Vec<ChannelConfig>,
    poster: FakePoster,
    mailer: FakeMail,
    secrets: Arc<MemorySecrets>,
) -> Rig {
    let poster = Arc::new(poster);
    let mailer = Arc::new(mailer);
    let clock = Arc::new(RecordingClock::default());
    let dispatcher = Dispatcher::new(
        Settings { channels },
        secrets.clone(),
        poster.clone(),
        mailer.clone(),
        clock.clone(),
    );
    Rig {
        dispatcher,
        poster,
        mailer,
        clock,
        secrets,
    }
}

pub fn name(scope: &str, key: &str) -> Name {
    Name::new(scope, key).unwrap()
}

/// One accepted HTTP connection: the request head and body, answered with `reply` (a full response), or never answered.
pub fn http_server(reply: Option<&'static str>) -> (u16, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut w = s.try_clone().unwrap();
        let mut r = BufReader::new(s);
        let mut head = String::new();
        let mut len = 0usize;
        loop {
            let mut line = String::new();
            if r.read_line(&mut line).unwrap_or(0) == 0 {
                return head;
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
        let _ = r.read_exact(&mut body);
        head.push_str(&String::from_utf8_lossy(&body));
        match reply {
            Some(reply) => {
                let _ = w.write_all(reply.as_bytes());
            }
            None => {
                // Hold the connection open until the client gives up and closes it.
                let mut sink = [0u8; 1];
                let _ = r.read(&mut sink);
            }
        }
        head
    });
    (port, h)
}

/// What a scripted SMTP server answers.
#[derive(Clone, Copy)]
pub struct SmtpScript {
    pub auth: &'static [u8],
    pub rcpt: &'static [u8],
    pub greeting: &'static [u8],
    pub ehlo: &'static [u8],
}

impl SmtpScript {
    pub const OK: SmtpScript = SmtpScript {
        auth: b"235 ok\r\n",
        rcpt: b"250 ok\r\n",
        greeting: b"220 fake ESMTP\r\n",
        ehlo: b"250-fake\r\n250-AUTH PLAIN LOGIN\r\n250 8BITMIME\r\n",
    };
}

/// A scripted SMTP server accepting one mail; returns the client's lines.
pub fn smtp_server(script: SmtpScript) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut w = s.try_clone().unwrap();
        let mut r = BufReader::new(s);
        let mut seen = Vec::new();
        let mut in_data = false;
        let _ = w.write_all(script.greeting);
        loop {
            let mut line = String::new();
            if r.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let line = line.trim_end().to_string();
            seen.push(line.clone());
            if in_data {
                if line == "." {
                    in_data = false;
                    let _ = w.write_all(b"250 queued\r\n");
                }
                continue;
            }
            let up = line.to_uppercase();
            let reply: &[u8] = if up.starts_with("EHLO") {
                script.ehlo
            } else if up.starts_with("AUTH") {
                script.auth
            } else if up.starts_with("RCPT") {
                script.rcpt
            } else if up.starts_with("DATA") {
                in_data = true;
                b"354 go\r\n"
            } else if up.starts_with("QUIT") {
                let _ = w.write_all(b"221 bye\r\n");
                break;
            } else {
                b"250 ok\r\n"
            };
            let _ = w.write_all(reply);
        }
        seen
    });
    (port, h)
}
