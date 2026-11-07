//! Apps, keys and pairing (ADR 0007, ADR 0009). Keys are stored as SHA-256 hashes (`apps.json`, versioned, written
//! atomically with a backup); the secret is shown once. The admin key (used by Pitwall's own window) is a random
//! secret in an owner-only file next to the data; apps never see it.

use crate::paths::{self, Dirs};
use paddock_secrets::{Secret, SecretFile};
use pitwall_core::ids::AppId;
use pitwall_core::keys::{self, StoredKey};
use pitwall_core::limits::{AppPermissions, RateWindow};
use pitwall_core::mgmt::{ApiKeyInfo, AppInfo, CreatedKey, PairingRequest, UsageCounters};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

const MAX_APPS: usize = 64;
const MAX_PENDING: usize = 8;
const PAIRING_TTL_MS: u64 = 5 * 60 * 1000;
const FILE_VERSION: u32 = 1;
const MAX_FILE: usize = 1024 * 1024;

/// The admin key from its owner-only file (ADR 0057 WP-4: `admin.key` is its only store, on every OS), or a new one written
/// there when there is none or the file is not trusted (others could read it, a link, another user's file, damaged): such a
/// key may have been read, so it is replaced, never repaired. The key itself is never logged.
fn admin_key(file: &SecretFile) -> String {
    let why = match file.read() {
        Ok(Some(k)) if k.expose().starts_with(keys::KEY_PREFIX) => return k.expose().to_string(),
        Ok(Some(_)) => "it does not hold a Pitwall key".to_string(),
        Ok(None) => "there is none yet".to_string(),
        Err(e) => e.to_string(),
    };
    let k = keys::generate(paths::random32(), paths::now_ms()).secret;
    let written = Secret::new(&k).and_then(|s| file.write(&s));
    match written {
        Ok(()) => log::info!(target: "pitwall_server", "admin key: a new one in {} ({why})", file.path().display()),
        Err(e) => log::warn!(
            target: "pitwall_server",
            "admin key: a new one for this run only ({why}); it could not be stored: {e}. Pitwall's window cannot connect until \
             {} can be written: check the data folder's owner and permissions",
            file.path().display()
        ),
    }
    k
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
}

/// The apps store.
pub struct Auth {
    dirs: Dirs,
    apps: Mutex<Vec<AppRecord>>,
    pending: Mutex<Vec<Pending>>,
    admin: String,
    rates: Mutex<HashMap<AppId, RateWindow>>,
    next_pair: Mutex<u64>,
}

impl Auth {
    /// Load (tolerating damage) and make sure the admin key exists.
    pub fn open(dirs: Dirs) -> Auth {
        let (primary, backup) = paths::read_with_backup(&dirs.apps(), MAX_FILE);
        let parse = |t: Option<String>| {
            t.and_then(|t| serde_json::from_str::<File>(&t).ok())
                .filter(|f| f.version == FILE_VERSION)
        };
        let apps = parse(primary).or_else(|| parse(backup)).map(|f| f.apps).unwrap_or_default();
        let admin = admin_key(&SecretFile::new(dirs.admin_key()));
        Auth {
            dirs,
            apps: Mutex::new(apps),
            pending: Mutex::new(vec![]),
            admin,
            rates: Mutex::new(HashMap::new()),
            next_pair: Mutex::new(0),
        }
    }

    fn save(&self, apps: &[AppRecord]) {
        let f = File {
            version: FILE_VERSION,
            apps: apps.to_vec(),
        };
        if let Ok(b) = serde_json::to_vec_pretty(&f) {
            if paths::write_atomic(&self.dirs.apps(), &b).is_ok() {
                paths::restrict(&self.dirs.apps());
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
        if keys::constant_time_eq(c, &self.admin) {
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
        if keys::constant_time_eq(c, &self.admin) {
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

    /// The admin key, for the local window (read from the owner-only file by the app).
    pub fn admin_matches(&self, c: &str) -> bool {
        keys::constant_time_eq(c, &self.admin)
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
        let (id, key) = (unique_id(&apps, id), keys::generate(paths::random32(), now));
        let info = key_info(&key.stored);
        apps.push(AppRecord {
            id: id.clone(),
            name: name.into(),
            suite: false,
            keys: vec![key.stored],
            permissions,
            last_seen_at: None,
        });
        self.save(&apps);
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
        let key = keys::generate(paths::random32(), now);
        let info = key_info(&key.stored);
        a.keys.push(key.stored);
        let id = a.id.to_string();
        self.save(&apps);
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
            self.save(&apps);
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
            self.save(&apps);
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
        self.save(&apps);
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
    pub fn request_pairing(&self, name: &str, code_random: u32, re_pair: bool, now: u64) -> Result<PairingRequest, AuthError> {
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
        let n = {
            let mut g = lock(&self.next_pair);
            *g += 1;
            *g
        };
        let req = PairingRequest {
            id: format!("pr{n}-{}", &keys::hash(&format!("{now}{n}"))[..6]),
            app_id: app_id.to_string(),
            app_name: name.into(),
            code: keys::pairing_code(code_random),
            requested_at: now,
            permissions: AppPermissions::default(),
            re_pair,
        };
        p.push(Pending {
            req: req.clone(),
            outcome: Outcome::Waiting,
        });
        Ok(req)
    }

    /// Pairing requests waiting for the user.
    pub fn pairings(&self, now: u64) -> Vec<PairingRequest> {
        lock(&self.pending)
            .iter()
            .filter(|x| matches!(x.outcome, Outcome::Waiting) && now.saturating_sub(x.req.requested_at) < PAIRING_TTL_MS)
            .map(|x| x.req.clone())
            .collect()
    }

    /// The user approves (with the permissions to grant) or denies.
    pub fn decide_pairing(&self, id: &str, approve: Option<AppPermissions>, now: u64) -> bool {
        let mut p = lock(&self.pending);
        let Some(x) = p.iter_mut().find(|x| x.req.id == id && matches!(x.outcome, Outcome::Waiting)) else {
            return false;
        };
        match approve {
            None => x.outcome = Outcome::Denied,
            Some(perms) => {
                let mut apps = lock(&self.apps);
                let key = keys::generate(paths::random32(), now);
                let secret = key.secret.clone();
                match apps.iter_mut().find(|a| a.id.as_str() == x.req.app_id) {
                    Some(a) => {
                        // A re-pair revokes the keys made before this approval, with the one revoke rule.
                        let revoked = if x.req.re_pair {
                            revoke_where(a, |k| k.created_at <= now)
                        } else {
                            0
                        };
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
                        let Ok(aid) = AppId::parse(&x.req.app_id) else { return false };
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
                self.save(&apps);
                x.outcome = Outcome::Approved(Some(secret));
            }
        }
        true
    }

    /// The poll: the key is handed over once.
    pub fn poll_pairing(&self, id: &str, now: u64) -> PairStatus {
        let mut p = lock(&self.pending);
        let Some(x) = p.iter_mut().find(|x| x.req.id == id) else {
            return PairStatus::Denied;
        };
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

    fn auth() -> (tempfile::TempDir, Auth) {
        let d = tempfile::tempdir().unwrap();
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
            a.admin,
            "the file holds the key and nothing else"
        );
        // A restart reads the same key.
        assert_eq!(Auth::open(Dirs::at(&root)).admin, a.admin);
        // Others could read it: replaced, owner-only again.
        std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let b = Auth::open(Dirs::at(&root));
        assert_ne!(b.admin, a.admin, "a key others could read is not kept");
        assert_eq!(mode(&key_file), 0o600);
        assert_eq!(std::fs::read_to_string(&key_file).unwrap(), b.admin);
        // A link to a file elsewhere: never followed, never written through.
        let outside = d.path().join("outside");
        std::fs::write(&outside, "pw_planted").unwrap();
        std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::remove_file(&key_file).unwrap();
        std::os::unix::fs::symlink(&outside, &key_file).unwrap();
        let c = Auth::open(Dirs::at(&root));
        assert_ne!(c.admin, "pw_planted");
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
        let admin = a.admin.clone();
        assert_eq!(a.caller_label(Some(&admin)), "pitwall");
        assert!(
            lock(&a.apps).iter().all(|app| app.last_seen_at.is_none()),
            "a log look-up marks nothing as seen"
        );
    }

    #[test]
    fn damaged_file_falls_back_to_the_backup_then_defaults() {
        let (d, a) = auth();
        a.create_app("one", AppPermissions::default(), 1).unwrap();
        a.create_app("two", AppPermissions::default(), 2).unwrap();
        std::fs::write(d.path().join("apps.json"), b"{broken").unwrap();
        let b = Auth::open(Dirs::at(d.path()));
        assert_eq!(
            b.list(|_| UsageCounters::default(), 0).len(),
            1,
            "the backup holds the previous save"
        );
        std::fs::write(d.path().join("apps.json.bak"), b"also broken").unwrap();
        assert!(Auth::open(Dirs::at(d.path())).list(|_| UsageCounters::default(), 0).is_empty());
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
        let r = a.request_pairing("Debrief", 123_456, false, 1000).unwrap();
        assert_eq!(r.code.len(), 6);
        assert_eq!(a.poll_pairing(&r.id, 1001), PairStatus::Pending);
        assert_eq!(a.pairings(1001).len(), 1);
        assert!(a.decide_pairing(&r.id, Some(AppPermissions::default()), 1002));
        assert!(!a.decide_pairing(&r.id, None, 1002));
        let PairStatus::Approved(Some(secret)) = a.poll_pairing(&r.id, 1003) else {
            panic!("approved")
        };
        assert!(matches!(a.authenticate(Some(&secret), false, 1004), Ok(Principal::App { .. })));
        assert_eq!(a.poll_pairing(&r.id, 1005), PairStatus::Approved(None));
        assert!(a.pairings(1005).is_empty());
    }

    fn approve(a: &Auth, name: &str, re_pair: bool, at: u64) -> String {
        let r = a.request_pairing(name, 7, re_pair, at).unwrap();
        assert!(a.decide_pairing(&r.id, Some(AppPermissions::default()), at + 1));
        let PairStatus::Approved(Some(secret)) = a.poll_pairing(&r.id, at + 2) else {
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
        let r = a.request_pairing("Debrief", 9, true, 3000).unwrap();
        assert!(r.re_pair, "the request says it replaces the app's keys");
        assert!(a.pairings(3000)[0].re_pair, "so the approval can say so");
        assert!(
            works(&a, &first) && works(&a, &extra),
            "nothing is revoked before the user approves"
        );
        assert!(a.decide_pairing(&r.id, Some(AppPermissions::default()), 3001));
        let PairStatus::Approved(Some(new)) = a.poll_pairing(&r.id, 3002) else {
            panic!("approved")
        };
        assert!(works(&a, &new), "the new key works");
        assert!(!works(&a, &first) && !works(&a, &extra), "the older keys of the app are revoked");
        assert!(works(&a, &other), "another app's keys are untouched");
        let apps = a.list(|_| UsageCounters::default(), 4000);
        let debrief = apps.iter().find(|x| x.id == "debrief").unwrap();
        assert_eq!(debrief.keys.len(), 1, "only the new key is listed");
        // It survives a restart: the revocation was saved.
        let b = Auth::open(Dirs::at(_d.path()));
        assert!(b.authenticate(Some(&first), false, 1).is_err());
        assert!(b.authenticate(Some(&new), false, 1).is_ok());
    }

    /// A pairing without the flag (a second machine under the same app id) keeps every older key, and so does a denied
    /// re-pair.
    #[test]
    fn a_pairing_without_the_flag_or_a_denied_re_pair_revokes_nothing() {
        let (_d, a) = auth();
        let first = approve(&a, "Debrief", false, 1000);
        let second = approve(&a, "Debrief", false, 2000);
        assert!(works(&a, &first) && works(&a, &second));
        assert!(!a.request_pairing("Debrief", 1, false, 2500).unwrap().re_pair);
        let r = a.request_pairing("Debrief", 9, true, 3000).unwrap();
        assert!(a.decide_pairing(&r.id, None, 3001));
        assert!(works(&a, &first) && works(&a, &second), "a denied re-pair revokes nothing");
        // A re-pair of an app Pitwall does not know yet just creates it.
        let fresh = approve(&a, "Spotter", true, 4000);
        assert!(works(&a, &fresh));
    }

    #[test]
    fn pairing_denial_expiry_and_flood() {
        let (_d, a) = auth();
        let r = a.request_pairing("X", 1, false, 0).unwrap();
        assert!(a.decide_pairing(&r.id, None, 1));
        assert_eq!(a.poll_pairing(&r.id, 2), PairStatus::Denied);
        let r = a.request_pairing("Y", 1, false, 0).unwrap();
        assert_eq!(a.poll_pairing(&r.id, PAIRING_TTL_MS + 1), PairStatus::Denied);
        assert_eq!(a.poll_pairing("nope", 1), PairStatus::Denied);
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
}
