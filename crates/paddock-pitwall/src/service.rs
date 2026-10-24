//! [`Link`]: one app's connection to Pitwall. It keeps what the app knows about the server ([`LinkView`]), looks at it
//! ([`Link::probe`]: discovery, health, the key, a model for the app), runs the pairing with the user's approval, and makes
//! the chat calls with the model Pitwall picked ([`Link::chat_with`]), recording how each went so a vanished or revoking
//! Pitwall is noticed at once. Every call is blocking and bounded: callers run it off the UI thread (Paddock rule 10).
//!
//! Pairing is never started by itself: the app calls [`Link::connect`] when the user chose to connect. A waiting pairing
//! is polled on a background thread until Pitwall answers, [`Link::cancel_pairing`], the expiry or the link going away.

use crate::client::{AppIdentity, ChatReply, Client};
use crate::discovery::{Found, Locator};
use crate::error::PitwallError;
use crate::keys::KeyVault;
use crate::loopback::{CancelToken, Timeouts};
use crate::model::{Caps, ModelSel};
use crate::request::ChatRequest;
use crate::retry::{Policy, Sleeper, ThreadSleeper};
use crate::wire::PairPoll;
use paddock_health::{Clock, SystemClock};
use paddock_secrets::Secrets;
use serde::Serialize;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

/// The client contract version this crate speaks (`/x/v1`, `apiVersion: 1`).
pub const API_VERSION: u32 = 1;
/// Longest a pairing request is waited for, whatever Pitwall says.
const MAX_PAIRING_SECS: u64 = 3600;
/// The least context a plan is sized for.
const MIN_CONTEXT: usize = 1024;

/// What the app found about Pitwall on this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum FoundKind {
    /// Never run here.
    #[default]
    Absent,
    /// Its data folder exists but no server answers.
    Installed,
    /// Answering.
    Running,
    /// Running, but it speaks another client contract version.
    Incompatible,
}

/// A pairing that waits for the user's approval in Pitwall.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingView {
    /// The six-digit code the user compares in Pitwall.
    pub code: String,
    /// When the request expires (Unix ms).
    pub expires_at_ms: u64,
}

/// Pitwall as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LinkView {
    /// Whether and how Pitwall is there.
    pub found: FoundKind,
    /// Pitwall's version.
    pub version: Option<String>,
    /// The app holds a key.
    pub paired: bool,
    /// A pairing waiting for approval.
    pub pairing: Option<PairingView>,
    /// The model Pitwall picked for the app.
    pub model: Option<String>,
    /// The context the app can plan prompts for.
    pub context_tokens: usize,
    /// The model accepts images.
    pub vision: bool,
    /// `paused` or `stopped` while Pitwall does not serve.
    pub paused: Option<String>,
    /// Connected, but no installed model fits.
    pub no_model: bool,
    /// Running, compatible, paired and a model fits.
    pub usable: bool,
    /// One plain sentence when something is wrong or just happened.
    pub problem: Option<String>,
}

/// What the owner hears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEvent {
    /// The view changed.
    Changed,
    /// A pairing was approved; `use_for_app`: the user started it from "Use Pitwall".
    Paired {
        /// See above.
        use_for_app: bool,
    },
}

/// A listener of [`LinkEvent`]s; called on whichever thread caused the change.
pub type Listener = Arc<dyn Fn(LinkEvent) + Send + Sync>;

/// The tunables of the link (tests shorten them).
#[derive(Debug, Clone, Copy)]
pub struct Tuning {
    /// How often a waiting pairing is polled.
    pub poll_every: Duration,
    /// Limits of quick calls.
    pub quick: Timeouts,
    /// Limits of chat calls.
    pub chat: Timeouts,
    /// One retry policy for every call (`None`: by priority).
    pub retry: Option<Policy>,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            poll_every: Duration::from_secs(1),
            quick: Timeouts::QUICK,
            chat: Timeouts::CHAT,
            retry: None,
        }
    }
}

/// Everything a link needs from its app.
pub struct LinkConfig {
    /// Who the app is to Pitwall.
    pub app: AppIdentity,
    /// Where Pitwall may be.
    pub locator: Locator,
    /// The app's credential store (its service is the app's display name).
    pub secrets: Arc<dyn Secrets>,
    /// What the app asks of models.
    pub needs: Caps,
    /// The preset name the user may define in Pitwall to decide what serves the app (default: the app's id).
    pub preset: Option<String>,
    /// Limits and intervals.
    pub tuning: Tuning,
    /// Time (pairing expiry: monotonic for the decision, wall for what the UI shows).
    pub clock: Arc<dyn Clock>,
    /// How retries and the pairing poll wait.
    pub sleeper: Arc<dyn Sleeper>,
}

impl LinkConfig {
    /// The real machine's defaults for `app`: system clock and sleeper, default tuning, a chat model with 8k context.
    pub fn new(app: AppIdentity, locator: Locator, secrets: Arc<dyn Secrets>) -> LinkConfig {
        LinkConfig {
            app,
            locator,
            secrets,
            needs: Caps::chat(8192),
            preset: None,
            tuning: Tuning::default(),
            clock: Arc::new(SystemClock),
            sleeper: Arc::new(ThreadSleeper),
        }
    }
}

/// What a ready link offers for one request.
#[derive(Debug, Clone)]
pub struct Ready {
    /// A client holding the key.
    pub client: Client,
    /// The model selection to send (the preset, or the capability selection).
    pub model: ModelSel,
    /// The model Pitwall picked when the app last looked.
    pub model_name: String,
    /// The context the app can plan prompts for.
    pub context_tokens: usize,
    /// The model accepts images.
    pub vision: bool,
}

struct Waiting {
    id: String,
    view: PairingView,
    cancel: CancelToken,
}

#[derive(Default)]
struct Inner {
    view: LinkView,
    port: Option<u16>,
    spec: Option<ModelSel>,
    pairing: Option<Waiting>,
    /// Pitwall answered a request of this session.
    served: bool,
}

/// One app's side of Pitwall.
pub struct Link {
    cfg: LinkConfig,
    keys: KeyVault,
    inner: Mutex<Inner>,
    listener: Mutex<Option<Listener>>,
    me: Weak<Link>,
}

fn sentence(s: &str) -> String {
    let t = s.trim().trim_end_matches('.');
    let mut c = t.chars();
    match c.next() {
        Some(f) => format!("{}{}.", f.to_uppercase(), c.as_str()),
        None => String::new(),
    }
}

impl Link {
    /// A link for the app described by `cfg`.
    pub fn new(cfg: LinkConfig) -> Arc<Link> {
        let keys = KeyVault::new(cfg.secrets.clone());
        Arc::new_cyclic(|me| Link {
            cfg,
            keys,
            inner: Mutex::default(),
            listener: Mutex::default(),
            me: me.clone(),
        })
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn app_name(&self) -> &str {
        self.cfg.app.display_name()
    }

    fn client(&self, port: u16, key: Option<paddock_secrets::Secret>) -> Client {
        let t = &self.cfg.tuning;
        let mut c = Client::new(port, self.cfg.app.clone())
            .with_key(key)
            .with_sleeper(self.cfg.sleeper.clone())
            .with_timeouts(t.quick, t.chat);
        if let Some(p) = t.retry {
            c = c.with_retry(p);
        }
        c
    }

    /// Registers the one listener (replacing an earlier one).
    pub fn set_listener(&self, l: Listener) {
        *self.listener.lock().unwrap_or_else(|p| p.into_inner()) = Some(l);
    }

    fn tell(&self, e: LinkEvent) {
        let l = self
            .listener
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(l) = l {
            l(e);
        }
    }

    /// The last view (no network).
    pub fn view(&self) -> LinkView {
        let i = self.inner();
        let mut v = i.view.clone();
        v.pairing = i.pairing.as_ref().map(|p| p.view.clone());
        v
    }

    /// What the app asked of models.
    pub fn needs(&self) -> Caps {
        self.cfg.needs
    }

    /// Whether Pitwall answered a request in this session.
    pub fn served(&self) -> bool {
        self.inner().served
    }

    /// The calendar time in Unix milliseconds, for what the UI shows.
    fn wall_ms(&self) -> u64 {
        self.cfg
            .clock
            .wall()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }

    /// Looks at Pitwall now: discovery, health, the key and a model for the app. Blocking (a few quick loopback calls);
    /// a query with no side effect except forgetting a key Pitwall rejects.
    pub fn probe(&self) -> LinkView {
        let before = self.view();
        let looked = self.look();
        {
            let mut i = self.inner();
            // A problem from a pairing or a failed request stays until the next success.
            let keep = i
                .view
                .problem
                .clone()
                .filter(|_| !looked.view.usable && looked.view.problem.is_none());
            i.view = LinkView {
                problem: looked.view.problem.or(keep),
                ..looked.view
            };
            (i.port, i.spec) = (looked.port, looked.spec);
        }
        let after = self.view();
        if after != before {
            self.tell(LinkEvent::Changed);
        }
        after
    }

    fn look(&self) -> Looked {
        let mut v = LinkView::default();
        let nothing = |v: LinkView| Looked {
            view: v,
            port: None,
            spec: None,
        };
        let (port, installed) = match self.cfg.locator.look() {
            Found::Absent => return nothing(v),
            Found::Discovered(d) if d.api_version != API_VERSION => {
                v.found = FoundKind::Incompatible;
                v.version = Some(d.version.clone());
                v.problem = Some(self.incompatible(&d.version, d.api_version));
                return nothing(v);
            }
            Found::Discovered(d) => (Some(d.port), true),
            Found::Guess {
                port, installed, ..
            } => (port, installed),
        };
        v.found = if installed {
            FoundKind::Installed
        } else {
            FoundKind::Absent
        };
        let Some(port) = port else { return nothing(v) };
        let health = match self.client(port, None).health() {
            Ok(h) => h,
            // Something else on the port, or nothing: not Pitwall.
            Err(_) => return nothing(v),
        };
        v.version = Some(health.version.chars().take(64).collect());
        if health.api_version != API_VERSION {
            v.found = FoundKind::Incompatible;
            v.problem = Some(self.incompatible(&health.version, health.api_version));
            return nothing(v);
        }
        v.found = FoundKind::Running;
        let Some(key) = self.keys.get() else {
            return Looked {
                view: v,
                port: Some(port),
                spec: None,
            };
        };
        v.paired = true;
        if !health.is_serving() {
            let state = if health.serving == "stopped" {
                "stopped"
            } else {
                "paused"
            };
            v.paused = Some(state.into());
            v.problem = Some(format!(
                "Pitwall is {state} at the moment. Resume serving in Pitwall."
            ));
            return Looked {
                view: v,
                port: Some(port),
                spec: None,
            };
        }
        let client = self.client(port, Some(key));
        let needs = self.cfg.needs;
        let preset = self
            .cfg
            .preset
            .clone()
            .unwrap_or_else(|| self.cfg.app.id().to_string());
        let found = client.model_ids().and_then(|ids| {
            let spec = if ids.contains(&preset) {
                ModelSel::Alias(preset.clone())
            } else {
                ModelSel::Auto(needs)
            };
            client.select(&spec).map(|s| (spec, s.model))
        });
        match found {
            Ok((spec, model)) => {
                let info = client
                    .models()
                    .ok()
                    .and_then(|ms| ms.into_iter().find(|m| m.id == model));
                let want = needs.context_tokens.map_or(8192, |c| c as usize);
                v.context_tokens = info
                    .as_ref()
                    .and_then(crate::wire::ModelInfo::usable_context)
                    .map_or(want, |c| {
                        usize::try_from(c)
                            .unwrap_or(usize::MAX)
                            .min(want)
                            .max(MIN_CONTEXT)
                    });
                v.vision = info.is_some_and(|m| m.can("vision"));
                v.model = Some(model.chars().take(200).collect());
                v.usable = true;
                Looked {
                    view: v,
                    port: Some(port),
                    spec: Some(spec),
                }
            }
            Err(e) => {
                if e.is_key_problem() {
                    let _ = self.keys.delete();
                    v.paired = false;
                }
                v.no_model = matches!(e, PitwallError::NoModelFits(_));
                v.problem = Some(sentence(&e.user_message(self.app_name())));
                Looked {
                    view: v,
                    port: Some(port),
                    spec: None,
                }
            }
        }
    }

    fn incompatible(&self, version: &str, api: u32) -> String {
        sentence(
            &PitwallError::Incompatible {
                version: version.into(),
                found: api,
            }
            .user_message(self.app_name()),
        )
    }

    /// What a request needs when Pitwall is usable.
    pub fn ready(&self, priority: crate::model::Priority) -> Result<Ready, PitwallError> {
        let _ = priority;
        let i = self.inner();
        let (Some(port), Some(spec), true) = (i.port, i.spec.clone(), i.view.usable) else {
            return Err(self.not_ready(&i.view));
        };
        Ok(Ready {
            client: self.client(port, self.keys.get()),
            model: spec,
            model_name: i.view.model.clone().unwrap_or_default(),
            context_tokens: i.view.context_tokens,
            vision: i.view.vision,
        })
    }

    fn not_ready(&self, v: &LinkView) -> PitwallError {
        match v.found {
            FoundKind::Absent | FoundKind::Installed => PitwallError::NotRunning,
            FoundKind::Incompatible => {
                PitwallError::NotReady(v.problem.clone().unwrap_or_default())
            }
            FoundKind::Running if !v.paired => PitwallError::NeedsPairing,
            FoundKind::Running => PitwallError::NotReady(
                v.problem
                    .clone()
                    .unwrap_or_else(|| "it has not looked at Pitwall yet".into()),
            ),
        }
    }

    /// A chat through the link: the link chooses the model (the preset or the capability selection, with `vision` added for
    /// images), the request's other fields are the caller's. How it went is recorded ([`Link::outcome`]).
    pub fn chat_with(
        &self,
        mut req: ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatReply, PitwallError> {
        let ready = self.ready(req.options.priority)?;
        req.model = ready.model;
        let result = ready.client.chat_with(&req, on_delta);
        self.outcome(result.as_ref());
        result
    }

    /// Records how a request went: served, or Pitwall is gone (stopped, key revoked).
    pub fn outcome(&self, r: Result<&ChatReply, &PitwallError>) {
        let changed = {
            let mut i = self.inner();
            let before = i.view.clone();
            match r {
                Ok(reply) => {
                    i.served = true;
                    if let Some(m) = &reply.provenance.model {
                        i.view.model = Some(m.clone());
                    }
                    i.view.problem = None;
                }
                Err(e) if e.is_gone() || e.is_key_problem() => {
                    i.view.usable = false;
                    i.view.problem = Some(sentence(&e.user_message(self.app_name())));
                    if e.is_key_problem() {
                        let _ = self.keys.delete();
                        i.view.paired = false;
                    }
                }
                Err(_) => {}
            }
            i.view != before
        };
        if changed {
            self.tell(LinkEvent::Changed);
        }
    }

    /// Starts pairing (after the user chose to connect): Pitwall shows a request with a code, the app shows the same code
    /// and polls in the background until the user approves or denies in Pitwall, it expires or [`Link::cancel_pairing`].
    /// Already usable: nothing to do. Blocking for one quick call.
    pub fn connect(&self, use_for_app: bool) -> Result<LinkView, PitwallError> {
        let view = self.probe();
        if view.usable {
            if use_for_app {
                self.tell(LinkEvent::Paired { use_for_app });
            }
            return Ok(view);
        }
        if view.pairing.is_some() {
            return Ok(view);
        }
        let port = match (view.found, self.inner().port) {
            (FoundKind::Running, Some(p)) => p,
            (FoundKind::Incompatible, _) => {
                return Err(PitwallError::NotReady(view.problem.unwrap_or_default()));
            }
            _ => return Err(PitwallError::NotRunning),
        };
        // A stored key Pitwall no longer accepts is replaced by the new one.
        let start = self.client(port, None).begin_pairing()?;
        let cancel = CancelToken::new();
        let life_ms = start.expires_in_secs.min(MAX_PAIRING_SECS) * 1000;
        // The UI shows the calendar time; the decision to give up uses the monotonic clock (it never jumps).
        let expires_at_ms = self.wall_ms().saturating_add(life_ms);
        let give_up_at = self.cfg.clock.now_ms().saturating_add(life_ms);
        {
            let mut i = self.inner();
            i.view.problem = None;
            i.pairing = Some(Waiting {
                id: start.pairing_id.clone(),
                view: PairingView {
                    code: start.code.clone(),
                    expires_at_ms,
                },
                cancel: cancel.clone(),
            });
        }
        self.tell(LinkEvent::Changed);
        let me = self.me.clone();
        let (every, sleeper) = (self.cfg.tuning.poll_every, self.cfg.sleeper.clone());
        std::thread::Builder::new()
            .name("pitwall-pairing".into())
            .spawn(move || {
                // The poll ends with an outcome, a cancel, the expiry or the owner going away.
                loop {
                    sleeper.sleep(every, &cancel);
                    let Some(link) = me.upgrade() else { return };
                    if cancel.is_cancelled() {
                        return;
                    }
                    let Some(outcome) = link.poll_once(port, &start.pairing_id, give_up_at) else {
                        continue;
                    };
                    link.finish_pairing(&start.pairing_id, outcome, use_for_app);
                    return;
                }
            })
            .map_err(|e| PitwallError::NotReady(format!("the pairing could not start: {e}")))?;
        Ok(self.view())
    }

    /// One poll of a waiting pairing: `None` keeps waiting, `Some` ends it.
    fn poll_once(&self, port: u16, id: &str, give_up_at: u64) -> Option<Result<(), PitwallError>> {
        if self.cfg.clock.now_ms() > give_up_at {
            return Some(Err(PitwallError::PairingExpired));
        }
        match self.client(port, None).poll_pairing(id) {
            Ok(PairPoll::Pending) => None,
            Ok(PairPoll::Approved(Some(key))) => Some(
                self.keys
                    .set(&key)
                    .map_err(|e| PitwallError::NotReady(format!("Pitwall approved the app, but the key could not be stored in the credential store ({e})"))),
            ),
            Ok(PairPoll::Approved(None)) => Some(Err(PitwallError::NotReady("Pitwall says the key was already delivered; connect again".into()))),
            Ok(PairPoll::Denied) => Some(Err(PitwallError::PairingDenied)),
            Err(e) if e.is_gone() => Some(Err(e)),
            // A hiccup of one poll is not the end of the pairing.
            Err(_) => None,
        }
    }

    fn finish_pairing(&self, id: &str, outcome: Result<(), PitwallError>, use_for_app: bool) {
        {
            let mut i = self.inner();
            if i.pairing.as_ref().is_none_or(|p| p.id != id) {
                return;
            }
            i.pairing = None;
            if let Err(e) = &outcome {
                i.view.problem = Some(sentence(&e.user_message(self.app_name())));
            }
        }
        if outcome.is_ok() {
            self.probe();
            self.tell(LinkEvent::Paired { use_for_app });
        }
        self.tell(LinkEvent::Changed);
    }

    /// Stops a waiting pairing; false when none waits.
    pub fn cancel_pairing(&self) -> bool {
        let p = self.inner().pairing.take();
        match p {
            Some(p) => {
                p.cancel.cancel();
                self.tell(LinkEvent::Changed);
                true
            }
            None => false,
        }
    }

    /// Deletes the app's key (Forget Pitwall).
    pub fn forget(&self) -> Result<LinkView, PitwallError> {
        self.keys
            .delete()
            .map_err(|e| PitwallError::NotReady(e.to_string()))?;
        self.inner().view.problem = None;
        Ok(self.probe())
    }
}

struct Looked {
    view: LinkView,
    port: Option<u16>,
    spec: Option<ModelSel>,
}

impl Drop for Link {
    fn drop(&mut self) {
        // A waiting pairing's thread ends at its next wake-up (it cannot upgrade the weak reference); cancel it now.
        if let Some(p) = self
            .inner
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .pairing
            .take()
        {
            p.cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sentence;

    #[test]
    fn a_message_becomes_one_sentence() {
        assert_eq!(sentence("  pitwall is paused. "), "Pitwall is paused.");
        assert_eq!(sentence("Already one."), "Already one.");
        assert_eq!(sentence(""), "");
        assert_eq!(sentence("  . "), "");
    }
}
