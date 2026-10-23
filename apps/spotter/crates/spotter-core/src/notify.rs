//! Notification policy: when a new release is worth telling the user about.

use crate::time::QuietHours;
use serde::{Deserialize, Serialize};

/// Which releases notify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotifyMode {
    /// Every release newer than the last one acknowledged.
    #[default]
    EveryNew,
    /// Only releases newer than the version the user has installed (every new one while that is unknown).
    NewerThanInstalled,
    /// Never (the watch still shows new releases).
    Off,
}

/// A watch's notification policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NotificationPolicy {
    /// Which releases notify.
    #[serde(default)]
    pub mode: NotifyMode,
    /// Repeat the notification at every check until the release is acknowledged (the old app's "naggy" switch).
    #[serde(default)]
    pub nag: bool,
    /// Hours when notifications wait (they are delivered at the first check after).
    #[serde(default)]
    pub quiet_hours: Option<QuietHours>,
}

/// Most extra channels per watch.
pub const MAX_CHANNELS: usize = 5;

/// The kind of an extra notification channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChannelKind {
    /// An HTTP(S) webhook that receives a JSON POST.
    Webhook,
    /// An ntfy topic (`https://ntfy.sh/topic` or a bare topic name).
    Ntfy,
    /// An e-mail address.
    Email,
}

/// An extra place to send a notification to (off unless added). Secrets (SMTP password, ntfy token) are never stored
/// here; they live in the credential store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationChannel {
    /// What kind.
    pub kind: ChannelKind,
    /// The address, URL or topic.
    pub target: String,
}

impl NotificationChannel {
    /// Checks the channel; the message says what to fix.
    pub fn check(&self) -> Result<(), String> {
        let t = self.target.trim();
        if t.is_empty() || t.len() > 2048 || t.contains(char::is_control) {
            return Err("A notification channel needs an address.".into());
        }
        match self.kind {
            ChannelKind::Webhook => match url::Url::parse(t) {
                Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some() => Ok(()),
                _ => Err("A webhook needs an http:// or https:// address.".into()),
            },
            ChannelKind::Ntfy => {
                if t.starts_with("http://") || t.starts_with("https://") {
                    url::Url::parse(t)
                        .ok()
                        .filter(|u| u.host_str().is_some() && u.path().len() > 1)
                        .map(|_| ())
                        .ok_or_else(|| "An ntfy address looks like https://ntfy.sh/my-topic.".to_string())
                } else if t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
                    Ok(())
                } else {
                    Err("An ntfy topic uses letters, digits, - and _ only.".into())
                }
            }
            ChannelKind::Email => {
                if is_email_address(t) {
                    Ok(())
                } else {
                    Err("Enter an e-mail address like name@example.org.".into())
                }
            }
        }
    }
}

/// How the connection to the mail server is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SmtpSecurity {
    /// Starts in plain text and requires `STARTTLS` (usually port 587). A server that does not offer it is refused.
    #[default]
    StartTls,
    /// TLS from the first byte (usually port 465).
    Tls,
    /// No encryption: only for a relay on a trusted network, since the password and the mail cross it in plain text.
    None,
}

/// The mail server Spotter sends e-mail channels through. The password is never stored here; it lives in the
/// credential store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmtpSettings {
    /// The server's name or address.
    pub host: String,
    /// The port.
    pub port: u16,
    /// How the connection is secured.
    #[serde(default)]
    pub security: SmtpSecurity,
    /// The account to sign in with; empty or absent for a relay that needs none.
    #[serde(default)]
    pub username: Option<String>,
    /// The sender address (`spotter@example.org`).
    pub from: String,
}

/// Whether `text` is a plain e-mail address (`name@example.org`).
pub fn is_email_address(text: &str) -> bool {
    let t = text.trim();
    t.len() <= 254
        && !t.contains(char::is_control)
        && t.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !local.contains(['<', '>', ',', ';', '"'])
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !t.contains(' ')
                && t.matches('@').count() == 1
        })
}

impl SmtpSettings {
    /// Checks the settings; the message says what to fix.
    pub fn check(&self) -> Result<(), String> {
        let host = self.host.trim();
        if host.is_empty() || host.len() > 253 || host.contains(|c: char| c.is_whitespace() || c.is_control() || "/@:".contains(c)) {
            return Err("Enter the mail server's name, like smtp.example.org.".into());
        }
        if self.port == 0 {
            return Err("Enter the mail server's port (587 for STARTTLS, 465 for TLS).".into());
        }
        if let Some(u) = &self.username {
            if u.len() > 256 || u.contains(char::is_control) {
                return Err("The mail account name is not valid.".into());
            }
        }
        if !is_email_address(&self.from) {
            return Err("Enter the sender's e-mail address, like spotter@example.org.".into());
        }
        Ok(())
    }

    /// The account to sign in with, when there is one.
    pub fn account(&self) -> Option<&str> {
        self.username.as_deref().map(str::trim).filter(|u| !u.is_empty())
    }
}

/// Whether releases are announced one by one or collected into one notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DigestMode {
    /// Announce at once.
    #[default]
    Off,
    /// One notification a day.
    Daily,
    /// One notification a week (Mondays).
    Weekly,
}

/// The local calendar position the digest policy needs; the caller derives it from its clock and time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DigestClock {
    /// Local days since 1970-01-01.
    pub day: i64,
    /// Local weekday, 0 = Monday.
    pub weekday: u8,
    /// Local minutes after midnight.
    pub minutes: u16,
}

/// Local time when digests go out (09:00).
pub const DIGEST_MINUTES: u16 = 9 * 60;

/// Whether the digest should be sent now. `last_sent_day` is the local day of the previous one. A digest that was
/// missed (the app was closed) goes out at the next opportunity once its period has passed.
pub fn digest_due(mode: DigestMode, last_sent_day: Option<i64>, clock: DigestClock) -> bool {
    if clock.minutes < DIGEST_MINUTES {
        return false;
    }
    match (mode, last_sent_day) {
        (DigestMode::Off, _) => false,
        (_, None) => true,
        (DigestMode::Daily, Some(last)) => clock.day > last,
        (DigestMode::Weekly, Some(last)) => clock.day - last >= 7 || (clock.weekday == 0 && clock.day > last),
    }
}

/// Whether notifications wait: inside the global quiet hours or the watch's own (they add up).
pub fn in_quiet_hours(global: Option<QuietHours>, watch: Option<QuietHours>, local: crate::time::ClockTime) -> bool {
    global.is_some_and(|q| q.contains(local)) || watch.is_some_and(|q| q.contains(local))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::time::ClockTime;

    fn ch(kind: ChannelKind, target: &str) -> NotificationChannel {
        NotificationChannel {
            kind,
            target: target.into(),
        }
    }

    #[test]
    fn channels_are_checked() {
        for ok in [
            ch(ChannelKind::Webhook, "https://hooks.example.org/x"),
            ch(ChannelKind::Ntfy, "my-topic_1"),
            ch(ChannelKind::Ntfy, "https://ntfy.sh/my-topic"),
            ch(ChannelKind::Email, "me@example.org"),
        ] {
            assert!(ok.check().is_ok(), "{ok:?}");
        }
        for bad in [
            ch(ChannelKind::Webhook, "ftp://x/y"),
            ch(ChannelKind::Webhook, "not a url"),
            ch(ChannelKind::Webhook, ""),
            ch(ChannelKind::Webhook, &"h".repeat(3000)),
            ch(ChannelKind::Ntfy, "bad topic!"),
            ch(ChannelKind::Ntfy, "https://ntfy.sh/"),
            ch(ChannelKind::Email, "nobody"),
            ch(ChannelKind::Email, "a b@example.org"),
            ch(ChannelKind::Email, "a@.org"),
        ] {
            assert!(bad.check().is_err(), "{bad:?}");
        }
        let json = serde_json::to_string(&ch(ChannelKind::Ntfy, "t")).unwrap();
        assert_eq!(json, r#"{"kind":"ntfy","target":"t"}"#);
    }

    #[test]
    fn digests_go_out_once_per_period_at_nine() {
        let at = |day, weekday, minutes| DigestClock { day, weekday, minutes };
        assert!(!digest_due(DigestMode::Off, None, at(10, 0, 600)));
        assert!(!digest_due(DigestMode::Daily, None, at(10, 0, 8 * 60)), "before 09:00");
        assert!(digest_due(DigestMode::Daily, None, at(10, 0, 9 * 60)));
        assert!(!digest_due(DigestMode::Daily, Some(10), at(10, 0, 900)));
        assert!(digest_due(DigestMode::Daily, Some(10), at(11, 1, 900)));
        assert!(digest_due(DigestMode::Weekly, Some(3), at(10, 0, 900)), "Monday");
        assert!(!digest_due(DigestMode::Weekly, Some(10), at(10, 0, 900)));
        assert!(!digest_due(DigestMode::Weekly, Some(8), at(12, 2, 900)), "not Monday yet");
        assert!(digest_due(DigestMode::Weekly, Some(1), at(12, 2, 900)), "a missed week goes out");
    }

    #[test]
    fn quiet_hours_add_up() {
        let night = QuietHours {
            start: ClockTime::new(22, 0).unwrap(),
            end: ClockTime::new(7, 0).unwrap(),
        };
        let lunch = QuietHours {
            start: ClockTime::new(12, 0).unwrap(),
            end: ClockTime::new(13, 0).unwrap(),
        };
        let at = |h| ClockTime::new(h, 30).unwrap();
        assert!(in_quiet_hours(Some(night), None, at(23)));
        assert!(!in_quiet_hours(Some(night), None, at(12)));
        assert!(in_quiet_hours(Some(night), Some(lunch), at(12)));
        assert!(!in_quiet_hours(None, None, at(3)));
    }

    #[test]
    fn policies_round_trip_with_defaults() {
        let p: NotificationPolicy = toml::from_str("").unwrap();
        assert_eq!(p, NotificationPolicy::default());
        let q: NotificationPolicy =
            toml::from_str("mode = \"newer-than-installed\"\nnag = true\nquiet_hours = { start = \"22:00\", end = \"07:00\" }").unwrap();
        assert_eq!(q.mode, NotifyMode::NewerThanInstalled);
        assert!(q.nag && q.quiet_hours.is_some());
        assert_eq!(toml::from_str::<NotificationPolicy>(&toml::to_string(&q).unwrap()).unwrap(), q);
    }
}

#[cfg(test)]
mod smtp_tests {
    use super::*;

    fn ok() -> SmtpSettings {
        SmtpSettings {
            host: "smtp.example.org".into(),
            port: 587,
            security: SmtpSecurity::StartTls,
            username: Some("spotter".into()),
            from: "spotter@example.org".into(),
        }
    }

    #[test]
    fn addresses_are_plain_mailboxes() {
        for good in ["a@b.test", "first.last+tag@sub.example.org"] {
            assert!(is_email_address(good), "{good}");
        }
        for bad in [
            "",
            "a",
            "a@b",
            "@b.test",
            "a@.test",
            "a@b.",
            "a b@c.test",
            "a@b@c.test",
            "Name <a@b.test>",
            "a,b@c.test",
            "a\n@b.test",
            "x@y.test\r\nBcc: z@y.test",
        ] {
            assert!(!is_email_address(bad), "{bad:?}");
        }
        assert!(!is_email_address(&format!("{}@b.test", "a".repeat(260))));
    }

    #[test]
    fn smtp_settings_say_what_to_fix() {
        assert!(ok().check().is_ok());
        assert_eq!(ok().account(), Some("spotter"));
        let mut s = ok();
        s.username = Some("  ".into());
        assert_eq!(s.account(), None);
        s.username = None;
        assert_eq!(s.account(), None);
        assert!(s.check().is_ok(), "a relay may need no account");
        for (tweak, word) in [
            (SmtpSettings { host: " ".into(), ..ok() }, "server"),
            (
                SmtpSettings {
                    host: "smtp.example.org/x".into(),
                    ..ok()
                },
                "server",
            ),
            (
                SmtpSettings {
                    host: "u@smtp.example.org".into(),
                    ..ok()
                },
                "server",
            ),
            (
                SmtpSettings {
                    host: "a".repeat(300),
                    ..ok()
                },
                "server",
            ),
            (SmtpSettings { port: 0, ..ok() }, "port"),
            (
                SmtpSettings {
                    username: Some("a\nb".into()),
                    ..ok()
                },
                "account",
            ),
            (
                SmtpSettings {
                    username: Some("a".repeat(300)),
                    ..ok()
                },
                "account",
            ),
            (
                SmtpSettings {
                    from: "nobody".into(),
                    ..ok()
                },
                "sender",
            ),
        ] {
            let e = tweak.check().unwrap_err();
            assert!(e.contains(word), "{e}");
        }
    }

    #[test]
    fn smtp_settings_round_trip_and_default_to_starttls() {
        let json = serde_json::to_string(&ok()).unwrap();
        assert!(json.contains("\"start-tls\""), "{json}");
        let back: SmtpSettings = serde_json::from_str(r#"{"host":"h.test","port":25,"from":"a@b.test"}"#).unwrap();
        assert_eq!((back.security, back.username), (SmtpSecurity::StartTls, None));
        let none: SmtpSecurity = serde_json::from_str("\"none\"").unwrap();
        assert_eq!(none, SmtpSecurity::None);
    }
}
