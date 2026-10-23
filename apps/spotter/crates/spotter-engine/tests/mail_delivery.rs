//! E-mail delivery against a fake SMTP server on a local port: the message, sign-in, and every failure a user can hit.
//! No real mail is ever sent; nothing here leaves the machine.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use base64::Engine as _;
use spotter_core::notify::{SmtpSecurity, SmtpSettings};
use spotter_engine::mail::{send, send_within, MailAccount, MailError, MailMessage};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

#[derive(Clone, Default)]
struct Script {
    /// The sign-in the server accepts; `None` means no authentication is offered.
    auth: Option<(String, String)>,
    /// Advertise STARTTLS (the fake cannot complete it).
    starttls: bool,
    /// Refuse every recipient with this code.
    reject_rcpt: Option<u16>,
    /// Answer the greeting with this code instead of 220.
    greeting: Option<u16>,
    /// Accept the connection and never speak.
    hang: bool,
    /// Close the connection at once.
    drop_at_once: bool,
}

#[derive(Debug, Clone, Default)]
struct Mail {
    from: String,
    to: Vec<String>,
    data: String,
    authenticated_as: Option<String>,
}

fn decode(s: &str) -> String {
    String::from_utf8_lossy(&base64::engine::general_purpose::STANDARD.decode(s).unwrap_or_default()).into_owned()
}

async fn smtp(script: Script) -> (u16, Arc<Mutex<Vec<Mail>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Arc::new(Mutex::new(Vec::new()));
    let store = received.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { return };
            let (script, store) = (script.clone(), store.clone());
            tokio::spawn(async move {
                if script.drop_at_once {
                    return;
                }
                if script.hang {
                    tokio::time::sleep(Duration::from_secs(120)).await;
                    return;
                }
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                let greeting = script.greeting.unwrap_or(220);
                let _ = write.write_all(format!("{greeting} fake.test ESMTP\r\n").as_bytes()).await;
                let mut mail = Mail::default();
                while let Ok(Some(line)) = lines.next_line().await {
                    let upper = line.to_ascii_uppercase();
                    let reply = if upper.starts_with("EHLO") || upper.starts_with("HELO") {
                        let mut r = String::from("250-fake.test\r\n");
                        if script.starttls {
                            r.push_str("250-STARTTLS\r\n");
                        }
                        if script.auth.is_some() {
                            r.push_str("250-AUTH PLAIN LOGIN\r\n");
                        }
                        r.push_str("250 8BITMIME\r\n");
                        r
                    } else if upper.starts_with("AUTH PLAIN") {
                        let initial = match line.split_whitespace().nth(2) {
                            Some(x) => x.to_string(),
                            None => {
                                let _ = write.write_all(b"334 \r\n").await;
                                lines.next_line().await.ok().flatten().unwrap_or_default()
                            }
                        };
                        let decoded = decode(&initial);
                        let parts: Vec<&str> = decoded.split('\0').collect();
                        match (&script.auth, parts.as_slice()) {
                            (Some((u, p)), [_, user, pass]) if user == u && pass == p => {
                                mail.authenticated_as = Some((*user).to_string());
                                "235 ok\r\n".to_string()
                            }
                            _ => "535 5.7.8 bad credentials\r\n".to_string(),
                        }
                    } else if upper.starts_with("AUTH LOGIN") {
                        let _ = write.write_all(b"334 VXNlcm5hbWU6\r\n").await;
                        let user = decode(&lines.next_line().await.ok().flatten().unwrap_or_default());
                        let _ = write.write_all(b"334 UGFzc3dvcmQ6\r\n").await;
                        let pass = decode(&lines.next_line().await.ok().flatten().unwrap_or_default());
                        match &script.auth {
                            Some((u, p)) if user == *u && pass == *p => {
                                mail.authenticated_as = Some(user);
                                "235 ok\r\n".to_string()
                            }
                            _ => "535 5.7.8 bad credentials\r\n".to_string(),
                        }
                    } else if upper.starts_with("MAIL FROM") {
                        mail.from = line[10..].trim().to_string();
                        "250 ok\r\n".into()
                    } else if upper.starts_with("RCPT TO") {
                        match script.reject_rcpt {
                            Some(code) => format!("{code} 5.1.1 no such user\r\n"),
                            None => {
                                mail.to.push(line[8..].trim().to_string());
                                "250 ok\r\n".into()
                            }
                        }
                    } else if upper.starts_with("DATA") {
                        let _ = write.write_all(b"354 go ahead\r\n").await;
                        let mut data = String::new();
                        while let Ok(Some(l)) = lines.next_line().await {
                            if l == "." {
                                break;
                            }
                            data.push_str(&l);
                            data.push('\n');
                        }
                        mail.data = data;
                        store.lock().unwrap().push(mail.clone());
                        "250 queued\r\n".into()
                    } else if upper.starts_with("QUIT") {
                        let _ = write.write_all(b"221 bye\r\n").await;
                        return;
                    } else if upper.starts_with("RSET") || upper.starts_with("NOOP") {
                        "250 ok\r\n".into()
                    } else {
                        "502 not implemented\r\n".into()
                    };
                    let _ = write.write_all(reply.as_bytes()).await;
                }
            });
        }
    });
    (port, received)
}

fn account(port: u16, security: SmtpSecurity, user: Option<&str>, password: Option<&str>) -> MailAccount {
    MailAccount {
        settings: SmtpSettings {
            host: "127.0.0.1".into(),
            port,
            security,
            username: user.map(Into::into),
            from: "spotter@example.test".into(),
        },
        password: password.map(Into::into),
    }
}

fn message() -> MailMessage {
    MailMessage {
        to: "me@example.test".into(),
        subject: "Spotter: 1.2.3 is out".into(),
        body: "Tool 1.2.3\nhttps://example.test/tool".into(),
    }
}

#[tokio::test]
async fn a_message_is_delivered_with_its_subject_and_body() {
    let (port, received) = smtp(Script::default()).await;
    send(&account(port, SmtpSecurity::None, None, None), &message()).await.unwrap();
    let got = received.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    assert!(got[0].from.contains("spotter@example.test"), "{:?}", got[0]);
    assert!(got[0].to[0].contains("me@example.test"), "{:?}", got[0]);
    assert!(got[0].data.contains("Subject: Spotter: 1.2.3 is out"), "{}", got[0].data);
    assert!(got[0].data.contains("https://example.test/tool"), "{}", got[0].data);
    assert_eq!(got[0].authenticated_as, None);
}

#[tokio::test]
async fn the_account_signs_in_and_a_wrong_password_says_so() {
    let script = Script {
        auth: Some(("spotter".into(), "s3cret".into())),
        ..Script::default()
    };
    let (port, received) = smtp(script).await;
    send(&account(port, SmtpSecurity::None, Some("spotter"), Some("s3cret")), &message())
        .await
        .unwrap();
    assert_eq!(received.lock().unwrap()[0].authenticated_as.as_deref(), Some("spotter"));

    let e = send(&account(port, SmtpSecurity::None, Some("spotter"), Some("wrong")), &message())
        .await
        .unwrap_err();
    assert!(matches!(e, MailError::Auth(_)), "{e:?}");
    assert!(e.message().contains("app password"), "{e}");
    assert!(!e.message().contains("wrong"), "the password is never echoed");
    assert_eq!(received.lock().unwrap().len(), 1, "nothing was delivered by the failed attempt");
}

#[tokio::test]
async fn an_account_without_a_password_is_a_clear_error_before_any_connection() {
    let e = send(&account(1, SmtpSecurity::None, Some("spotter"), None), &message())
        .await
        .unwrap_err();
    assert_eq!(e, MailError::NoPassword);
    assert!(e.message().contains("Settings"));
    let e = send(&account(1, SmtpSecurity::None, Some("spotter"), Some("")), &message())
        .await
        .unwrap_err();
    assert_eq!(e, MailError::NoPassword);
}

#[tokio::test]
async fn bad_settings_and_addresses_are_refused_before_connecting() {
    let mut a = account(1, SmtpSecurity::None, None, None);
    a.settings.host = String::new();
    assert!(matches!(send(&a, &message()).await.unwrap_err(), MailError::Invalid(_)));
    let a = account(1, SmtpSecurity::None, None, None);
    for to in ["", "nobody", "a@b.test\r\nBcc: x@y.test"] {
        let m = MailMessage {
            to: to.into(),
            ..message()
        };
        assert!(matches!(send(&a, &m).await.unwrap_err(), MailError::Invalid(_)), "{to:?}");
    }
    let mut bad_from = account(1, SmtpSecurity::None, None, None);
    bad_from.settings.from = "Name <x@y.test>".into();
    assert!(matches!(send(&bad_from, &message()).await.unwrap_err(), MailError::Invalid(_)));
}

#[tokio::test]
async fn subjects_are_one_line_and_long_bodies_are_cut() {
    let (port, received) = smtp(Script::default()).await;
    let m = MailMessage {
        to: "me@example.test".into(),
        subject: format!("Line one\r\nBcc: attacker@example.test {}", "x".repeat(400)),
        body: "€".repeat(100_000),
    };
    send(&account(port, SmtpSecurity::None, None, None), &m).await.unwrap();
    let data = received.lock().unwrap()[0].data.clone();
    assert!(!data.contains("\nBcc:"), "no header injection: {data}");
    assert!(data.len() < 200_000, "the body was cut");
}

#[tokio::test]
async fn a_refused_recipient_is_reported_with_the_next_step() {
    let (port, _) = smtp(Script {
        reject_rcpt: Some(550),
        ..Script::default()
    })
    .await;
    let e = send(&account(port, SmtpSecurity::None, None, None), &message()).await.unwrap_err();
    assert!(matches!(e, MailError::Rejected(_)), "{e:?}");
    assert!(e.message().contains("addresses"), "{e}");
}

#[tokio::test]
async fn a_transient_server_refusal_is_a_failure_with_its_code() {
    let (port, _) = smtp(Script {
        reject_rcpt: Some(451),
        ..Script::default()
    })
    .await;
    let e = send(&account(port, SmtpSecurity::None, None, None), &message()).await.unwrap_err();
    assert!(matches!(&e, MailError::Failed(m) if m.contains("451")), "{e:?}");
}

#[tokio::test]
async fn starttls_is_required_and_a_server_without_it_is_a_tls_error() {
    let (port, received) = smtp(Script::default()).await;
    let e = send(&account(port, SmtpSecurity::StartTls, None, None), &message())
        .await
        .unwrap_err();
    assert!(matches!(e, MailError::Tls(_)), "{e:?}");
    assert!(e.message().contains("STARTTLS"), "{e}");
    assert!(
        received.lock().unwrap().is_empty(),
        "nothing is sent in plain text when TLS was asked for"
    );
}

#[tokio::test]
async fn implicit_tls_against_a_plain_server_is_a_tls_error() {
    let (port, received) = smtp(Script::default()).await;
    let e = send_within(
        &account(port, SmtpSecurity::Tls, None, None),
        &message(),
        Duration::from_secs(5),
        Duration::from_secs(10),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(e, MailError::Tls(_) | MailError::Timeout(_) | MailError::Connect(_)),
        "{e:?}"
    );
    assert!(received.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_unreachable_server_a_dropped_connection_and_a_silent_server_are_told_apart() {
    // Nothing listens on port 1.
    let e = send(&account(1, SmtpSecurity::None, None, None), &message()).await.unwrap_err();
    assert!(matches!(e, MailError::Connect(_)), "{e:?}");
    assert!(e.message().contains("127.0.0.1"));

    let (port, _) = smtp(Script {
        drop_at_once: true,
        ..Script::default()
    })
    .await;
    let e = send(&account(port, SmtpSecurity::None, None, None), &message()).await.unwrap_err();
    assert!(matches!(e, MailError::Connect(_) | MailError::Failed(_)), "{e:?}");

    let (port, _) = smtp(Script {
        hang: true,
        ..Script::default()
    })
    .await;
    let started = std::time::Instant::now();
    let e = send_within(
        &account(port, SmtpSecurity::None, None, None),
        &message(),
        Duration::from_secs(5),
        Duration::from_millis(400),
    )
    .await
    .unwrap_err();
    assert!(matches!(e, MailError::Timeout(_)), "{e:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(e.message().contains("in time"));
}

#[tokio::test]
async fn a_server_that_greets_with_an_error_is_a_failure() {
    let (port, _) = smtp(Script {
        greeting: Some(554),
        ..Script::default()
    })
    .await;
    let e = send(&account(port, SmtpSecurity::None, None, None), &message()).await.unwrap_err();
    assert!(!matches!(e, MailError::Auth(_)), "{e:?}");
    assert!(!e.message().is_empty());
}

#[test]
fn every_error_tells_the_user_what_to_do_and_hides_secrets() {
    let all = [
        MailError::Invalid("The recipient is not valid.".into()),
        MailError::NoPassword,
        MailError::Connect("smtp.example.test".into()),
        MailError::Tls("smtp.example.test".into()),
        MailError::Auth("smtp.example.test".into()),
        MailError::Rejected("code 550".into()),
        MailError::Timeout("smtp.example.test".into()),
        MailError::Failed("code 451".into()),
    ];
    for e in all {
        assert!(e.message().len() > 20, "{e:?}");
        assert_eq!(e.to_string(), e.message());
    }
    let a = account(25, SmtpSecurity::None, Some("u"), Some("hunter2"));
    assert!(!format!("{a:?}").contains("hunter2"), "the password is not in Debug output");
}

#[tokio::test]
async fn a_silent_server_is_stopped_by_the_total_limit_whatever_the_step_limit_is() {
    let (port, received) = smtp(Script {
        hang: true,
        ..Script::default()
    })
    .await;
    let started = std::time::Instant::now();
    let e = send_within(
        &account(port, SmtpSecurity::None, None, None),
        &message(),
        Duration::from_secs(30),
        Duration::from_millis(500),
    )
    .await
    .unwrap_err();
    assert!(matches!(e, MailError::Timeout(_)), "{e:?}");
    assert!(started.elapsed() < Duration::from_secs(10), "the total limit ended it");
    assert!(received.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_account_on_a_server_without_sign_in_is_an_account_error() {
    // The server offers no AUTH at all, but the settings name an account.
    let (port, _) = smtp(Script::default()).await;
    let e = send(&account(port, SmtpSecurity::None, Some("spotter"), Some("pw")), &message())
        .await
        .unwrap_err();
    assert!(matches!(e, MailError::Auth(_)), "{e:?}");
}
