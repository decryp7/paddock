//! The dispatcher: routes a message to the channels that want it, delivers with retries, contains failures.

use crate::config::{ChannelConfig, Settings, Transport};
use crate::error::SendError;
use crate::http::{HttpPoster, MAX_HEADER_VALUE, PostRequest};
use crate::mail::{Login, MailRequest, MailSender};
use crate::message::{Message, Severity, one_line};
use paddock_download::{Header, TransportErrorKind};
use paddock_jobs::CancelToken;
use paddock_jobs::time::{Backoff, BackoffPolicy, Clock, Retry};
use paddock_secrets::{Secret, Secrets};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Duration;

/// Default time limit of one delivery attempt.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
/// The `kind` of the message [`Dispatcher::test`] sends.
pub const TEST_KIND: &str = "test";
/// The most attempts a policy may ask for.
const MAX_ATTEMPTS: u32 = 5;

/// How hard a delivery tries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Attempts in all, the first included (1 to 5; a larger number is lowered).
    pub attempts: u32,
    /// The waits between attempts; its `give_up` is ignored (the attempts bound it). A server's `Retry-After` is honoured up
    /// to `backoff.max`.
    pub backoff: BackoffPolicy,
    /// The time limit of one attempt.
    pub timeout: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            attempts: 3,
            backoff: BackoffPolicy::retry_forever(
                Duration::from_secs(2),
                Duration::from_secs(30),
                Duration::from_secs(300),
            ),
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

/// What a delivery is doing now: attempt `attempt` of `of`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// The attempt under way or just made (1 to `of`).
    pub attempt: u32,
    /// The most attempts this delivery makes.
    pub of: u32,
    /// What it is doing.
    pub phase: Phase,
}

/// The phase of a [`Progress`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Sending the message.
    Sending,
    /// Waiting this long before the next attempt.
    Waiting(Duration),
}

fn delay_of(retry: Retry, max: Duration) -> Duration {
    if let Retry::After(d) = retry { d } else { max }
}

/// What happened to one channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    /// The channel's id.
    pub channel: String,
    /// Attempts made (at least one, unless the delivery was stopped first).
    pub attempts: u32,
    /// Delivered, or why not (the last attempt's error).
    pub result: Result<(), SendError>,
}

/// Sends messages to the configured channels. Cheap to clone; the settings are replaced with a new dispatcher.
#[derive(Clone)]
pub struct Dispatcher {
    settings: Settings,
    secrets: Arc<dyn Secrets>,
    poster: Arc<dyn HttpPoster>,
    mailer: Arc<dyn MailSender>,
    clock: Arc<dyn Clock>,
    policy: RetryPolicy,
}

impl Dispatcher {
    /// A dispatcher over the given ports and clock with the default [`RetryPolicy`].
    pub fn new(
        settings: Settings,
        secrets: Arc<dyn Secrets>,
        poster: Arc<dyn HttpPoster>,
        mailer: Arc<dyn MailSender>,
        clock: Arc<dyn Clock>,
    ) -> Dispatcher {
        Dispatcher {
            settings,
            secrets,
            poster,
            mailer,
            clock,
            policy: RetryPolicy::default(),
        }
    }

    /// Replaces the retry policy.
    pub fn with_policy(mut self, policy: RetryPolicy) -> Dispatcher {
        self.policy = policy;
        self
    }

    /// The settings in force.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Sends `msg` (bounded) to every enabled channel subscribed to its kind whose minimum severity it reaches, all channels
    /// at once, and returns one [`Delivery`] per attempted channel in the settings' order. Transient failures are retried per
    /// the policy, waiting on the injected clock; `cancel` stops the waits and the next attempts. Never panics. Blocks: call
    /// it from a background thread.
    pub fn dispatch(&self, msg: &Message, cancel: &CancelToken) -> Vec<Delivery> {
        let msg = msg.bounded();
        let wanted: Vec<&ChannelConfig> = self
            .settings
            .channels
            .iter()
            .filter(|c| c.wants(&msg))
            .collect();
        std::thread::scope(|scope| {
            let handles: Vec<_> = wanted
                .iter()
                .map(|cfg| {
                    scope.spawn(|| {
                        self.deliver(cfg, None, &msg, self.policy.attempts, cancel, &|_| {})
                    })
                })
                .collect();
            handles
                .into_iter()
                .zip(&wanted)
                .map(|(h, cfg)| {
                    h.join().unwrap_or_else(|_| Delivery {
                        channel: cfg.id.clone(),
                        attempts: 1,
                        result: Err(SendError::Panicked),
                    })
                })
                .collect()
        })
    }

    /// Sends a test message to the saved channel `id`, whether it is enabled or subscribed or not, with one attempt (the user
    /// is waiting for the answer). `Err` when there is no such channel.
    pub fn test(&self, id: &str, app: &str, cancel: &CancelToken) -> Result<Delivery, SendError> {
        let cfg = self.settings.channel(id).ok_or_else(|| {
            SendError::Invalid(format!("There is no notification channel '{id}'."))
        })?;
        Ok(self.test_with(cfg, None, app, cancel))
    }

    /// The test send for a channel as the user is editing it: `secret` is the token or password typed but not saved yet (when
    /// `None`, the stored one is used). Not limited to the saved settings, so the form can offer "Send test" before Save.
    pub fn test_with(
        &self,
        cfg: &ChannelConfig,
        secret: Option<&Secret>,
        app: &str,
        cancel: &CancelToken,
    ) -> Delivery {
        self.test_run(cfg, secret, app, cancel, 1, &|_| {})
    }

    /// Like [`test_with`](Self::test_with) but with the dispatcher's retries (a settings page that shows the attempts), reporting
    /// each attempt and each wait to `report` as it happens (attempt numbers only go up).
    pub fn test_with_progress(
        &self,
        cfg: &ChannelConfig,
        secret: Option<&Secret>,
        app: &str,
        cancel: &CancelToken,
        report: &dyn Fn(Progress),
    ) -> Delivery {
        self.test_run(cfg, secret, app, cancel, self.policy.attempts, report)
    }

    fn test_run(
        &self,
        cfg: &ChannelConfig,
        secret: Option<&Secret>,
        app: &str,
        cancel: &CancelToken,
        attempts: u32,
        report: &dyn Fn(Progress),
    ) -> Delivery {
        let msg = Message::new(
            TEST_KIND,
            Severity::Info,
            &format!("{app} test message"),
            &format!("This is a test message from {app}. If you can read it, notifications through {} work.", cfg.label()),
            app,
        )
        .bounded();
        if let Err(e) = (Settings {
            channels: vec![cfg.clone()],
        })
        .validate()
        {
            return Delivery {
                channel: cfg.id.clone(),
                attempts: 0,
                result: Err(SendError::Invalid(e.0)),
            };
        }
        self.deliver(cfg, secret, &msg, attempts, cancel, report)
    }

    fn deliver(
        &self,
        cfg: &ChannelConfig,
        secret: Option<&Secret>,
        msg: &Message,
        attempts: u32,
        cancel: &CancelToken,
        report: &dyn Fn(Progress),
    ) -> Delivery {
        let max = attempts.clamp(1, MAX_ATTEMPTS);
        let mut backoff = Backoff::new(BackoffPolicy {
            give_up: None,
            ..self.policy.backoff
        });
        let mut made = 0;
        loop {
            if cancel.is_cancelled() {
                return Delivery {
                    channel: cfg.id.clone(),
                    attempts: made,
                    result: Err(SendError::Cancelled),
                };
            }
            made += 1;
            report(Progress {
                attempt: made,
                of: max,
                phase: Phase::Sending,
            });
            let result = catch_unwind(AssertUnwindSafe(|| self.attempt(cfg, secret, msg)))
                .unwrap_or(Err(SendError::Panicked));
            let Err(err) = result else {
                return Delivery {
                    channel: cfg.id.clone(),
                    attempts: made,
                    result: Ok(()),
                };
            };
            if made >= max || !err.retryable() {
                return Delivery {
                    channel: cfg.id.clone(),
                    attempts: made,
                    result: Err(err),
                };
            }
            let delay = delay_of(
                backoff.failed(self.clock.monotonic()),
                self.policy.backoff.max,
            );
            let delay = err
                .retry_after()
                .map_or(delay, |asked| asked.max(delay))
                .min(self.policy.backoff.max);
            report(Progress {
                attempt: made,
                of: max,
                phase: Phase::Waiting(delay),
            });
            if self.clock.wait(delay, cancel).is_err() {
                return Delivery {
                    channel: cfg.id.clone(),
                    attempts: made,
                    result: Err(SendError::Cancelled),
                };
            }
        }
    }

    /// One attempt: the secret, the request, the answer.
    fn attempt(
        &self,
        cfg: &ChannelConfig,
        secret: Option<&Secret>,
        msg: &Message,
    ) -> Result<(), SendError> {
        let stored;
        let secret = match (cfg.secret_name(), secret) {
            (None, _) => None,
            (Some(_), Some(s)) => Some(s),
            (Some(name), None) => {
                stored = self
                    .secrets
                    .get(&name)
                    .map_err(|e| SendError::SecretStore(e.to_string()))?;
                Some(stored.as_ref().ok_or_else(|| SendError::MissingSecret {
                    channel: cfg.label().to_string(),
                })?)
            }
        };
        match &cfg.transport {
            Transport::Webhook { url, .. } => {
                let body = serde_json::to_vec(msg).map_err(|e| SendError::Failed(e.to_string()))?;
                let mut headers = vec![header("Content-Type", "application/json")?];
                headers.extend(bearer(secret)?);
                self.post(url, headers, body)
            }
            Transport::Ntfy { server, topic, .. } => {
                let url = format!("{}/{}", server.trim_end_matches('/'), topic);
                let mut headers = vec![
                    header("Title", &one_line(&msg.title, crate::MAX_TITLE_CHARS))?,
                    header("Priority", priority(msg.severity))?,
                    header("Tags", tags(msg.severity))?,
                ];
                headers.extend(bearer(secret)?);
                self.post(&url, headers, msg.body.clone().into_bytes())
            }
            Transport::Email {
                host,
                port,
                security,
                username,
                from,
                to,
            } => {
                let login = match (username, secret) {
                    (Some(username), Some(password)) => Some(Login {
                        username: username.clone(),
                        password: password.clone(),
                    }),
                    _ => None,
                };
                self.mailer.send(&MailRequest {
                    host: host.trim().to_string(),
                    port: *port,
                    security: *security,
                    login,
                    from: from.trim().to_string(),
                    to: to.iter().map(|a| a.trim().to_string()).collect(),
                    subject: msg.title.clone(),
                    body: msg.body.clone(),
                    step_timeout: self.policy.timeout,
                })
            }
        }
    }

    fn post(&self, url: &str, headers: Vec<Header>, body: Vec<u8>) -> Result<(), SendError> {
        let host = host_label(url);
        let request = PostRequest {
            url: url.to_string(),
            headers,
            body,
            timeout: self.policy.timeout,
        };
        let response = self.poster.post(&request).map_err(|e| match e.kind {
            TransportErrorKind::Connect => SendError::Connect(host.clone()),
            TransportErrorKind::Timeout => SendError::Timeout(host.clone()),
            TransportErrorKind::Tls => SendError::Tls(host.clone()),
            TransportErrorKind::Proxy => SendError::Proxy(e.message.clone()),
            TransportErrorKind::Invalid => {
                SendError::Invalid(format!("The address {host} cannot be used."))
            }
        })?;
        if (200..300).contains(&response.status) {
            Ok(())
        } else {
            Err(SendError::Rejected {
                status: response.status,
                retry_after: response.retry_after,
            })
        }
    }
}

fn header(name: &str, value: &str) -> Result<Header, SendError> {
    if value.len() > MAX_HEADER_VALUE {
        return Err(SendError::Invalid(format!(
            "The {name} header is too long."
        )));
    }
    Header::new(name, value).map_err(|e| SendError::Invalid(e.to_string()))
}

fn bearer(secret: Option<&Secret>) -> Result<Option<Header>, SendError> {
    secret
        .map(|s| header("Authorization", &format!("Bearer {}", s.expose())))
        .transpose()
        // A refused header must not echo the token.
        .map_err(|_| {
            SendError::Invalid("The token cannot be sent as a header; enter it again.".into())
        })
}

fn priority(s: Severity) -> &'static str {
    match s {
        Severity::Info => "3",
        Severity::Warning => "4",
        Severity::Error => "5",
    }
}

fn tags(s: Severity) -> &'static str {
    match s {
        Severity::Info => "information_source",
        Severity::Warning => "warning",
        Severity::Error => "rotating_light",
    }
}

/// `host[:port]` of a URL for messages: no scheme, login, path or query (those can hold secrets).
fn host_label(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    authority.rsplit('@').next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::host_label;

    #[test]
    fn labels_hide_login_path_and_query() {
        assert_eq!(
            host_label("https://u:p@hooks.example.org:8443/a/b?token=x"),
            "hooks.example.org:8443"
        );
        assert_eq!(host_label("ntfy.sh/topic"), "ntfy.sh");
        assert_eq!(host_label(""), "");
    }
}
