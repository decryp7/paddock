//! The health gate of the tools that talk to the CI server: hold while Gitea or the shared cache (RustFS) is down.
//!
//! On 2026-11-20 a restart of the NAS took Gitea (HTTP 502) and RustFS down in the middle of a release: a push got a 401 that
//! erased a git login, and a cache trial failed with nothing to say why. A command that was about to dispatch a run or was
//! waiting for one now looks at both services first and **holds** (it does not fail) while either is down, up to the command's
//! own `--max`, and ends with one `::error::` that names the service and the next step.
//!
//! - **Gitea** answers `GET <server>/api/v1/version`.
//! - **RustFS** answers on its S3 endpoint: an unauthenticated GET of the bucket URL. The endpoint, bucket and TLS setting are the
//!   sccache settings (`PADDOCK_SCCACHE_S3_*`, [`crate::local_sccache::backend`]), never typed here. A command holds on a service
//!   only when it needs it: the dispatch, the wait on a run and the pushes read or write Gitea only, so none of them watches
//!   RustFS (a command that compiles through the cache would ask for it with [`services`]'s `rustfs` flag).
//!
//! **An address that can never work** (a malformed URL, a name that does not resolve: a wrong `--server`) is an error at the first
//! look, not a hold; it is held for only after the service has answered once.
//!
//! **Up** means any HTTP answer below 500 (a 403 from the bucket is a live server that wants a key); **down** is a connection
//! error, a timeout or a 5xx. A [`Held`] probe wraps the probe of a wait: while a service is down it does not poll the inner
//! probe at all (a 502 is not "the run list could not be read"), says so once per change of state, and answers `Pending` so the
//! caller's `--max` still bounds the hold. The hold takes no capacity (ER-43): it is a poll every interval, with no slot, lock or
//! queue entry.

use crate::wait::{Clock, Outcome, Poll, Probe, wait_for};
use std::collections::HashMap;
use std::io::Write;
use std::time::Duration;

/// What one look at a service found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// The server answered with this HTTP status.
    Answered(u16),
    /// No answer: a refused connection or a timeout, with the cause. A service that answered before is held for.
    Down(String),
    /// The address cannot be used at all: a malformed URL or a name that does not resolve (a wrong `--server`). Held for only after
    /// the service has answered once; at the first look it is an error at once, since no wait will ever change it.
    Invalid(String),
}

impl Reach {
    /// `None` when the service is up (any answer below 500), else why it is down.
    pub fn why_down(&self) -> Option<String> {
        match self {
            Reach::Answered(code) if *code < 500 => None,
            Reach::Answered(code) => Some(format!("HTTP {code}")),
            Reach::Down(why) | Reach::Invalid(why) => Some(why.clone()),
        }
    }
}

/// One look at a URL; the tests use a fake or a stub server.
pub trait Reacher {
    /// Asks `url` once and says how it answered. Never blocks for long.
    fn reach(&mut self, url: &str) -> Reach;
}

/// The real look: `curl`, no credentials, no redirects followed, a few seconds at most, standard input closed.
pub struct CurlReach;

/// The longest one look may take (connection and answer): a service that needs longer is not answering.
const LOOK_SECONDS: &str = "8";

impl Reacher for CurlReach {
    fn reach(&mut self, url: &str) -> Reach {
        let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let mut cmd = paddock_proc::command("curl");
        // `-q` first: no ~/.curlrc. No `--fail`: a 403 is an answer, and the status is what is asked for.
        cmd.args([
            "-q",
            "-sS",
            "-o",
            null,
            "-w",
            "%{http_code}",
            "--connect-timeout",
            "4",
            "--max-time",
            LOOK_SECONDS,
            "--url",
            url,
        ]);
        paddock_env::command::no_prompts(&mut cmd);
        let limits = paddock_proc::Limits::new(Duration::from_secs(15));
        match paddock_proc::run_bounded(cmd, limits) {
            Err(e) => Reach::Down(format!("cannot run curl: {e}")),
            Ok(out) => {
                let http = out.stdout_text().trim().parse::<u16>().unwrap_or(0);
                classify(out.success(), exit_code(&out), http, &out.stderr_text())
            }
        }
    }
}

fn exit_code(out: &paddock_proc::Output) -> Option<i32> {
    match out.outcome {
        paddock_proc::Outcome::Exited(s) => s.code(),
        _ => None,
    }
}

/// curl's own exit codes for an address that can never work: 1 unsupported protocol, 3 malformed URL, 5 and 6 a proxy or host that
/// does not resolve.
const INVALID_CURL_EXITS: [i32; 4] = [1, 3, 5, 6];

/// What a finished curl means: its HTTP status when it got one, else why there was none.
pub(crate) fn classify(success: bool, exit: Option<i32>, http: u16, stderr: &str) -> Reach {
    if success && http != 0 {
        return Reach::Answered(http);
    }
    let why = stderr
        .trim()
        .lines()
        .next()
        .filter(|l| !l.is_empty())
        .unwrap_or("no answer")
        .to_string();
    match exit {
        Some(c) if INVALID_CURL_EXITS.contains(&c) => Reach::Invalid(why),
        _ => Reach::Down(why),
    }
}

/// A service the tools depend on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// Its name in messages: `Gitea` or `RustFS`.
    pub name: &'static str,
    /// The URL that is asked.
    pub url: String,
    /// What to do when it stays down.
    pub next: &'static str,
}

/// The services a command needs: Gitea at `server`, and, only when `rustfs` (a command that compiles through the shared cache) and
/// the environment configures the S3 sccache backend (`PADDOCK_SCCACHE_BACKEND=s3`, [`crate::local_sccache::backend`]), RustFS. A
/// command that only reads Gitea (a dispatch, a wait on a run) never holds on RustFS: it does not need it. The second value is a
/// note when the S3 settings are wrong.
pub fn services(server: &str, env: &HashMap<String, String>, rustfs: bool) -> (Vec<Service>, Option<String>) {
    let mut list = vec![Service {
        name: "Gitea",
        url: format!("{}/api/v1/version", server.trim_end_matches('/')),
        next: "Gitea runs on the NAS: check that the NAS is up and that the Gitea container answers (`curl -sI <url>`)",
    }];
    let mut note = None;
    if rustfs && env.get("PADDOCK_SCCACHE_BACKEND").map(|v| v.trim()) == Some("s3") {
        match crate::local_sccache::backend(env) {
            Ok(crate::local_sccache::Backend::S3(s)) => list.push(Service {
                name: "RustFS",
                url: format!("{}://{}/{}", if s.ssl { "https" } else { "http" }, s.endpoint, s.bucket),
                next: "RustFS runs on the NAS: check that the NAS is up and that the RustFS container answers on its S3 port",
            }),
            Ok(_) => {}
            Err(e) => note = Some(format!("RustFS is not watched: {e}")),
        }
    }
    (list, note)
}

/// [`services`] for the real process: the environment as it is, a wrong S3 setting (asked for only with `rustfs`) printed as a warning
/// naming `who`.
pub fn real_services(server: &str, who: &str, rustfs: bool) -> Vec<Service> {
    let env: HashMap<String, String> = crate::process_env::vars().collect();
    let (list, note) = services(server, &env, rustfs);
    if let Some(note) = note {
        eprintln!("::warning::{who}: {note}");
    }
    list
}

/// The services and how to look at them, with the last state seen (for one line per change).
pub struct Health<'a> {
    services: Vec<Service>,
    reach: &'a mut dyn Reacher,
    /// Per service: why it was down at the last look (`None` up); `None` overall before the first look.
    last: Option<Vec<Option<String>>>,
    /// Per service: whether it has answered (any HTTP status) at least once.
    answered: Vec<bool>,
}

/// What one look at every service found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Look {
    /// The services that are down now (name, URL, cause).
    pub down: Vec<String>,
    /// One line per service that changed state since the last look.
    pub notes: Vec<String>,
    /// An address that can never work (a malformed URL, a name that does not resolve) at a service that never answered: no hold
    /// will help, so the wait ends at once with this text.
    pub fatal: Option<String>,
}

impl<'a> Health<'a> {
    /// Watches `services`, looking through `reach`.
    pub fn new(services: Vec<Service>, reach: &'a mut dyn Reacher) -> Health<'a> {
        let answered = vec![false; services.len()];
        Health {
            services,
            reach,
            last: None,
            answered,
        }
    }

    /// Looks at every service once.
    pub fn check(&mut self) -> Look {
        let seen: Vec<Reach> = self.services.iter().map(|s| self.reach.reach(&s.url)).collect();
        let mut look = Look::default();
        for (i, r) in seen.iter().enumerate() {
            if matches!(r, Reach::Answered(_)) {
                self.answered[i] = true;
            }
            if let (Reach::Invalid(why), false) = (r, self.answered[i]) {
                let s = &self.services[i];
                look.fatal = Some(format!(
                    "{} cannot be reached at {}: {why}. Check the address (a wrong --server, a typo in the host) or whether the DNS server (the NAS) is down; waiting will not help",
                    s.name, s.url
                ));
            }
        }
        let now: Vec<Option<String>> = seen.iter().map(Reach::why_down).collect();
        for (i, s) in self.services.iter().enumerate() {
            let before = self.last.as_ref().map(|l| l[i].is_some());
            match (&now[i], before) {
                (Some(why), None | Some(false)) => look
                    .notes
                    .push(format!("{} is down ({why}) at {}; holding until it answers", s.name, s.url)),
                (None, Some(true)) => look.notes.push(format!("{} answers again", s.name)),
                _ => {}
            }
        }
        look.down = self
            .services
            .iter()
            .zip(&now)
            .filter_map(|(s, w)| w.as_ref().map(|w| format!("{} ({}) is down: {w}", s.name, s.url)))
            .collect();
        self.last = Some(now);
        look
    }

    /// The names of the watched services: `Gitea`, or `Gitea or RustFS`.
    pub fn names(&self) -> String {
        self.services.iter().map(|s| s.name).collect::<Vec<_>>().join(" or ")
    }

    /// What is down as of the last look (empty when everything answered or nothing was looked at).
    pub fn down_now(&self) -> Vec<String> {
        let Some(last) = &self.last else { return Vec::new() };
        self.services
            .iter()
            .zip(last)
            .filter_map(|(s, w)| w.as_ref().map(|w| format!("{} ({}) is down: {w}", s.name, s.url)))
            .collect()
    }

    /// The next step for whatever is down as of the last look.
    pub fn next_step(&self) -> Option<String> {
        let last = self.last.as_ref()?;
        let steps: Vec<&str> = self
            .services
            .iter()
            .zip(last)
            .filter(|(_, w)| w.is_some())
            .map(|(s, _)| s.next)
            .collect();
        (!steps.is_empty()).then(|| format!("{}; then run the command again", steps.join("; and ")))
    }
}

/// A probe that holds while a service is down, then asks `inner`.
pub struct Held<'a, 'b> {
    inner: &'a mut dyn Probe,
    health: &'a mut Health<'b>,
    notes: Vec<String>,
}

impl<'a, 'b> Held<'a, 'b> {
    /// Wraps `inner`.
    pub fn new(inner: &'a mut dyn Probe, health: &'a mut Health<'b>) -> Held<'a, 'b> {
        Held {
            inner,
            health,
            notes: Vec::new(),
        }
    }
}

impl Probe for Held<'_, '_> {
    fn describe(&self) -> String {
        format!("{} (holding while {} is down)", self.inner.describe(), self.health.names())
    }
    fn poll(&mut self) -> Result<Poll, String> {
        let look = self.health.check();
        self.notes.extend(look.notes);
        if let Some(fatal) = look.fatal {
            return Ok(Poll::Failed(fatal));
        }
        if look.down.is_empty() {
            self.inner.poll()
        } else {
            Ok(Poll::Pending(format!("{}; holding", look.down.join("; "))))
        }
    }
    fn next_step(&self) -> String {
        self.health.next_step().unwrap_or_else(|| self.inner.next_step())
    }
    fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }
}

/// [`wait_for`] over `probe`, held while a service of `health` is down (no `health`: exactly [`wait_for`]).
pub fn wait_held(
    probe: &mut dyn Probe,
    health: Option<&mut Health<'_>>,
    clock: &mut dyn Clock,
    max: Duration,
    interval: Duration,
    log: &mut dyn Write,
) -> Outcome {
    match health {
        Some(h) => wait_for(&mut Held::new(probe, h), clock, max, interval, log),
        None => wait_for(probe, clock, max, interval, log),
    }
}

/// `scheme://host[:port]` of an http(s) repository URL, without any user information (a token in a URL is never kept); `None` for
/// a path or another scheme (a `file://` clone has no server to ask).
pub fn server_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    (!host.is_empty()).then(|| format!("{scheme}://{host}"))
}

/// How often [`hold_gitea`] looks while Gitea is down.
pub const PUSH_HOLD_INTERVAL: Duration = Duration::from_secs(10);

/// Holds until Gitea at `server` answers, at most `max` (the caller's own `--max`, never a bound of this module), before a push, so
/// the push is not sent into an outage and fails with a 502 nobody can read. `Ok(waited)` once it answers (zero when it did at
/// once); `Err` is the `::error::` text when it stayed down for `max` or its address can never work. One line per change of state
/// goes to `log`.
pub fn hold_gitea(
    server: &str,
    reach: &mut dyn Reacher,
    clock: &mut dyn Clock,
    max: Duration,
    log: &mut dyn Write,
) -> Result<Duration, String> {
    let (list, _) = services(server, &HashMap::new(), false);
    let mut health = Health::new(list, reach);
    match wait_held(
        &mut Ready("Gitea to answer"),
        Some(&mut health),
        clock,
        max,
        PUSH_HOLD_INTERVAL,
        log,
    ) {
        Outcome::Done { elapsed, .. } => Ok(elapsed),
        Outcome::Failed { detail, .. } => Err(format!("::error::push not sent: {detail}.")),
        _ => Err(format!(
            "::error::push held: {} for the whole bound ({}). Next: {}.",
            health.down_now().join("; "),
            crate::wait::fmt_duration(max),
            health.next_step().unwrap_or_else(|| "run the command again".into())
        )),
    }
}

/// A probe that is done at its first look: the wait is only the hold.
pub struct Ready(pub &'static str);

impl Probe for Ready {
    fn describe(&self) -> String {
        self.0.to_string()
    }
    fn poll(&mut self) -> Result<Poll, String> {
        Ok(Poll::Done("answering".into()))
    }
    fn next_step(&self) -> String {
        "run the command again".into()
    }
}

#[cfg(test)]
pub(crate) mod tests;
