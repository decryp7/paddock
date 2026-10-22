//! Apps, keys and pairing (ADR 0007, ADR 0009). Keys are stored as SHA-256 hashes (`apps.json`, versioned, written
//! atomically with a backup); the secret is shown once. The admin key (used by Pitwall's own window) is a random
//! secret in an owner-only file next to the data; apps never see it.

use crate::paths::{self, Dirs};
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
        let admin = std::fs::read_to_string(dirs.admin_key())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| s.starts_with(keys::KEY_PREFIX))
            .unwrap_or_else(|| {
                let k = keys::generate(paths::random32(), paths::now_ms()).secret;
                if paths::write_atomic(&dirs.admin_key(), k.as_bytes()).is_ok() {
                    paths::restrict(&dirs.admin_key());
                }
                k
            });
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

    /// Apply the app's request-rate limit (the window is per app).
    pub fn admit(&self, id: &AppId, per_min: Option<u32>, now: u64) -> Result<(), pitwall_core::limits::Denied> {
        lock(&self.rates).entry(id.clone()).or_default().admit(now, per_min)
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
        let before = a.keys.len();
        a.keys.retain(|k| k.id != key_id);
        let changed = a.keys.len() != before;
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
    pub fn request_pairing(&self, name: &str, code_random: u32, now: u64) -> Result<PairingRequest, AuthError> {
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
        let r = a.request_pairing("Debrief", 123_456, 1000).unwrap();
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

    #[test]
    fn pairing_denial_expiry_and_flood() {
        let (_d, a) = auth();
        let r = a.request_pairing("X", 1, 0).unwrap();
        assert!(a.decide_pairing(&r.id, None, 1));
        assert_eq!(a.poll_pairing(&r.id, 2), PairStatus::Denied);
        let r = a.request_pairing("Y", 1, 0).unwrap();
        assert_eq!(a.poll_pairing(&r.id, PAIRING_TTL_MS + 1), PairStatus::Denied);
        assert_eq!(a.poll_pairing("nope", 1), PairStatus::Denied);
        assert!(a.request_pairing("bad\nname", 1, 0).is_err());
        for i in 0..MAX_PENDING {
            let _ = a.request_pairing(&format!("app{i}"), 1, 10);
        }
        assert!(a.request_pairing("one-more", 1, 10).is_err());
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
