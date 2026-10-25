//! What the engine tells the outside world: notifications (the shell shows them through the OS; webhook and ntfy
//! channels are delivered by the engine itself) and actions on new releases (the shell runs them).
use crate::fetch::PoliteFetcher;
use crate::mail::{self, MailAccount, MailMessage};
use spotter_core::action::Action;
use spotter_core::notify::{ChannelKind, NotificationChannel};
use spotter_core::release::Release;
use spotter_core::source::SourceError;
use std::sync::Mutex;
use std::time::Duration;
use url::Url;

/// What a notification is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    /// A new release.
    NewRelease,
    /// Several releases collected into one.
    Digest,
    /// A page needs the user (login or captcha).
    NeedsYou,
}

/// A notification to deliver.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    /// What it is about.
    pub kind: NotificationKind,
    /// The watch, for a single-watch notification.
    pub watch_id: Option<String>,
    /// The headline.
    pub title: String,
    /// The text.
    pub body: String,
    /// The release, for a new-release notification.
    pub release: Option<Release>,
    /// Extra channels. The engine delivers webhooks and ntfy topics itself; e-mail is left to the shell.
    pub channels: Vec<NotificationChannel>,
}

/// An action to run on a new release.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionRequest {
    /// The watch.
    pub watch_id: String,
    /// Its name.
    pub watch_name: String,
    /// The release.
    pub release: Release,
    /// The watch's actions.
    pub actions: Vec<Action>,
}

/// The shell's side of the engine's effects.
///
/// Contract: calls return quickly (hand the work to another task); never panic.
pub trait Notifier: Send + Sync {
    /// Shows a notification through the OS.
    fn notify(&self, notification: &Notification);
    /// Runs a watch's actions for a new release.
    fn run_actions(&self, _request: &ActionRequest) {}
}

/// Does nothing.
#[derive(Debug, Default)]
pub struct NullNotifier;

impl Notifier for NullNotifier {
    fn notify(&self, _: &Notification) {}
}

/// Remembers what it was told, for tests.
#[derive(Debug, Default)]
pub struct RecordingNotifier {
    /// Notifications received.
    pub notifications: Mutex<Vec<Notification>>,
    /// Action requests received.
    pub actions: Mutex<Vec<ActionRequest>>,
}

impl RecordingNotifier {
    /// The notifications so far.
    pub fn notifications(&self) -> Vec<Notification> {
        self.notifications.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// The action requests so far.
    pub fn actions(&self) -> Vec<ActionRequest> {
        self.actions.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

impl Notifier for RecordingNotifier {
    fn notify(&self, n: &Notification) {
        self.notifications
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(n.clone());
    }

    fn run_actions(&self, r: &ActionRequest) {
        self.actions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(r.clone());
    }
}

/// How long a channel delivery may take.
pub const CHANNEL_TIMEOUT: Duration = Duration::from_secs(10);

/// Delivers a notification to its webhook, ntfy and e-mail channels; returns one message per failure. `mail` is the
/// configured mail server, if any.
pub async fn deliver(fetcher: &PoliteFetcher, mail: Option<&MailAccount>, n: &Notification, allow_local: bool) -> Vec<String> {
    let mut failures = Vec::new();
    for ch in &n.channels {
        let result = match ch.kind {
            ChannelKind::Webhook => post_webhook(fetcher, ch.target.trim(), n, allow_local)
                .await
                .map_err(|e| e.message().to_string()),
            ChannelKind::Ntfy => post_ntfy(fetcher, ch.target.trim(), n, allow_local)
                .await
                .map_err(|e| e.message().to_string()),
            ChannelKind::Email => send_email(mail, ch.target.trim(), n).await,
        };
        if let Err(message) = result {
            failures.push(format!("{} channel: {message}", kind_name(ch.kind)));
        }
    }
    failures
}

/// The text of an e-mail: the notification's body and, for a release, where to get it.
fn email_body(n: &Notification) -> String {
    let mut body = n.body.clone();
    if let Some(r) = &n.release {
        body.push_str(&format!("\n\n{} {}\n{}", r.name, r.version.text, r.location));
    }
    body.push_str("\n\n-- \nSent by Spotter");
    body
}

async fn send_email(mail: Option<&MailAccount>, to: &str, n: &Notification) -> Result<(), String> {
    let Some(account) = mail else {
        return Err("Set up the mail server in Settings to send e-mail.".into());
    };
    let message = MailMessage {
        to: to.to_string(),
        subject: n.title.clone(),
        body: email_body(n),
    };
    mail::send(account, &message).await.map_err(|e| e.message())
}

fn kind_name(k: ChannelKind) -> &'static str {
    match k {
        ChannelKind::Webhook => "Webhook",
        ChannelKind::Ntfy => "ntfy",
        ChannelKind::Email => "E-mail",
    }
}

async fn post(fetcher: &PoliteFetcher, request: reqwest::RequestBuilder, shown: &Url) -> Result<(), SourceError> {
    let response = request
        .timeout(CHANNEL_TIMEOUT)
        .send()
        .await
        .map_err(|e| spotter_sources::http::transport(e, &format!("{}://{}", shown.scheme(), shown.host_str().unwrap_or(""))))?;
    let _ = fetcher;
    match spotter_core::source::http::status_error(response.status().as_u16(), shown.host_str().unwrap_or("the channel")) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

async fn post_webhook(fetcher: &PoliteFetcher, target: &str, n: &Notification, allow_local: bool) -> Result<(), SourceError> {
    let url = Url::parse(target).map_err(|_| SourceError::InvalidConfig("The webhook address is not valid.".into()))?;
    fetcher.check_url(&url, allow_local).await?;
    let body = serde_json::json!({
        "title": n.title,
        "body": n.body,
        "watch": n.watch_id,
        "version": n.release.as_ref().map(|r| r.version.text.clone()),
        "location": n.release.as_ref().map(|r| r.location.clone()),
    });
    post(
        fetcher,
        fetcher
            .client_for(allow_local)
            .post(url.clone())
            .header("Content-Type", "application/json")
            .body(body.to_string()),
        &url,
    )
    .await
}

async fn post_ntfy(fetcher: &PoliteFetcher, target: &str, n: &Notification, allow_local: bool) -> Result<(), SourceError> {
    let address = if target.starts_with("http://") || target.starts_with("https://") {
        target.to_string()
    } else {
        format!("https://ntfy.sh/{target}")
    };
    let url = Url::parse(&address).map_err(|_| SourceError::InvalidConfig("The ntfy address is not valid.".into()))?;
    fetcher.check_url(&url, allow_local).await?;
    let mut req = fetcher.client_for(allow_local).post(url.clone()).body(n.body.clone());
    // Header values must be visible ASCII; the title is sanitised rather than refused.
    let title: String = n.title.chars().filter(|c| c.is_ascii() && !c.is_control()).collect();
    if !title.is_empty() {
        req = req.header("Title", title);
    }
    if let Some(r) = &n.release {
        if r.location.starts_with("http://") || r.location.starts_with("https://") {
            if let Ok(v) = reqwest::header::HeaderValue::from_str(&r.location) {
                req = req.header("Click", v);
            }
        }
    }
    post(fetcher, req, &url).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use spotter_core::notify::NotificationChannel;

    struct OnlyNotify;
    impl Notifier for OnlyNotify {
        fn notify(&self, _: &Notification) {}
    }

    fn note(location: &str, channel: NotificationChannel) -> Notification {
        Notification {
            kind: NotificationKind::NewRelease,
            watch_id: None,
            title: "Title".into(),
            body: "Body".into(),
            release: Some(spotter_core::release::Release {
                version: spotter_core::version::VersionScheme::Semver.extract("1.0.0").unwrap(),
                name: "app".into(),
                location: location.into(),
                published_at: None,
                size: None,
                sha256: None,
                detail: Default::default(),
            }),
            channels: vec![channel],
        }
    }

    #[test]
    fn the_default_and_null_notifiers_do_nothing_and_do_not_panic() {
        let n = note(
            "https://x/1",
            NotificationChannel {
                kind: ChannelKind::Email,
                target: String::new(),
            },
        );
        OnlyNotify.notify(&n);
        NullNotifier.notify(&n);
        OnlyNotify.run_actions(&ActionRequest {
            watch_id: "a".into(),
            watch_name: "A".into(),
            release: n.release.clone().unwrap(),
            actions: Vec::new(),
        });
        assert_eq!(kind_name(ChannelKind::Webhook), "Webhook");
        assert_eq!(kind_name(ChannelKind::Ntfy), "ntfy");
        assert_eq!(kind_name(ChannelKind::Email), "E-mail");
    }

    #[tokio::test]
    async fn failures_name_the_channel_and_a_release_without_a_web_address_has_no_click_target() {
        let fetcher = PoliteFetcher::new(crate::fetch::FetchConfig::new("test")).unwrap();
        // Nothing listens on port 1: each delivery fails and says which channel.
        for (kind, target) in [
            (ChannelKind::Ntfy, "http://127.0.0.1:1/topic"),
            (ChannelKind::Webhook, "http://127.0.0.1:1/hook"),
        ] {
            let n = note(
                "C:/not/a/web/address",
                NotificationChannel {
                    kind,
                    target: target.into(),
                },
            );
            let failures = deliver(&fetcher, None, &n, true).await;
            assert_eq!(failures.len(), 1, "{failures:?}");
            assert!(failures[0].starts_with(&format!("{} channel:", kind_name(kind))), "{failures:?}");
        }
        let email = note(
            "https://x",
            NotificationChannel {
                kind: ChannelKind::Email,
                target: "a@b.test".into(),
            },
        );
        let failures = deliver(&fetcher, None, &email, true).await;
        assert_eq!(
            failures,
            vec!["E-mail channel: Set up the mail server in Settings to send e-mail.".to_string()]
        );
    }
    #[tokio::test]
    async fn a_notification_without_a_release_still_goes_to_ntfy_and_the_mail_text_says_where_it_is_from() {
        let fetcher = PoliteFetcher::new(crate::fetch::FetchConfig::new("test")).unwrap();
        let mut digest = note(
            "https://x",
            NotificationChannel {
                kind: ChannelKind::Ntfy,
                target: "http://127.0.0.1:1/topic".into(),
            },
        );
        digest.release = None;
        let failures = deliver(&fetcher, None, &digest, true).await;
        assert!(failures[0].starts_with("ntfy channel:"), "{failures:?}");
        assert_eq!(email_body(&digest), "Body\n\n-- \nSent by Spotter");
        let with_release = note(
            "https://x/app",
            NotificationChannel {
                kind: ChannelKind::Email,
                target: "a@b.test".into(),
            },
        );
        let text = email_body(&with_release);
        assert!(text.starts_with("Body\n\napp 1.0.0\nhttps://x/app\n"), "{text}");
    }

    #[tokio::test]
    async fn a_location_that_is_not_a_valid_header_value_is_left_out_of_the_click_target() {
        let fetcher = PoliteFetcher::new(crate::fetch::FetchConfig::new("test")).unwrap();
        let n = note(
            "https://x/line\nbreak",
            NotificationChannel {
                kind: ChannelKind::Ntfy,
                target: "http://127.0.0.1:1/topic".into(),
            },
        );
        let failures = deliver(&fetcher, None, &n, true).await;
        assert_eq!(
            failures.len(),
            1,
            "the delivery still fails only because nothing listens: {failures:?}"
        );
        assert!(failures[0].starts_with("ntfy channel:"), "{failures:?}");
        let mut plain = n.clone();
        plain.release = None;
        assert_eq!(
            deliver(&fetcher, None, &plain, true).await.len(),
            1,
            "a notification without a release has no click target either"
        );
    }
}
