//! Behaviour tests: validation, filtering, HTTP channels against a local fake server, error paths.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

fn msg(kind: &str, severity: Severity) -> Message {
    Message {
        title: "Disk low".into(),
        body: "C: has 2 GB left".into(),
        severity,
        kind: kind.into(),
        source: "runner-1".into(),
    }
}

fn cfg(id: &str, transport: Transport) -> ChannelConfig {
    ChannelConfig {
        id: id.into(),
        name: String::new(),
        enabled: true,
        events: vec!["diskLow".into()],
        min_severity: Severity::Info,
        transport,
    }
}

fn webhook(url: &str, key: Option<&str>) -> Transport {
    Transport::Webhook {
        url: url.into(),
        secret_key: key.map(String::from),
    }
}

fn ntfy(server: &str, topic: &str, key: Option<&str>) -> Transport {
    Transport::Ntfy {
        server: server.into(),
        topic: topic.into(),
        secret_key: key.map(String::from),
    }
}

fn email(to: Vec<&str>) -> Transport {
    Transport::Email {
        host: "smtp.example.com".into(),
        port: 587,
        tls: Tls::StartTls,
        username: "bot".into(),
        secret_key: "smtp-pw".into(),
        from: "bot@example.com".into(),
        to: to.into_iter().map(String::from).collect(),
    }
}

/// Serves one request with `status`, returning the raw request text.
fn fake_http(status: u16) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = s.read(&mut chunk).unwrap_or(0);
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).to_string();
            if let Some(i) = text.find("\r\n\r\n") {
                let len = text[..i]
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                    })
                    .unwrap_or(0);
                if buf.len() >= i + 4 + len {
                    break;
                }
            }
            if n == 0 {
                break;
            }
        }
        let extra = if status / 100 == 3 {
            "Location: http://127.0.0.1:1/elsewhere\r\n"
        } else {
            ""
        };
        let _ = write!(s, "HTTP/1.1 {status} X\r\n{extra}Content-Length: 0\r\nConnection: close\r\n\r\n");
        let _ = tx.send(String::from_utf8_lossy(&buf).to_string());
    });
    (url, rx)
}

fn header<'a>(req: &'a str, name: &str) -> Option<&'a str> {
    req.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

fn one(results: Vec<(String, Result<(), SendError>)>) -> Result<(), SendError> {
    assert_eq!(results.len(), 1);
    results.into_iter().next().unwrap().1
}

#[test]
fn default_settings_have_no_channels_and_send_nothing() {
    let s = Settings::default();
    assert!(s.channels.is_empty());
    assert!(s.validate().is_ok());
    assert!(Dispatcher::new(s)
        .dispatch(&msg("diskLow", Severity::Error), &MemorySecrets::new())
        .is_empty());
}

#[test]
fn severity_is_ordered() {
    assert!(Severity::Info < Severity::Warning && Severity::Warning < Severity::Error);
}

#[test]
fn config_deserializes_with_defaults() {
    let json = r#"{"channels":[{"id":"a","transport":{"type":"ntfy","topic":"t"}},
        {"id":"b","enabled":true,"events":["x"],"minSeverity":"warning",
         "transport":{"type":"email","host":"h","port":25,"tls":"startTls","username":"u","secretKey":"k","from":"a@b.c","to":["d@e.f"]}}]}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert!(!s.channels[0].enabled && s.channels[0].events.is_empty());
    assert_eq!(s.channels[0].transport, ntfy("https://ntfy.sh", "t", None));
    assert_eq!(s.channels[1].min_severity, Severity::Warning);
    let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back, s);
    assert!(serde_json::from_str::<Settings>(r#"{"channels":[{"id":"a","transport":{"type":"fax"}}]}"#).is_err());
}

#[test]
fn configs_and_debug_output_hold_no_secrets() {
    let secrets = MemorySecrets::new().with("smtp-pw", "hunter2-super-secret");
    assert!(!format!("{secrets:?}").contains("hunter2"));
    let s = Settings {
        channels: vec![cfg("m", email(vec!["a@b.co"]))],
    };
    let text = format!("{s:?}{}", serde_json::to_string(&s).unwrap());
    assert!(text.contains("smtp-pw") && !text.contains("hunter2"));
}

fn invalid(s: Settings, needle: &str) {
    let e = s.validate().unwrap_err();
    assert!(e.contains(needle), "'{e}' should contain '{needle}'");
}

#[test]
fn validate_accepts_good_channels() {
    let s = Settings {
        channels: vec![
            cfg("w", webhook("https://hooks.example.com/x?y=1", Some("k"))),
            cfg("n", ntfy("http://192.168.1.5:8080", "my_topic-1", None)),
            cfg("m", email(vec!["a@b.co", "c@d.org"])),
        ],
    };
    assert_eq!(s.validate(), Ok(()));
}

#[test]
fn validate_rejects_bad_settings_with_actionable_messages() {
    let one = |c: ChannelConfig| Settings { channels: vec![c] };
    invalid(one(cfg("w", webhook("ftp://x.com", None))), "https://");
    invalid(one(cfg("w", webhook("https://", None))), "host");
    invalid(one(cfg("w", webhook("https://u@evil.com", None))), "host");
    invalid(
        one(cfg("w", webhook(&format!("https://a.com/{}", "x".repeat(3000)), None))),
        "too long",
    );
    invalid(one(cfg("w", webhook("https://a.com", Some("")))), "Secret name");
    invalid(one(cfg("n", ntfy("https://ntfy.sh", "bad topic", None))), "topic");
    invalid(one(cfg("n", ntfy("https://ntfy.sh", "", None))), "topic");
    invalid(one(cfg("n", ntfy("ntfy.sh", "t", None))), "ntfy server");
    invalid(one(cfg("", webhook("https://a.com", None))), "id");
    let mut c = cfg("w", webhook("https://a.com", None));
    c.name = "n\nl".into();
    invalid(one(c.clone()), "control");
    c.name = "n".repeat(81);
    invalid(one(c.clone()), "too long");
    c.name.clear();
    c.events = vec!["e".into(); 65];
    invalid(one(c), "at most 64");
    invalid(one(cfg("m", email(vec![]))), "recipient");
    invalid(one(cfg("m", email(vec!["nobody"]))), "valid email");
    invalid(one(cfg("m", email(vec!["a@b"]))), "valid email");
    invalid(one(cfg("m", email(vec!["a b@c.d"]))), "valid email");
    invalid(one(cfg("m", email(vec!["a@c.d"; 11]))), "At most 10");
    for (mutate, needle) in [
        (
            Box::new(|t: &mut Transport| {
                if let Transport::Email { host, .. } = t {
                    host.clear()
                }
            }) as Box<dyn Fn(&mut Transport)>,
            "host",
        ),
        (
            Box::new(|t| {
                if let Transport::Email { port, .. } = t {
                    *port = 0
                }
            }),
            "port",
        ),
        (
            Box::new(|t| {
                if let Transport::Email { secret_key, .. } = t {
                    secret_key.clear()
                }
            }),
            "Secret",
        ),
        (
            Box::new(|t| {
                if let Transport::Email { from, .. } = t {
                    *from = "x".into()
                }
            }),
            "Sender",
        ),
        (
            Box::new(|t| {
                if let Transport::Email { username, .. } = t {
                    *username = "u".repeat(300)
                }
            }),
            "too long",
        ),
    ] {
        let mut c = cfg("m", email(vec!["a@b.co"]));
        mutate(&mut c.transport);
        invalid(one(c), needle);
    }
    let many = Settings {
        channels: (0..9).map(|i| cfg(&format!("c{i}"), webhook("https://a.com", None))).collect(),
    };
    invalid(many, "At most 8");
    let mut named = cfg("w", webhook("ftp://x", None));
    named.name = "Ops hook".into();
    invalid(one(named), "Ops hook:");
    invalid(
        Settings {
            channels: vec![
                cfg("same", webhook("https://a.com", None)),
                cfg("same", webhook("https://b.com", None)),
            ],
        },
        "used twice",
    );
}

#[test]
fn filtering_by_enabled_event_and_severity() {
    let (url, rx) = fake_http(200);
    let mut c = cfg("w", webhook(&url, None));
    c.min_severity = Severity::Warning;
    let mut off = cfg("off", webhook(&url, None));
    off.enabled = false;
    let mut other = cfg("other", webhook(&url, None));
    other.events = vec!["jobFailed".into()];
    let mut none = cfg("none", webhook(&url, None));
    none.events.clear();
    let d = Dispatcher::new(Settings {
        channels: vec![c, off, other, none],
    });
    let secrets = MemorySecrets::new();
    assert!(d.dispatch(&msg("diskLow", Severity::Info), &secrets).is_empty());
    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    let r = d.dispatch(&msg("diskLow", Severity::Warning), &secrets);
    assert_eq!(r, vec![("w".to_string(), Ok(()))]);
    assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok());
}

#[test]
fn webhook_posts_json_with_bearer_token() {
    let (url, rx) = fake_http(204);
    let d = Dispatcher::new(Settings {
        channels: vec![cfg("w", webhook(&url, Some("tok")))],
    });
    let m = msg("diskLow", Severity::Error);
    assert_eq!(one(d.dispatch(&m, &MemorySecrets::new().with("tok", "s3cret"))), Ok(()));
    let req = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(req.starts_with("POST / "));
    assert_eq!(header(&req, "authorization"), Some("Bearer s3cret"));
    assert_eq!(header(&req, "content-type"), Some("application/json"));
    let body: Message = serde_json::from_str(req.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body, m);
    assert!(req.contains("\"severity\":\"error\""));
}

#[test]
fn webhook_without_secret_sends_no_authorization() {
    let (url, rx) = fake_http(200);
    let d = Dispatcher::new(Settings {
        channels: vec![cfg("w", webhook(&url, None))],
    });
    assert_eq!(one(d.dispatch(&msg("diskLow", Severity::Info), &MemorySecrets::new())), Ok(()));
    assert_eq!(header(&rx.recv_timeout(Duration::from_secs(5)).unwrap(), "authorization"), None);
}

#[test]
fn ntfy_posts_text_with_headers_and_token() {
    let (url, rx) = fake_http(200);
    let d = Dispatcher::new(Settings {
        channels: vec![cfg("n", ntfy(&format!("{url}/"), "alerts", Some("tk")))],
    });
    let mut m = msg("diskLow", Severity::Warning);
    m.title = "Disk\r\nlow".into();
    assert_eq!(one(d.dispatch(&m, &MemorySecrets::new().with("tk", "tk_abc"))), Ok(()));
    let req = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(req.starts_with("POST /alerts "));
    assert_eq!(header(&req, "title"), Some("Disk  low"));
    assert_eq!(header(&req, "priority"), Some("4"));
    assert_eq!(header(&req, "tags"), Some("warning"));
    assert_eq!(header(&req, "authorization"), Some("Bearer tk_abc"));
    assert!(req.ends_with("C: has 2 GB left"));
}

#[test]
fn ntfy_priority_follows_severity() {
    for (sev, prio) in [(Severity::Info, "3"), (Severity::Error, "5")] {
        let (url, rx) = fake_http(200);
        let d = Dispatcher::new(Settings {
            channels: vec![cfg("n", ntfy(&url, "t", None))],
        });
        assert_eq!(one(d.dispatch(&msg("diskLow", sev), &MemorySecrets::new())), Ok(()));
        assert_eq!(header(&rx.recv_timeout(Duration::from_secs(5)).unwrap(), "priority"), Some(prio));
    }
}

#[test]
fn server_errors_are_rejections() {
    let (url, _rx) = fake_http(500);
    let d = Dispatcher::new(Settings {
        channels: vec![cfg("w", webhook(&url, None))],
    });
    assert_eq!(
        one(d.dispatch(&msg("diskLow", Severity::Info), &MemorySecrets::new())),
        Err(SendError::Rejected(500))
    );
}

#[test]
fn redirects_are_not_followed() {
    let (url, _rx) = fake_http(302);
    let d = Dispatcher::new(Settings {
        channels: vec![cfg("w", webhook(&url, None))],
    });
    assert_eq!(
        one(d.dispatch(&msg("diskLow", Severity::Info), &MemorySecrets::new())),
        Err(SendError::Rejected(302))
    );
}

#[test]
fn missing_secret_is_reported_without_a_request() {
    let d = Dispatcher::new(Settings {
        channels: vec![
            cfg("w", webhook("http://127.0.0.1:1", Some("gone"))),
            cfg("n", ntfy("http://127.0.0.1:1", "t", Some("gone2"))),
        ],
    });
    let r = d.dispatch(&msg("diskLow", Severity::Info), &MemorySecrets::new());
    assert_eq!(r[0].1, Err(SendError::MissingSecret("gone".into())));
    assert_eq!(r[1].1, Err(SendError::MissingSecret("gone2".into())));
    assert!(r[0].1.as_ref().unwrap_err().to_string().contains("keychain"));
}

#[test]
fn connection_refused_is_an_error_not_a_panic() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let d = Dispatcher::new(Settings {
        channels: vec![cfg("w", webhook(&format!("http://127.0.0.1:{port}"), None))],
    });
    assert!(matches!(
        one(d.dispatch(&msg("diskLow", Severity::Info), &MemorySecrets::new())),
        Err(SendError::Connect(_))
    ));
}

#[test]
fn silent_server_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let hold = std::thread::spawn(move || {
        let conn = listener.accept();
        std::thread::sleep(Duration::from_millis(1500));
        drop(conn);
    });
    let d = Dispatcher::new(Settings {
        channels: vec![cfg("w", webhook(&url, None))],
    })
    .with_timeout(Duration::from_millis(300));
    assert_eq!(
        one(d.dispatch(&msg("diskLow", Severity::Info), &MemorySecrets::new())),
        Err(SendError::Timeout)
    );
    hold.join().unwrap();
}

#[test]
fn odd_input_never_panics() {
    let odd = "\u{0}\u{202e}\r\n💥".repeat(500);
    let m = Message {
        title: odd.clone(),
        body: odd.clone(),
        severity: Severity::Error,
        kind: odd.clone(),
        source: odd,
    };
    let d = Dispatcher::new(Settings {
        channels: vec![
            ChannelConfig {
                events: vec![m.kind.clone()],
                ..cfg("w", webhook("http://", None))
            },
            ChannelConfig {
                events: vec![m.kind.clone()],
                ..cfg("n", ntfy("not a url", "t\n", None))
            },
            ChannelConfig {
                events: vec![m.kind.clone()],
                ..cfg("m", email(vec!["bad address"]))
            },
        ],
    });
    let r = d.dispatch(&m, &MemorySecrets::new().with("smtp-pw", "x"));
    assert_eq!(r.len(), 3);
    assert!(r.iter().all(|(_, res)| res.is_err()));
}

#[test]
fn email_without_password_in_keychain_fails_clearly() {
    let d = Dispatcher::new(Settings {
        channels: vec![cfg("m", email(vec!["a@b.co"]))],
    });
    assert_eq!(
        one(d.dispatch(&msg("diskLow", Severity::Info), &MemorySecrets::new())),
        Err(SendError::MissingSecret("smtp-pw".into()))
    );
}

#[test]
fn email_to_unreachable_server_is_a_connect_error() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let mut t = email(vec!["a@b.co"]);
    if let Transport::Email { host, port: p, .. } = &mut t {
        *host = "127.0.0.1".into();
        *p = port;
    }
    let d = Dispatcher::new(Settings {
        channels: vec![cfg("m", t)],
    })
    .with_timeout(Duration::from_secs(3));
    assert!(matches!(
        one(d.dispatch(&msg("diskLow", Severity::Info), &MemorySecrets::new().with("smtp-pw", "x"))),
        Err(SendError::Connect(_)) | Err(SendError::Smtp(_))
    ));
}

#[test]
fn error_messages_are_user_facing() {
    assert!(SendError::Rejected(403).to_string().contains("403"));
    assert!(SendError::Timeout.to_string().contains("in time"));
    assert!(SendError::Panicked.to_string().contains("unexpectedly"));
}
