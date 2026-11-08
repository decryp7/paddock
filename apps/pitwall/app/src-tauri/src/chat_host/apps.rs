//! Settings > Connected apps (ADR 0023 Decision 7): the family apps whose MCP tools the chat may use, the key the user pasted
//! for each (kept by `paddock-secrets`, never shown again and never logged), the connection over the app's loopback listener
//! (`paddock_mcp::client::connect_loopback`, the family's one handshake), and the assistant's tool set built from what is
//! connected. Listing is on connect and on Refresh, never per turn (Dyno sends no `list_changed`).

use super::policy::PitwallPolicy;
use super::tool::{tool_set, tools_of, ToolNames};
use paddock_assistant::{Assistant, AssistantTool};
use paddock_jobs::time::{Backoff, BackoffPolicy, Retry};
use paddock_mcp::client::{connect_loopback, Connection, ConnectionConfig};
use paddock_secrets::{Name, Secret, Secrets};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// The family apps the chat can connect to: id (its `paddock-env` folder name) and title. Recce joins when it serves MCP.
pub const KNOWN: &[(&str, &str)] = &[("dyno", "Dyno")];
/// Longest key accepted.
pub const MAX_KEY_LEN: usize = 256;

/// One tool of a connected app, as Settings lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTool {
    /// The tool's name on that app.
    pub name: String,
    /// Its name for people.
    pub title: String,
    /// It only looks (asked once, then remembered when allowed); otherwise an action, asked every call.
    pub read_only: bool,
    /// The user said Always allow for it (Settings can ask again).
    pub remembered: bool,
}

/// A family app as Settings > Connected apps shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedApp {
    /// `dyno`.
    pub id: String,
    /// `Dyno`.
    pub title: String,
    /// A key is saved for it.
    pub key_saved: bool,
    /// Connected now.
    pub connected: bool,
    /// Why the last connect failed, in the app's words with the next step; `None` when it did not.
    pub problem: Option<String>,
    /// The app refused the saved key: Refresh cannot help, only a new key.
    pub key_refused: bool,
    /// The connection was lost and Pitwall is trying again on its back-off (the chat answers without its tools meanwhile).
    pub reconnecting: bool,
    /// What only the user can do before the app connects (install it, update it); `None` otherwise.
    pub needs: Option<AppNeed>,
    /// Its tools, when connected.
    pub tools: Vec<AppTool>,
    /// How many of its tools are always allowed.
    pub remembered: usize,
}

/// What only the user can do before an app connects; Pitwall does not retry meanwhile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AppNeed {
    /// It is not installed on this computer.
    Install,
    /// It is too old for Pitwall.
    Update,
}

/// Why a command about a connected app was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppsError {
    /// Not one of [`KNOWN`].
    Unknown(String),
    /// The pasted text is not a key.
    BadKey,
    /// The key could not be saved or read; the text says why.
    Store(String),
}

impl std::fmt::Display for AppsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppsError::Unknown(id) => write!(f, "Pitwall does not know an app called {id}."),
            AppsError::BadKey => {
                f.write_str("That is not an AI access key. Copy the key Dyno shows in Settings > AI access (it starts with pmk_).")
            }
            AppsError::Store(why) => write!(f, "The key could not be kept: {why}."),
        }
    }
}

impl std::error::Error for AppsError {}

/// The key as typed: trimmed, `pmk_`, no spaces, at most [`MAX_KEY_LEN`].
pub fn parse_key(text: &str) -> Result<String, AppsError> {
    let key = text.trim();
    if key.len() > 4 && key.len() <= MAX_KEY_LEN && key.starts_with("pmk_") && !key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        Ok(key.to_string())
    } else {
        Err(AppsError::BadKey)
    }
}

/// Where an app's listener file is, by app id.
type Discovery = Box<dyn Fn(&str) -> Option<PathBuf> + Send + Sync>;

struct Live {
    conn: Arc<Connection>,
    tools: Vec<Arc<dyn AssistantTool>>,
}

/// The connected apps of this app process.
pub struct ConnectedApps {
    secrets: Arc<dyn Secrets>,
    discovery: Discovery,
    assistant: Arc<Assistant>,
    policy: Arc<PitwallPolicy>,
    names: ToolNames,
    live: Mutex<BTreeMap<String, Live>>,
    problems: Mutex<BTreeMap<String, Problem>>,
    changed: Mutex<Option<OnChange>>,
    retry: Mutex<BTreeMap<String, Retrying>>,
    opener: Mutex<Option<Opener>>,
}

/// Why an app is not connected, in words, and whether Pitwall can help by trying again.
#[derive(Debug, Clone)]
struct Problem {
    words: String,
    stuck: Stuck,
}

/// What a failed connect leaves to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stuck {
    /// Nothing for the user: Pitwall tries again on its back-off (the app is closed or busy).
    No,
    /// The app refused the saved key: only a new key helps.
    KeyRefused,
    /// Only the user can help: install or update the app. Not retried.
    Needs(AppNeed),
}

impl Problem {
    fn retry(words: String) -> Problem {
        Problem { words, stuck: Stuck::No }
    }
}

/// An app whose connection was lost: when to try again, on the family's back-off (1 s doubling to 60 s, never giving up:
/// the app may come back at any time).
struct Retrying {
    backoff: Backoff,
    next: Duration,
}

fn reconnect_policy() -> BackoffPolicy {
    BackoffPolicy::retry_forever(Duration::from_secs(1), Duration::from_secs(60), Duration::from_secs(300))
}

/// What opening an app came to. On the wire a need is a problem like any other (`{kind: "problem", message}`); the row's
/// `needs` says which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    /// It is starting (or was running and came to the front).
    Started,
    /// It could not be opened; the words say what to do.
    Problem(String),
    /// It could not be opened until the user installs or updates it; the words say so.
    Needs(AppNeed, String),
}

impl Serialize for Opened {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut out = s.serialize_struct("Opened", 2)?;
        match self {
            Opened::Started => out.serialize_field("kind", "started")?,
            Opened::Problem(words) | Opened::Needs(_, words) => {
                out.serialize_field("kind", "problem")?;
                out.serialize_field("message", words)?;
            }
        }
        out.end()
    }
}

/// What the family's hand-off answered, in Pitwall's words for Connected apps (paddock-siblings speaks of files; here the
/// user wants the app open).
pub fn open_words(outcome: Result<paddock_siblings::Outcome, paddock_siblings::SiblingError>, title: &str) -> Opened {
    match outcome {
        Ok(paddock_siblings::Outcome::Opened { .. }) => Opened::Started,
        Ok(paddock_siblings::Outcome::NotInstalled { .. }) => Opened::Needs(
            AppNeed::Install,
            format!("{title} isn't installed on this computer. Install {title}, then choose Open {title}."),
        ),
        // A program found under the app's name before the family's rename: an old copy.
        Ok(paddock_siblings::Outcome::NeedsUpdate { .. }) => Opened::Needs(
            AppNeed::Update,
            format!("This copy of {title} is too old to open from Pitwall. Update {title}, then choose Open {title}."),
        ),
        Err(e) => Opened::Problem(format!("{title} could not be opened: {e}.")),
    }
}

/// Why `initialize` failed, in Pitwall's words where they say more. A Dyno too old for Pitwall's AI access shows up here and
/// only here: a Dyno without the listener at all (before 1.1.0) writes no discovery file, which reads as "isn't open, or AI
/// access is off", and the handshake line carries no version; the MCP protocol version `initialize` negotiates is the one
/// signal of an app too old (or too new) for this Pitwall.
pub fn init_problem(e: &paddock_mcp::client::ClientError, title: &str) -> String {
    match e {
        paddock_mcp::client::ClientError::UnsupportedVersion { .. } => {
            format!("{title} is too old for AI access from Pitwall. Update {title}, then choose Refresh tools.")
        }
        other => other.to_string(),
    }
}

/// Starts a family app by id (`paddock-siblings` in the app; a fake in tests).
type Opener = Box<dyn Fn(&str) -> Opened + Send + Sync>;

/// Told every apps' views after each change.
type OnChange = Box<dyn Fn(&[ConnectedApp]) + Send + Sync>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn title_of(id: &str) -> Result<&'static str, AppsError> {
    KNOWN
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, t)| *t)
        .ok_or_else(|| AppsError::Unknown(id.into()))
}

fn key_name(id: &str) -> Result<Name, AppsError> {
    Name::new("mcp", id).map_err(|e| AppsError::Store(e.to_string()))
}

impl ConnectedApps {
    /// The apps over `secrets`; `discovery` names an app's listener file (`paddock_env` folders in the app, a temp folder
    /// in tests).
    pub fn new(
        secrets: Arc<dyn Secrets>,
        discovery: impl Fn(&str) -> Option<PathBuf> + Send + Sync + 'static,
        assistant: Arc<Assistant>,
        policy: Arc<PitwallPolicy>,
        names: ToolNames,
    ) -> ConnectedApps {
        ConnectedApps {
            secrets,
            discovery: Box::new(discovery),
            assistant,
            policy,
            names,
            live: Mutex::default(),
            problems: Mutex::default(),
            changed: Mutex::default(),
            retry: Mutex::default(),
            opener: Mutex::default(),
        }
    }

    /// How [`ConnectedApps::open`] starts an app.
    pub fn set_opener(&self, f: impl Fn(&str) -> Opened + Send + Sync + 'static) {
        *lock(&self.opener) = Some(Box::new(f));
    }

    /// Opens the app ("Open Dyno"), then connects at the next tick once it listens (on the back-off after that).
    pub fn open(&self, id: &str) -> Result<Opened, AppsError> {
        title_of(id)?;
        let outcome = lock(&self.opener)
            .as_ref()
            .map_or_else(|| Opened::Problem("Pitwall can't open other apps in this build.".into()), |f| f(id));
        log::info!(target: "pitwall_chat", "connected apps: open {id}: {outcome:?}");
        match &outcome {
            Opened::Started => {
                let mut retry = lock(&self.retry);
                retry.insert(
                    id.to_string(),
                    Retrying {
                        backoff: Backoff::new(reconnect_policy()),
                        next: Duration::ZERO,
                    },
                );
            }
            // Its words replace the row's problem (not stacked under the reconnect line), and nothing is retried until the
            // user acts.
            Opened::Problem(words) | Opened::Needs(_, words) => {
                let stuck = match &outcome {
                    Opened::Needs(need, _) => Stuck::Needs(*need),
                    _ => Stuck::No,
                };
                lock(&self.retry).remove(id);
                lock(&self.problems).insert(
                    id.to_string(),
                    Problem {
                        words: words.clone(),
                        stuck,
                    },
                );
                self.publish();
            }
        }
        Ok(outcome)
    }

    /// The app's timer (every second or so, `now` from the monotonic clock): a connection that ended is dropped and retried
    /// on the back-off; an app with a saved key that is not connected is tried when its delay is over. A refused key is never
    /// retried (only a new key helps); a connect the user starts resets the back-off.
    pub fn tick(&self, now: Duration) {
        let ended: Vec<(String, String)> = lock(&self.live)
            .iter()
            .filter_map(|(id, l)| l.conn.closed().map(|why| (id.clone(), why)))
            .collect();
        for (id, why) in &ended {
            lock(&self.live).remove(id);
            let title = title_of(id).unwrap_or("The app");
            log::warn!(target: "pitwall_chat", "connected apps: {id} connection ended ({why}); retrying with back-off");
            lock(&self.problems).insert(
                id.clone(),
                Problem::retry(format!("{title} closed the connection ({why}). Trying again when {title} is back.")),
            );
            self.schedule(id, now);
        }
        if !ended.is_empty() {
            self.publish();
        }
        let due: Vec<String> = lock(&self.retry)
            .iter()
            .filter(|(_, r)| r.next <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            let v = self.attempt(&id);
            let stuck = lock(&self.problems).get(&id).map_or(Stuck::No, |p| p.stuck);
            // Connected, or only the user can help now (a refused or removed key, an app too old): stop retrying.
            if v.connected || stuck != Stuck::No || !v.key_saved {
                lock(&self.retry).remove(&id);
                if !v.connected {
                    log::info!(target: "pitwall_chat", "connected apps: {id} not retried until the user acts ({stuck:?})");
                    self.publish();
                }
            } else {
                self.schedule(&id, now);
                // Keep the words that say it is retrying.
                if let Some(p) = lock(&self.problems).get_mut(&id) {
                    if !p.words.contains("Trying again") {
                        p.words.push_str(" Trying again.");
                    }
                }
            }
        }
    }

    fn schedule(&self, id: &str, now: Duration) {
        let mut retry = lock(&self.retry);
        let r = retry.entry(id.to_string()).or_insert_with(|| Retrying {
            backoff: Backoff::new(reconnect_policy()),
            next: now,
        });
        r.next = match r.backoff.failed(now) {
            Retry::After(d) => now + d,
            // A policy that never gives up; kept safe anyway.
            Retry::GiveUp => now + Duration::from_secs(60),
        };
    }

    /// Calls `f` with every app's view after each change (connect, a new key, forget): the app emits it to every window.
    pub fn on_change(&self, f: impl Fn(&[ConnectedApp]) + Send + Sync + 'static) {
        *lock(&self.changed) = Some(Box::new(f));
    }

    /// Every known app.
    pub fn views(&self) -> Vec<ConnectedApp> {
        KNOWN.iter().map(|(id, _)| self.view(id)).collect()
    }

    fn view(&self, id: &str) -> ConnectedApp {
        let live = lock(&self.live);
        let allowed: Vec<String> = self
            .policy
            .remembered()
            .into_iter()
            .filter(|r| r.server == id)
            .map(|r| r.tool)
            .collect();
        let tools = live
            .get(id)
            .map(|l| {
                l.tools
                    .iter()
                    .filter_map(|t| {
                        self.names.of(&t.spec().name).map(|(_, name)| AppTool {
                            remembered: allowed.contains(&name),
                            name,
                            title: t.title().unwrap_or_default(),
                            read_only: !t.access().writes(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let problem = lock(&self.problems).get(id).cloned();
        ConnectedApp {
            id: id.into(),
            title: title_of(id).unwrap_or(id).into(),
            key_saved: key_name(id).ok().and_then(|n| self.secrets.get(&n).ok().flatten()).is_some(),
            connected: live.contains_key(id),
            key_refused: problem.as_ref().is_some_and(|p| p.stuck == Stuck::KeyRefused),
            reconnecting: !live.contains_key(id) && lock(&self.retry).contains_key(id),
            needs: match problem.as_ref().map(|p| p.stuck) {
                Some(Stuck::Needs(n)) => Some(n),
                _ => None,
            },
            problem: problem.map(|p| p.words),
            tools,
            remembered: self.policy.remembered().iter().filter(|r| r.server == id).count(),
        }
    }

    /// Keeps a pasted key and connects with it. A bad key is refused before anything is kept.
    pub fn set_key(&self, id: &str, text: &str) -> Result<ConnectedApp, AppsError> {
        title_of(id)?;
        let key = parse_key(text)?;
        let secret = Secret::new(key).map_err(|e| AppsError::Store(e.to_string()))?;
        self.secrets
            .set(&key_name(id)?, &secret)
            .map_err(|e| AppsError::Store(e.to_string()))?;
        log::info!(target: "pitwall_chat", "connected apps: a key was saved for {id}");
        self.connect(id)
    }

    /// Connects (again) with the saved key and lists the app's tools; a failure is kept as the app's problem, in words. The
    /// user asked: the back-off starts again.
    pub fn connect(&self, id: &str) -> Result<ConnectedApp, AppsError> {
        title_of(id)?;
        lock(&self.retry).remove(id);
        Ok(self.attempt(id))
    }

    fn attempt(&self, id: &str) -> ConnectedApp {
        let title = title_of(id).unwrap_or("The app");
        lock(&self.live).remove(id);
        // The failure in words, and whether the app refused the key (then only a new key helps).
        let outcome = (|| -> Result<Live, Problem> {
            let plain = Problem::retry;
            let key = self
                .secrets
                .get(&key_name(id).map_err(|e| plain(e.to_string()))?)
                .map_err(|e| plain(e.to_string()))?
                .ok_or_else(|| {
                    plain(format!(
                        "No key is saved for {title}. Issue one in {title}'s Settings > AI access and paste it here."
                    ))
                })?;
            let file = (self.discovery)(id).ok_or_else(|| plain(format!("Pitwall cannot find {title}'s folders.")))?;
            let link = connect_loopback(&file, key.expose()).map_err(|e| {
                let refused = matches!(e, paddock_mcp::client::ConnectError::KeyRefused { .. });
                Problem {
                    words: e.to_string(),
                    stuck: if refused { Stuck::KeyRefused } else { Stuck::No },
                }
            })?;
            let conn = Arc::new(link.into_connection(ConnectionConfig::default()));
            conn.initialize("Pitwall", env!("CARGO_PKG_VERSION")).map_err(|e| {
                let stuck = match e {
                    paddock_mcp::client::ClientError::UnsupportedVersion { .. } => Stuck::Needs(AppNeed::Update),
                    _ => Stuck::No,
                };
                Problem {
                    words: init_problem(&e, title),
                    stuck,
                }
            })?;
            let defs = conn.list_tools().map_err(|e| plain(e.to_string()))?;
            let tools = tools_of(id, title, &defs, true, &conn, &self.names);
            Ok(Live { conn, tools })
        })();
        match outcome {
            Ok(live) => {
                log::info!(target: "pitwall_chat", "connected apps: {id} connected, {} tools", live.tools.len());
                lock(&self.problems).remove(id);
                lock(&self.live).insert(id.into(), live);
            }
            Err(problem) => {
                log::warn!(target: "pitwall_chat", "connected apps: {id} not connected, cause: {}", problem.words);
                lock(&self.problems).insert(id.into(), problem);
            }
        }
        self.publish();
        self.view(id)
    }

    /// Connects every app that has a key (at start and when the chat opens); failures are each app's problem.
    pub fn connect_saved(&self) {
        for (id, _) in KNOWN {
            if self.view(id).key_saved {
                let _ = self.connect(id);
            }
        }
    }

    /// Asks again for one tool: its Always allow is forgotten.
    pub fn revoke(&self, id: &str, tool: &str) -> Result<ConnectedApp, AppsError> {
        title_of(id)?;
        self.policy.revoke(id, tool);
        log::info!(target: "pitwall_chat", "connected apps: {id} {tool} asks again");
        self.publish();
        Ok(self.view(id))
    }

    /// Forgets an app: its connection, its key and its remembered approvals.
    pub fn forget(&self, id: &str) -> Result<ConnectedApp, AppsError> {
        title_of(id)?;
        lock(&self.live).remove(id);
        lock(&self.problems).remove(id);
        self.secrets.delete(&key_name(id)?).map_err(|e| AppsError::Store(e.to_string()))?;
        self.policy.revoke_server(id);
        log::info!(target: "pitwall_chat", "connected apps: {id} forgotten");
        self.publish();
        Ok(self.view(id))
    }

    /// The assistant's tools: every connected app's.
    fn publish(&self) {
        let tools = lock(&self.live).values().flat_map(|l| l.tools.iter().cloned()).collect();
        self.assistant.set_tools(tool_set(tools));
        let views = self.views();
        if let Some(f) = lock(&self.changed).as_ref() {
            f(&views);
        }
    }
}
