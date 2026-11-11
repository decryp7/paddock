//! Apps, keys and pairing (ADR 0007, ADR 0009). Keys are stored as SHA-256 hashes (`apps.json`, versioned, written
//! owner-only through a new file, no backup); the secret is shown once. The admin key (used by Pitwall's own window) is a random
//! secret in an owner-only file next to the data; apps never see it.

use crate::paths::{self, Dirs};
use paddock_secrets::{Secret, SecretFile};
use pitwall_core::ids::AppId;
use pitwall_core::keys::{self, StoredKey};
use pitwall_core::limits::{AppPermissions, RateWindow};
use pitwall_core::mgmt::{ApiKeyInfo, AppInfo, CreatedKey, PairingRequest, ReplacedApp, UsageCounters};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

const MAX_APPS: usize = 64;
const MAX_PENDING: usize = 8;
const PAIRING_TTL_MS: u64 = 5 * 60 * 1000;
/// Polls with a missing or wrong token one pairing request puts up with before it is closed (the key is dropped).
const MAX_BAD_POLLS: u32 = 5;
const FILE_VERSION: u32 = 1;
const MAX_FILE: usize = 1024 * 1024;

/// What the first start of 1.1 leaves in the data folder once the admin key was replaced (ADR 0092 D11).
const ADMIN_KEY_MARKER: &str = "admin-key-rotated-1.1";

/// The admin key from its owner-only file (ADR 0057 WP-4: `admin.key` is its only store, on every OS), or a new one written
/// there when there is none or the file is not trusted (others could read it, a link, another user's file, damaged): such a
/// key may have been read, so it is replaced, never repaired. One more case (ADR 0092 W3, D11): until `marker` exists a key
/// found in the file is replaced once, because 1.0.3 wrote it through a temporary file others could read for a moment on a
/// multi-user Linux machine; the marker is written once the new key is stored. Paired apps are not affected (their keys are
/// in `apps.json`); scripts that hold the old admin key must read `admin.key` again. The key itself is never logged.
/// `None` when the OS random source failed: no key is made and nobody can authenticate as the admin.
/// The random source is injected so tests can fail it; [`Auth::open`] passes the OS one.
fn admin_key_with(rng: impl FnOnce() -> Result<[u8; 32], paths::RandomError>, file: &SecretFile, marker: &Path) -> Option<String> {
    // Only "not found" means the key is not yet marked. Any other error (a permission problem, a broken link) leaves the key
    // alone and says so: a read error must not replace a working admin key on every start.
    let marked = match std::fs::symlink_metadata(marker) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            log::warn!(target: "pitwall_server", "admin key: the upgrade marker {} could not be checked ({e}); the key is kept. Check the folder's permissions", marker.display());
            true
        }
    };
    let why = match file.read() {
        Ok(Some(k)) if k.expose().starts_with(keys::KEY_PREFIX) && marked => return Some(k.expose().to_string()),
        Ok(Some(k)) if k.expose().starts_with(keys::KEY_PREFIX) => {
            "it was made before 1.1, and a key from 1.0.3 may have been read; it is replaced once".to_string()
        }
        Ok(Some(_)) => "it does not hold a Pitwall key".to_string(),
        Ok(None) => "there is none yet".to_string(),
        Err(e) => e.to_string(),
    };
    let k = match rng() {
        Ok(b) => keys::generate(b, paths::now_ms()).secret,
        Err(e) => {
            log::error!(target: "pitwall_server", "admin key: none made ({why}): {e}. Pitwall's window cannot connect until it can");
            return None;
        }
    };
    let written = Secret::new(&k).and_then(|s| file.write(&s));
    match written {
        Ok(()) => {
            log::info!(target: "pitwall_server", "admin key: a new one in {} ({why})", file.path().display());
            match paths::write_private(marker, b"the admin key was replaced once by 1.1\n") {
                Ok(()) => log::info!(target: "pitwall_server", "admin key: upgrade marker written at {}", marker.display()),
                Err(e) => log::warn!(
                    target: "pitwall_server",
                    "admin key: the upgrade marker {} could not be written ({e}); the key is replaced again at the next start",
                    marker.display()
                ),
            }
        }
        Err(e) => log::warn!(
            target: "pitwall_server",
            "admin key: a new one for this run only ({why}); it could not be stored: {e}. Pitwall's window cannot connect until \
             {} can be written: check the data folder's owner and permissions",
            file.path().display()
        ),
    }
    Some(k)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppRecord {
    id: AppId,
    name: String,
    #[serde(default)]
    suite: bool,
    keys: Vec<StoredKey>,
    permissions: AppPermissions,
    #[serde(default)]
    last_seen_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct File {
    version: u32,
    apps: Vec<AppRecord>,
}

struct Pending {
    req: PairingRequest,
    /// SHA-256 (hex) of the poll token that only the requester holds; the token itself is never stored.
    poll_hash: String,
    /// Polls that came with a missing or wrong token. After [`MAX_BAD_POLLS`] the request is closed.
    bad_polls: u32,
    outcome: Outcome,
}

enum Outcome {
    Waiting,
    Approved(Option<String>),
    Denied,
}

/// What the poller of a pairing sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairStatus {
    /// Waiting for the user.
    Pending,
    /// Approved: the key, once.
    Approved(Option<String>),
    /// Refused or expired.
    Denied,
}

/// Who is calling.
#[derive(Debug, Clone, PartialEq)]
pub enum Principal {
    /// Pitwall's own window.
    Admin,
    /// An app with its key.
    App {
        /// The app.
        id: AppId,
        /// What it may do.
        permissions: AppPermissions,
    },
    /// A local request with no key (allowed only by `allowKeylessLocal`).
    Keyless,
}

/// Why a credential was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// No key, or not a key Pitwall knows.
    #[error("a valid Pitwall API key is required. Create one in Pitwall ▸ Apps & keys, or pair the app with POST /x/v1/pair.")]
    Unauthorized,
    /// Something in the request is not allowed.
    #[error("{0}")]
    Invalid(String),
    /// The OS random source failed, so no key or code can be made (fails closed; ADR 0092 W5).
    #[error("{0}")]
    Random(String),
}

impl From<paddock_secrets::SecretsError> for AuthError {
    fn from(e: paddock_secrets::SecretsError) -> Self {
        log::error!(target: "pitwall_server", "no pairing id made: {e}");
        AuthError::Random(e.to_string())
    }
}

impl From<paths::RandomError> for AuthError {
    fn from(e: paths::RandomError) -> Self {
        log::error!(target: "pitwall_server", "no key made: {e}");
        AuthError::Random(e.to_string())
    }
}

/// The apps store.
pub struct Auth {
    dirs: Dirs,
    apps: Mutex<Vec<AppRecord>>,
    pending: Mutex<Vec<Pending>>,
    /// `None` when the OS random source failed at start: nobody is the admin then.
    admin: Option<String>,
    rates: Mutex<HashMap<AppId, RateWindow>>,
}

impl Auth {
    /// Load (tolerating damage) and make sure the admin key exists.
    pub fn open(dirs: Dirs) -> Auth {
        Auth::open_with(dirs, paths::random32)
    }

    /// [`Auth::open`] over a given random source for the admin key (tests inject a failing one).
    fn open_with(dirs: Dirs, rng: impl FnOnce() -> Result<[u8; 32], paths::RandomError>) -> Auth {
        // The access list has no backup fallback (ADR 0092 W4): a damaged file loads nothing and says so, because a backup
        // can bring back a key revoked since. The leftovers of 1.0.3 (`apps.json.bak`, `apps.tmp`) are removed.
        for stale in [paths::backup_of(&dirs.apps()), dirs.apps().with_extension("tmp")] {
            match std::fs::remove_file(&stale) {
                Ok(()) => log::info!(target: "pitwall_server", "removed {}: the access list has no backup copy any more", stale.display()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => log::warn!(target: "pitwall_server", "could not remove {}: {e}; delete it by hand", stale.display()),
            }
        }
        let apps = match paddock_secrets::read_owner_only(&dirs.apps(), MAX_FILE) {
            Ok(None) => Vec::new(),
            Ok(Some(t)) => match serde_json::from_str::<File>(&t) {
                Ok(f) if f.version == FILE_VERSION => f.apps,
                Ok(f) => {
                    log::error!(target: "pitwall_server", "{} has version {} (expected {FILE_VERSION}): no apps or keys were loaded; every app must pair again, or restore a file of this version", dirs.apps().display(), f.version);
                    Vec::new()
                }
                Err(e) => {
                    log::error!(target: "pitwall_server", "{} is damaged ({e}): no apps or keys were loaded; every app must pair again. Delete the file to clear this message", dirs.apps().display());
                    Vec::new()
                }
            },
            Err(e) => {
                log::error!(
                    target: "pitwall_server",
                    "{} is not trusted ({e}): no apps or keys were loaded, so every app must pair again. If you did not put it there, check who can write in {}; if it is yours, delete it and pair again",
                    dirs.apps().display(),
                    dirs.root.display()
                );
                Vec::new()
            }
        };
        let admin = admin_key_with(rng, &SecretFile::new(dirs.admin_key()), &dirs.root.join(ADMIN_KEY_MARKER));
        Auth {
            dirs,
            apps: Mutex::new(apps),
            pending: Mutex::new(vec![]),
            admin,
            rates: Mutex::new(HashMap::new()),
        }
    }

    /// Writes `apps.json`. A failure is logged with the path and the cause (ER-27) and returned: the change is in memory
    /// only and is lost when the server restarts (a revoked key would work again).
    fn save(&self, apps: &[AppRecord]) -> Result<(), String> {
        let f = File {
            version: FILE_VERSION,
            apps: apps.to_vec(),
        };
        let path = self.dirs.apps();
        let written = serde_json::to_vec_pretty(&f)
            .map_err(|e| e.to_string())
            .and_then(|b| paths::write_private(&path, &b).map_err(|e| e.to_string()));
        match written {
            Ok(()) => Ok(()),
            Err(e) => {
                log::error!(
                    target: "pitwall_server",
                    "the apps and keys could not be saved to {}: {e}. The change holds until the server restarts; check the data folder's space and permissions",
                    path.display()
                );
                Err(format!("the change could not be saved to {}: {e}", path.display()))
            }
        }
    }

    /// Identify the caller from its credential.
    pub fn authenticate(&self, credential: Option<&str>, allow_keyless: bool, now: u64) -> Result<Principal, AuthError> {
        let Some(c) = credential else {
            return if allow_keyless {
                Ok(Principal::Keyless)
            } else {
                Err(AuthError::Unauthorized)
            };
        };
        if self.is_admin(c) {
            return Ok(Principal::Admin);
        }
        let mut apps = lock(&self.apps);
        for a in apps.iter_mut() {
            if a.keys.iter().any(|k| keys::matches(c, k)) {
                a.last_seen_at = Some(now);
                for k in a.keys.iter_mut().filter(|k| keys::matches(c, k)) {
                    k.last_used_at = Some(now);
                }
                return Ok(Principal::App {
                    id: a.id.clone(),
                    permissions: a.permissions.clone(),
                });
            }
        }
        Err(AuthError::Unauthorized)
    }

    /// Who a request is from, for the request log: the app's id, `pitwall` for the app's own window, `no key` or `unknown
    /// key`. A read-only look-up (unlike [`Auth::authenticate`], it marks nothing as seen); never the key itself.
    pub fn caller_label(&self, credential: Option<&str>) -> String {
        let Some(c) = credential else {
            return "no key".into();
        };
        if self.is_admin(c) {
            return "pitwall".into();
        }
        lock(&self.apps)
            .iter()
            .find(|a| a.keys.iter().any(|k| keys::matches(c, k)))
            .map_or_else(|| "unknown key".into(), |a| a.id.to_string())
    }

    /// Apply the app's request-rate limit (the window is per app).
    pub fn admit(&self, id: &AppId, per_min: Option<u32>, now: u64) -> Result<(), pitwall_core::limits::Denied> {
        lock(&self.rates).entry(id.clone()).or_default().admit(now, per_min)
    }

    /// Requests the app made in the last minute.
    pub fn requests_last_minute(&self, id: &AppId, now: u64) -> u64 {
        lock(&self.rates).get(id).map_or(0, |w| w.count_since(now) as u64)
    }

    fn is_admin(&self, c: &str) -> bool {
        self.admin.as_deref().is_some_and(|a| keys::constant_time_eq(c, a))
    }

    /// The admin key, for the local window (read from the owner-only file by the app).
    pub fn admin_matches(&self, c: &str) -> bool {
        self.is_admin(c)
    }

    /// Create an app with a first key (from the UI).
    pub fn create_app(&self, name: &str, permissions: AppPermissions, now: u64) -> Result<CreatedKey, AuthError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 60 || name.chars().any(char::is_control) {
            return Err(AuthError::Invalid("give the app a name of 1 to 60 characters".into()));
        }
        let id = AppId::from_name(name).map_err(|e| AuthError::Invalid(e.to_string()))?;
        let mut apps = lock(&self.apps);
        if apps.len() >= MAX_APPS {
            return Err(AuthError::Invalid(format!("at most {MAX_APPS} apps; remove one first")));
        }
        let key = keys::generate(paths::random32()?, now);
        let id = unique_id(&apps, id);
        let info = key_info(&key.stored);
        apps.push(AppRecord {
            id: id.clone(),
            name: name.into(),
            suite: false,
            keys: vec![key.stored],
            permissions,
            last_seen_at: None,
        });
        self.save(&apps).ok();
        Ok(CreatedKey {
            app_id: id.to_string(),
            key: info,
            secret: key.secret,
        })
    }

    /// A new key for an app (the old ones keep working until revoked).
    pub fn add_key(&self, app: &str, now: u64) -> Result<CreatedKey, AuthError> {
        let mut apps = lock(&self.apps);
        let a = apps
            .iter_mut()
            .find(|a| a.id.as_str() == app)
            .ok_or_else(|| AuthError::Invalid(format!("no app \"{app}\"")))?;
        if a.keys.len() >= 8 {
            return Err(AuthError::Invalid("an app can have at most 8 keys; revoke one first".into()));
        }
        let key = keys::generate(paths::random32()?, now);
        let info = key_info(&key.stored);
        a.keys.push(key.stored);
        let id = a.id.to_string();
        self.save(&apps).ok();
        Ok(CreatedKey {
            app_id: id,
            key: info,
            secret: key.secret,
        })
    }

    /// Revoke one key; the app stays (with no key) so its permissions survive.
    pub fn revoke_key(&self, app: &str, key_id: &str) -> bool {
        let mut apps = lock(&self.apps);
        let Some(a) = apps.iter_mut().find(|a| a.id.as_str() == app) else {
            return false;
        };
        let changed = revoke_where(a, |k| k.id == key_id) > 0;
        if changed {
            self.save(&apps).ok();
        }
        changed
    }

    /// Remove an app and all its keys.
    pub fn remove_app(&self, app: &str) -> bool {
        let mut apps = lock(&self.apps);
        let before = apps.len();
        apps.retain(|a| a.id.as_str() != app);
        let changed = apps.len() != before;
        if changed {
            self.save(&apps).ok();
        }
        changed
    }

    /// Change an app's permissions.
    pub fn set_permissions(&self, app: &str, p: AppPermissions) -> bool {
        let mut apps = lock(&self.apps);
        let Some(a) = apps.iter_mut().find(|a| a.id.as_str() == app) else {
            return false;
        };
        a.permissions = p;
        self.save(&apps).ok();
        true
    }

    /// The apps for the UI.
    pub fn list(&self, today: impl Fn(&str) -> UsageCounters, now: u64) -> Vec<AppInfo> {
        lock(&self.apps)
            .iter()
            .map(|a| AppInfo {
                id: a.id.to_string(),
                name: a.name.clone(),
                suite: a.suite,
                connected: a.last_seen_at.is_some_and(|t| now.saturating_sub(t) < 60_000),
                last_seen_at: a.last_seen_at,
                keys: a.keys.iter().map(key_info).collect(),
                permissions: a.permissions.clone(),
                today: today(a.id.as_str()),
            })
            .collect()
    }

    /// An app asks to pair (ADR 0009). The code is shown to the user in Pitwall and by the app.
    /// `re_pair`: the app asks to replace its keys (ADR 0057 §4); approving it revokes the app's older keys.
    ///
    /// Returns the request (shown to the user, with its id) and the poll token: 256 random bits that only the requester
    /// gets, in this answer. The id is 128 random bits from the OS and says nothing about other requests; polling needs
    /// both (the token as a header, ADR 0092 pairing), because the id is shown in Pitwall's window and in logs.
    pub fn request_pairing(&self, name: &str, code_random: u32, re_pair: bool, now: u64) -> Result<(PairingRequest, String), AuthError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 60 || name.chars().any(char::is_control) {
            return Err(AuthError::Invalid("send a name of 1 to 60 characters".into()));
        }
        let app_id = AppId::from_name(name).map_err(|e| AuthError::Invalid(e.to_string()))?;
        let mut p = lock(&self.pending);
        p.retain(|x| now.saturating_sub(x.req.requested_at) < PAIRING_TTL_MS);
        if p.len() >= MAX_PENDING {
            return Err(AuthError::Invalid(
                "too many pairing requests are waiting; approve or deny them in Pitwall first".into(),
            ));
        }
        let id = format!("pr-{}", paddock_secrets::random_token(16)?);
        let poll_token = paddock_secrets::random_token(32)?;
        let req = PairingRequest {
            id,
            app_id: app_id.to_string(),
            app_name: name.into(),
            code: keys::pairing_code(code_random),
            requested_at: now,
            permissions: AppPermissions::default(),
            re_pair,
            replaces: None,
        };
        p.push(Pending {
            req: req.clone(),
            poll_hash: keys::hash(&poll_token),
            bad_polls: 0,
            outcome: Outcome::Waiting,
        });
        drop(p);
        Ok((self.with_replaces(req), poll_token))
    }

    /// For a re-pair, the app Pitwall has under that id (its stored name, not the requester's) and how many keys an
    /// approval revokes; read now, so the count is what the approval would do.
    fn with_replaces(&self, mut req: PairingRequest) -> PairingRequest {
        req.replaces = req
            .re_pair
            .then(|| {
                lock(&self.apps).iter().find(|a| a.id.as_str() == req.app_id).map(|a| ReplacedApp {
                    app_id: a.id.to_string(),
                    app_name: a.name.clone(),
                    keys: u32::try_from(a.keys.len()).unwrap_or(u32::MAX),
                })
            })
            .flatten();
        req
    }

    /// Pairing requests waiting for the user.
    pub fn pairings(&self, now: u64) -> Vec<PairingRequest> {
        let waiting: Vec<PairingRequest> = lock(&self.pending)
            .iter()
            .filter(|x| matches!(x.outcome, Outcome::Waiting) && now.saturating_sub(x.req.requested_at) < PAIRING_TTL_MS)
            .map(|x| x.req.clone())
            .collect();
        waiting.into_iter().map(|r| self.with_replaces(r)).collect()
    }

    /// The user approves (with the permissions to grant) or denies. `Ok(false)`: no such waiting request. An approval that
    /// cannot be saved is an error: the key is handed over and works until the server restarts, but the change is lost then.
    pub fn decide_pairing(&self, id: &str, approve: Option<AppPermissions>, now: u64) -> Result<bool, AuthError> {
        let mut p = lock(&self.pending);
        let Some(x) = p.iter_mut().find(|x| x.req.id == id && matches!(x.outcome, Outcome::Waiting)) else {
            return Ok(false);
        };
        match approve {
            None => x.outcome = Outcome::Denied,
            Some(perms) => {
                let mut apps = lock(&self.apps);
                let key = keys::generate(paths::random32()?, now);
                let secret = key.secret.clone();
                match apps.iter_mut().find(|a| a.id.as_str() == x.req.app_id) {
                    Some(a) => {
                        // A re-pair revokes every key the app has, with the one revoke rule (not by time: a clock moved
                        // back must not keep an old key alive).
                        let revoked = if x.req.re_pair { revoke_where(a, |_| true) } else { 0 };
                        log::info!(
                            target: "pitwall_server",
                            "pairing approved for {}: {} (re-pair {}, {revoked} older keys revoked)",
                            a.id,
                            if x.req.re_pair { "replaces its keys" } else { "a key beside the others" },
                            x.req.re_pair
                        );
                        a.keys.push(key.stored);
                        a.permissions = perms;
                    }
                    None => {
                        let Ok(aid) = AppId::parse(&x.req.app_id) else { return Ok(false) };
                        apps.push(AppRecord {
                            id: aid,
                            name: x.req.app_name.clone(),
                            suite: true,
                            keys: vec![key.stored],
                            permissions: perms,
                            last_seen_at: None,
                        });
                    }
                }
                let saved = self.save(&apps);
                x.outcome = Outcome::Approved(Some(secret));
                saved.map_err(|e| AuthError::Invalid(format!("the app was approved, but {e}; it is lost when Pitwall restarts")))?;
            }
        }
        Ok(true)
    }

    /// The poll: the key is handed over once, to the holder of the poll token. An unknown id, a missing or wrong token, a
    /// request closed for too many wrong polls and one that expired all answer [`PairStatus::Denied`], so a caller learns
    /// nothing about which of them it hit. A wrong token counts against the request: after [`MAX_BAD_POLLS`] it is closed.
    pub fn poll_pairing(&self, id: &str, token: Option<&str>, now: u64) -> PairStatus {
        let mut p = lock(&self.pending);
        let Some(x) = p.iter_mut().find(|x| x.req.id == id) else {
            return PairStatus::Denied;
        };
        let held = token.is_some_and(|t| keys::constant_time_eq(&keys::hash(t), &x.poll_hash));
        if !held {
            x.bad_polls += 1;
            if x.bad_polls >= MAX_BAD_POLLS && !matches!(x.outcome, Outcome::Denied) {
                log::warn!(target: "pitwall_server", "pairing request {id} closed after {MAX_BAD_POLLS} polls without its token");
                x.outcome = Outcome::Denied;
            }
            return PairStatus::Denied;
        }
        match &mut x.outcome {
            Outcome::Waiting if now.saturating_sub(x.req.requested_at) >= PAIRING_TTL_MS => PairStatus::Denied,
            Outcome::Waiting => PairStatus::Pending,
            Outcome::Approved(k) => PairStatus::Approved(k.take()),
            Outcome::Denied => PairStatus::Denied,
        }
    }
}

/// Removes the keys of `a` that match; how many went. The one revoke rule for a single key and for a re-pair.
fn revoke_where(a: &mut AppRecord, revoke: impl Fn(&StoredKey) -> bool) -> usize {
    let before = a.keys.len();
    a.keys.retain(|k| !revoke(k));
    before - a.keys.len()
}

fn key_info(k: &StoredKey) -> ApiKeyInfo {
    ApiKeyInfo {
        id: k.id.clone(),
        prefix: k.shown.clone(),
        created_at: k.created_at,
        last_used_at: k.last_used_at,
    }
}

fn unique_id(apps: &[AppRecord], base: AppId) -> AppId {
    let mut id = base.clone();
    let mut n = 2;
    while apps.iter().any(|a| a.id == id) {
        id = AppId::parse(&format!("{}-{n}", base.as_str().chars().take(28).collect::<String>())).unwrap_or_else(|_| base.clone());
        n += 1;
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An `Auth` over a temporary data folder made private like the real per-user one (a Windows CI runner's TMP inherits a
    /// drive root's entries, where `admin.key` is not stored; CI run 7821).
    fn auth() -> (tempfile::TempDir, Auth) {
        let d = tempfile::tempdir().unwrap();
        paddock_secrets::make_folder_private(d.path()).unwrap();
        let a = Auth::open(Dirs::at(d.path()));
        (d, a)
    }

    #[test]
    fn keys_authenticate_and_revoke() {
        let (d, a) = auth();
        assert_eq!(a.authenticate(None, false, 1), Err(AuthError::Unauthorized));
        assert_eq!(a.authenticate(None, true, 1), Ok(Principal::Keyless));
        let k = a.create_app("Continue (VS Code)", AppPermissions::default(), 5).unwrap();
        assert_eq!(k.app_id, "continue-vs-code");
        assert!(matches!(a.authenticate(Some(&k.secret), false, 9), Ok(Principal::App { .. })));
        assert_eq!(a.authenticate(Some("pw_nope"), false, 9), Err(AuthError::Unauthorized));
        // Only the hash is on disk.
        let text = std::fs::read_to_string(d.path().join("apps.json")).unwrap();
        assert!(!text.contains(&k.secret));
        // It survives a restart.
        let b = Auth::open(Dirs::at(d.path()));
        assert!(b.authenticate(Some(&k.secret), false, 9).is_ok());
        assert!(b.revoke_key("continue-vs-code", &k.key.id));
        assert_eq!(b.authenticate(Some(&k.secret), false, 9), Err(AuthError::Unauthorized));
        let admin = std::fs::read_to_string(d.path().join("admin.key")).unwrap();
        assert_eq!(b.authenticate(Some(admin.trim()), false, 1), Ok(Principal::Admin));
        assert!(b.admin_matches(admin.trim()));
    }

    /// ADR 0057 WP-4: `admin.key` is the only copy of the admin key, so the server writes it owner-only from the first byte
    /// and never trusts a loose one, a link or another user's file: such a key may have been read, so a new one replaces it.
    #[cfg(unix)]
    #[test]
    fn the_admin_key_file_is_owner_only_and_a_loose_or_linked_one_is_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("data");
        let key_file = root.join("admin.key");
        let a = Auth::open(Dirs::at(&root));
        assert_eq!(mode(&key_file), 0o600);
        assert_eq!(mode(&root), 0o700, "the data folder is private too");
        assert_eq!(
            std::fs::read_to_string(&key_file).unwrap(),
            a.admin.clone().unwrap(),
            "the file holds the key and nothing else"
        );
        // A restart reads the same key.
        assert_eq!(Auth::open(Dirs::at(&root)).admin, a.admin);
        // Others could read it: replaced, owner-only again.
        std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let b = Auth::open(Dirs::at(&root));
        assert_ne!(b.admin, a.admin, "a key others could read is not kept");
        assert_eq!(mode(&key_file), 0o600);
        assert_eq!(std::fs::read_to_string(&key_file).unwrap(), b.admin.clone().unwrap());
        // A link to a file elsewhere: never followed, never written through.
        let outside = d.path().join("outside");
        std::fs::write(&outside, "pw_planted").unwrap();
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::remove_file(&key_file).unwrap();
        std::os::unix::fs::symlink(&outside, &key_file).unwrap();
        let c = Auth::open(Dirs::at(&root));
        assert_ne!(c.admin.as_deref(), Some("pw_planted"));
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "pw_planted",
            "the link's target is untouched"
        );
        assert!(!std::fs::symlink_metadata(&key_file).unwrap().file_type().is_symlink());
        assert_eq!(c.authenticate(Some("pw_planted"), false, 1), Err(AuthError::Unauthorized));
        assert!(!root.join("admin.key.bak").exists(), "no backup copy of the key");
    }

    /// The request log names the caller by a read-only look-up: nothing is marked as seen (ER-27, B4 review P2-5).
    #[test]
    fn the_caller_is_named_for_the_log_without_marking_anything() {
        let (_d, a) = auth();
        let k = a.create_app("Continue (VS Code)", AppPermissions::default(), 5).unwrap();
        assert_eq!(a.caller_label(Some(&k.secret)), "continue-vs-code");
        assert_eq!(a.caller_label(None), "no key");
        assert_eq!(a.caller_label(Some("pw_nope")), "unknown key");
        let admin = a.admin.clone().unwrap();
        assert_eq!(a.caller_label(Some(&admin)), "pitwall");
        assert!(
            lock(&a.apps).iter().all(|app| app.last_seen_at.is_none()),
            "a log look-up marks nothing as seen"
        );
    }

    /// ADR 0092 test 4 (W4), second half: a damaged `apps.json` loads nothing, even when a `.bak` still holds a key that
    /// was revoked since. 1.0.3's `.bak` is removed on open, so it cannot come back later either.
    #[test]
    fn a_damaged_access_list_loads_nothing_and_a_backup_never_brings_back_a_revoked_key() {
        let (d, a) = auth();
        let k = a.create_app("one", AppPermissions::default(), 1).unwrap();
        let old = std::fs::read(d.path().join("apps.json")).unwrap();
        assert!(a.revoke_key("one", &k.key.id));
        // What 1.0.3 left behind: the list from before the revocation as `.bak`, and a damaged primary.
        paddock_secrets::write_owner_only(&d.path().join("apps.json.bak"), &old).unwrap();
        paddock_secrets::write_owner_only(&d.path().join("apps.json"), b"{broken").unwrap();
        let b = Auth::open(Dirs::at(d.path()));
        assert!(
            b.list(|_| UsageCounters::default(), 0).is_empty(),
            "nothing is loaded from a damaged list"
        );
        assert_eq!(
            b.authenticate(Some(&k.secret), false, 1),
            Err(AuthError::Unauthorized),
            "the revoked key is not back"
        );
        assert!(!d.path().join("apps.json.bak").exists(), "no backup copy is kept or used");
        b.create_app("two", AppPermissions::default(), 2).unwrap();
        assert!(!d.path().join("apps.json.bak").exists(), "a save keeps no backup either");
    }

    /// ADR 0092 test 4 (W4), first half: a link planted at `apps.tmp` (1.0.3's fixed temporary name) is never written
    /// through; the save lands in a new file of an unpredictable name and the link's target is untouched.
    #[cfg(unix)]
    #[test]
    fn a_link_planted_at_the_old_temporary_name_is_not_written_through() {
        let (d, a) = auth();
        let outside = d.path().join("outside.txt");
        std::fs::write(&outside, "precious").unwrap();
        std::os::unix::fs::symlink(&outside, d.path().join("apps.tmp")).unwrap();
        let k = a.create_app("one", AppPermissions::default(), 1).unwrap();
        assert_eq!(
            std::fs::read_to_string(&outside).unwrap(),
            "precious",
            "the link's target is untouched"
        );
        assert!(
            Auth::open(Dirs::at(d.path())).authenticate(Some(&k.secret), false, 1).is_ok(),
            "the list was saved"
        );
    }

    /// ADR 0092 W3, D11: an admin key found without the upgrade marker (1.0.3 wrote it where others could watch) is replaced
    /// once; paired apps keep working; the next start keeps the new key.
    #[test]
    fn an_admin_key_from_before_1_1_is_replaced_once_and_paired_apps_keep_working() {
        let (d, a) = auth();
        let app = a.create_app("Debrief", AppPermissions::default(), 1).unwrap();
        let old = a.admin.clone().unwrap();
        // Back to the state 1.0.3 left: a key in the file, no marker.
        std::fs::remove_file(d.path().join(ADMIN_KEY_MARKER)).unwrap();
        let b = Auth::open(Dirs::at(d.path()));
        assert_ne!(b.admin.as_deref(), Some(old.as_str()), "the old admin key is replaced");
        assert!(!b.admin_matches(&old));
        assert!(
            b.admin_matches(&std::fs::read_to_string(d.path().join("admin.key")).unwrap()),
            "the file holds the new key"
        );
        assert!(d.path().join(ADMIN_KEY_MARKER).exists(), "the marker says it was done");
        assert!(
            b.authenticate(Some(&app.secret), false, 1).is_ok(),
            "a paired app's key still works"
        );
        let c = Auth::open(Dirs::at(d.path()));
        assert_eq!(c.admin, b.admin, "a second start keeps the key");
    }

    /// A first install has no key to replace and still gets the marker, so the next start keeps its key.
    #[test]
    fn a_fresh_install_gets_the_marker_and_keeps_its_key() {
        let (d, a) = auth();
        assert!(d.path().join(ADMIN_KEY_MARKER).exists());
        assert_eq!(Auth::open(Dirs::at(d.path())).admin, a.admin);
    }

    #[test]
    fn app_management() {
        let (_d, a) = auth();
        assert!(a.create_app("  ", AppPermissions::default(), 1).is_err());
        let k1 = a.create_app("Tool", AppPermissions::default(), 1).unwrap();
        let k2 = a.create_app("Tool", AppPermissions::default(), 1).unwrap();
        assert_eq!((k1.app_id.as_str(), k2.app_id.as_str()), ("tool", "tool-2"));
        let extra = a.add_key("tool", 2).unwrap();
        assert!(a.add_key("missing", 2).is_err());
        let p = AppPermissions {
            tools: true,
            ..Default::default()
        };
        assert!(a.set_permissions("tool", p) && !a.set_permissions("zzz", AppPermissions::default()));
        let apps = a.list(|_| UsageCounters::default(), 10);
        assert_eq!(apps[0].keys.len(), 2);
        assert!(matches!(a.authenticate(Some(&extra.secret), false, 3), Ok(Principal::App { permissions, .. }) if permissions.tools));
        assert!(a.remove_app("tool") && !a.remove_app("tool"));
        assert!(!a.revoke_key("nope", "x"));
    }

    #[test]
    fn pairing_hands_the_key_over_once() {
        let (_d, a) = auth();
        let (r, t) = a.request_pairing("Debrief", 123_456, false, 1000).unwrap();
        let t = Some(t.as_str());
        assert_eq!(r.code.len(), 6);
        assert_eq!(a.poll_pairing(&r.id, t, 1001), PairStatus::Pending);
        assert_eq!(a.pairings(1001).len(), 1);
        assert!(a.decide_pairing(&r.id, Some(AppPermissions::default()), 1002).unwrap());
        assert!(!a.decide_pairing(&r.id, None, 1002).unwrap());
        let PairStatus::Approved(Some(secret)) = a.poll_pairing(&r.id, t, 1003) else {
            panic!("approved")
        };
        assert!(matches!(a.authenticate(Some(&secret), false, 1004), Ok(Principal::App { .. })));
        assert_eq!(a.poll_pairing(&r.id, t, 1005), PairStatus::Approved(None));
        assert!(a.pairings(1005).is_empty());
    }

    fn approve(a: &Auth, name: &str, re_pair: bool, at: u64) -> String {
        let (r, t) = a.request_pairing(name, 7, re_pair, at).unwrap();
        assert!(a.decide_pairing(&r.id, Some(AppPermissions::default()), at + 1).unwrap());
        let PairStatus::Approved(Some(secret)) = a.poll_pairing(&r.id, Some(&t), at + 2) else {
            panic!("approved")
        };
        secret
    }

    fn works(a: &Auth, key: &str) -> bool {
        a.authenticate(Some(key), false, 1).is_ok()
    }

    /// ADR 0057 §4: an app that re-pairs after its update (Debrief 1.2) sends the flag; approving it revokes that app's
    /// older keys, so the key it can no longer read does not stay valid.
    #[test]
    fn approving_a_re_pair_revokes_the_older_keys_of_that_app() {
        let (_d, a) = auth();
        let first = approve(&a, "Debrief", false, 1000);
        let extra = a.add_key("debrief", 2000).unwrap().secret;
        let other = approve(&a, "Pitcrew", false, 2500);
        // A key made "later" than the approval (the clock was moved back) is the app's too, and goes with the others.
        let future = a.add_key("debrief", 9_000).unwrap().secret;
        let (r, t) = a.request_pairing("Debrief", 9, true, 3000).unwrap();
        assert!(r.re_pair, "the request says it replaces the app's keys");
        // Review P2-3: the approval shows the app Pitwall has, by its stored name and id, and how many keys go.
        let shown = &a.pairings(3000)[0];
        let replaces = shown.replaces.as_ref().expect("the app exists");
        assert_eq!(
            (replaces.app_id.as_str(), replaces.app_name.as_str(), replaces.keys),
            ("debrief", "Debrief", 3)
        );
        assert!(
            works(&a, &first) && works(&a, &extra),
            "nothing is revoked before the user approves"
        );
        assert!(a.decide_pairing(&r.id, Some(AppPermissions::default()), 3001).unwrap());
        let PairStatus::Approved(Some(new)) = a.poll_pairing(&r.id, Some(&t), 3002) else {
            panic!("approved")
        };
        assert!(works(&a, &new), "the new key works");
        assert!(
            !works(&a, &first) && !works(&a, &extra) && !works(&a, &future),
            "every key the app had is revoked"
        );
        assert!(works(&a, &other), "another app's keys are untouched");
        let apps = a.list(|_| UsageCounters::default(), 4000);
        let debrief = apps.iter().find(|x| x.id == "debrief").unwrap();
        assert_eq!(debrief.keys.len(), 1, "only the new key is listed");
        // It survives a restart: the revocation was saved.
        let b = Auth::open(Dirs::at(_d.path()));
        assert!(b.authenticate(Some(&first), false, 1).is_err());
        assert!(b.authenticate(Some(&new), false, 1).is_ok());
    }

    /// Review P2-4: an approval that cannot be saved says so, instead of looking done (and a restart bringing back revoked
    /// keys without a word).
    #[test]
    fn an_approval_that_cannot_be_saved_is_an_error() {
        let (d, a) = auth();
        let (r, _) = a.request_pairing("Debrief", 9, false, 1000).unwrap();
        std::fs::create_dir(d.path().join("apps.json")).unwrap();
        std::fs::create_dir(d.path().join("apps.tmp")).unwrap();
        let e = a.decide_pairing(&r.id, Some(AppPermissions::default()), 1001).unwrap_err();
        assert!(e.to_string().contains("could not be saved"), "{e}");
        // A re-pair request for an app Pitwall does not know names nothing to replace.
        let (fresh, _) = a.request_pairing("Spotter", 9, true, 1002).unwrap();
        assert!(fresh.re_pair && fresh.replaces.is_none());
    }

    /// A pairing without the flag (a second machine under the same app id) keeps every older key, and so does a denied
    /// re-pair.
    #[test]
    fn a_pairing_without_the_flag_or_a_denied_re_pair_revokes_nothing() {
        let (_d, a) = auth();
        let first = approve(&a, "Debrief", false, 1000);
        let second = approve(&a, "Debrief", false, 2000);
        assert!(works(&a, &first) && works(&a, &second));
        assert!(!a.request_pairing("Debrief", 1, false, 2500).unwrap().0.re_pair);
        let (r, _) = a.request_pairing("Debrief", 9, true, 3000).unwrap();
        assert!(a.decide_pairing(&r.id, None, 3001).unwrap());
        assert!(works(&a, &first) && works(&a, &second), "a denied re-pair revokes nothing");
        // A re-pair of an app Pitwall does not know yet just creates it.
        let fresh = approve(&a, "Spotter", true, 4000);
        assert!(works(&a, &fresh));
    }

    #[test]
    fn pairing_denial_expiry_and_flood() {
        let (_d, a) = auth();
        let (r, t) = a.request_pairing("X", 1, false, 0).unwrap();
        assert!(a.decide_pairing(&r.id, None, 1).unwrap());
        assert_eq!(a.poll_pairing(&r.id, Some(&t), 2), PairStatus::Denied);
        let (r, t) = a.request_pairing("Y", 1, false, 0).unwrap();
        assert_eq!(a.poll_pairing(&r.id, Some(&t), PAIRING_TTL_MS + 1), PairStatus::Denied);
        assert_eq!(a.poll_pairing("nope", Some(&t), 1), PairStatus::Denied);
        assert!(a.request_pairing("bad\nname", 1, false, 0).is_err());
        for i in 0..MAX_PENDING {
            let _ = a.request_pairing(&format!("app{i}"), 1, false, 10);
        }
        assert!(a.request_pairing("one-more", 1, false, 10).is_err());
    }

    #[test]
    fn rate_window_per_app() {
        let (_d, a) = auth();
        let id = AppId::parse("a").unwrap();
        assert!(a.admit(&id, Some(1), 0).is_ok());
        assert!(a.admit(&id, Some(1), 1).is_err());
        assert!(a.admit(&id, None, 2).is_ok());
    }

    /// ADR 0092 W4, the read side: with the data folder somewhere others can write, a list they planted is not trusted. A
    /// file others can read or change (mode 0644) and a link to a list of ours elsewhere both load nothing, with the reason
    /// in the log, and the keys in them do not work.
    #[cfg(unix)]
    #[test]
    fn a_planted_access_list_is_not_loaded() {
        use std::os::unix::fs::PermissionsExt;
        let (d, a) = auth();
        let k = a.create_app("one", AppPermissions::default(), 1).unwrap();
        let good = d.path().join("apps.json");
        let copy = std::fs::read(&good).unwrap();
        // Loose mode: someone else could have written it.
        std::fs::set_permissions(&good, std::fs::Permissions::from_mode(0o644)).unwrap();
        let b = Auth::open(Dirs::at(d.path()));
        assert!(b.list(|_| UsageCounters::default(), 0).is_empty(), "a 0644 list loads nothing");
        assert_eq!(b.authenticate(Some(&k.secret), false, 1), Err(AuthError::Unauthorized));
        // A link, even to a good 0600 list of ours.
        let elsewhere = d.path().join("elsewhere.json");
        paddock_secrets::write_owner_only(&elsewhere, &copy).unwrap();
        std::fs::remove_file(&good).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &good).unwrap();
        let c = Auth::open(Dirs::at(d.path()));
        assert!(c.list(|_| UsageCounters::default(), 0).is_empty(), "a linked list loads nothing");
        assert_eq!(c.authenticate(Some(&k.secret), false, 1), Err(AuthError::Unauthorized));
        // And a list of ours is loaded as before.
        std::fs::remove_file(&good).unwrap();
        paddock_secrets::write_owner_only(&good, &copy).unwrap();
        assert!(Auth::open(Dirs::at(d.path())).authenticate(Some(&k.secret), false, 1).is_ok());
    }

    /// ADR 0092 pairing: ids are 128 random bits and unrelated to each other; the poll token is a separate 256 bits.
    #[test]
    fn two_pairing_requests_get_unrelated_ids_and_tokens() {
        let (_d, a) = auth();
        let (r1, t1) = a.request_pairing("Debrief", 1, false, 1000).unwrap();
        let (r2, t2) = a.request_pairing("Pitcrew", 1, false, 1000).unwrap();
        for id in [&r1.id, &r2.id] {
            let hex = id.strip_prefix("pr-").expect("pr- and hex");
            assert_eq!(hex.len(), 32);
            assert!(hex.bytes().all(|c| c.is_ascii_hexdigit()));
        }
        assert_ne!(r1.id, r2.id);
        assert_eq!((t1.len(), t2.len()), (64, 64));
        assert_ne!(t1, t2);
        assert_ne!(r1.id.strip_prefix("pr-"), Some(&t1[..32]), "the id is not a piece of the token");
    }

    /// The key is handed over only to the holder of the token, once. A poll without the token, with a wrong one or with
    /// another request's token never gets it, and every such answer is the same as for an unknown id.
    #[test]
    fn a_poll_without_the_token_never_gets_the_key() {
        let (_d, a) = auth();
        let (r, t) = a.request_pairing("Debrief", 1, false, 1000).unwrap();
        let (_other, other_t) = a.request_pairing("Pitcrew", 1, false, 1000).unwrap();
        assert!(a.decide_pairing(&r.id, Some(AppPermissions::default()), 1001).unwrap());
        let unknown = a.poll_pairing("pr-00000000000000000000000000000000", Some(&t), 1002);
        assert_eq!(unknown, PairStatus::Denied);
        for bad in [None, Some("wrong"), Some(other_t.as_str()), Some("")] {
            assert_eq!(a.poll_pairing(&r.id, bad, 1002), unknown, "{bad:?}");
        }
        let PairStatus::Approved(Some(key)) = a.poll_pairing(&r.id, Some(&t), 1003) else {
            panic!("the holder of the token gets the key")
        };
        assert!(matches!(a.authenticate(Some(&key), false, 1004), Ok(Principal::App { .. })));
        assert_eq!(a.poll_pairing(&r.id, Some(&t), 1005), PairStatus::Approved(None), "once");
    }

    /// Too many polls with a wrong token close the request: the key is dropped and even the right token gets nothing.
    #[test]
    fn wrong_polls_are_capped_per_request() {
        let (_d, a) = auth();
        let (r, t) = a.request_pairing("Debrief", 1, false, 1000).unwrap();
        assert!(a.decide_pairing(&r.id, Some(AppPermissions::default()), 1001).unwrap());
        for _ in 0..MAX_BAD_POLLS {
            assert_eq!(a.poll_pairing(&r.id, Some("guess"), 1002), PairStatus::Denied);
        }
        assert_eq!(a.poll_pairing(&r.id, Some(&t), 1003), PairStatus::Denied, "the request is closed");
    }

    /// ADR 0092 W5 for the admin key: with a failing OS random source there is no admin key, nobody authenticates as the
    /// admin (not even with an empty credential), and no upgrade marker is written.
    #[test]
    fn a_failing_random_source_leaves_no_admin_key_and_no_marker() {
        let d = tempfile::tempdir().unwrap();
        paddock_secrets::make_folder_private(d.path()).unwrap();
        let a = Auth::open_with(Dirs::at(d.path()), || Err(paths::RandomError("no entropy".into())));
        assert_eq!(a.admin, None);
        for c in ["", "pw_x", "None"] {
            assert_eq!(a.authenticate(Some(c), false, 1), Err(AuthError::Unauthorized), "{c:?}");
            assert!(!a.admin_matches(c));
        }
        assert!(!d.path().join(ADMIN_KEY_MARKER).exists(), "no marker");
        assert!(!d.path().join("admin.key").exists(), "no key file");
    }

    /// The marker says "done": it is written only when the new key was stored. A key that cannot be written (here the
    /// path is a folder) is used for this run and the next start tries again.
    #[test]
    fn a_refused_key_write_writes_no_marker() {
        let d = tempfile::tempdir().unwrap();
        paddock_secrets::make_folder_private(d.path()).unwrap();
        std::fs::create_dir(d.path().join("admin.key")).unwrap();
        let a = Auth::open(Dirs::at(d.path()));
        assert!(a.admin.is_some(), "a key for this run");
        assert!(!d.path().join(ADMIN_KEY_MARKER).exists(), "no marker without a stored key");
    }
}
