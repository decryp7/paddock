//! Test kinds shaped like the three apps' packs (invented data), and a fake source.
#![allow(dead_code)]

use paddock_packs::user_regex::{self, RegexLimits};
use paddock_packs::{Origin, PackError, PackKind, PackSource};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex};

/// Dyno-shaped: categories with rules for user packs.
pub struct Cleanup;

#[derive(Deserialize)]
pub struct CleanupRaw {
    #[serde(default)]
    pub category: Vec<CategoryRaw>,
}

#[derive(Deserialize)]
pub struct CategoryRaw {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub risk: Option<Risk>,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Safe,
    Review,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub paths: Vec<String>,
}

impl PackKind for Cleanup {
    type Raw = CleanupRaw;
    type Pack = Vec<Category>;
    const FORMAT: u32 = 1;

    fn convert(raw: CleanupRaw, origin: Origin) -> Result<Vec<Category>, PackError> {
        let mut out = Vec::new();
        for c in raw.category {
            if origin == Origin::User && c.risk == Some(Risk::Safe) {
                return Err(PackError::Rejected(format!(
                    "category `{}`: a user pack cannot mark a category safe",
                    c.id
                )));
            }
            out.push(Category {
                id: c.id,
                name: c.name,
                paths: c.paths,
            });
        }
        Ok(out)
    }
}

/// Spotter-shaped: version schemes with a user regex.
pub struct Schemes;

#[derive(Deserialize)]
pub struct SchemesRaw {
    #[serde(default)]
    pub scheme: Vec<SchemeRaw>,
}

#[derive(Deserialize)]
pub struct SchemeRaw {
    pub id: String,
    pub pattern: String,
}

impl PackKind for Schemes {
    type Raw = SchemesRaw;
    type Pack = Vec<(String, String)>;
    const FORMAT: u32 = 2;

    fn convert(raw: SchemesRaw, _origin: Origin) -> Result<Self::Pack, PackError> {
        raw.scheme
            .into_iter()
            .map(|s| {
                user_regex::compile_with(&s.pattern, false, &RegexLimits::STRICT)
                    .map(|_| (s.id.clone(), s.pattern))
                    .map_err(|e| PackError::Rejected(format!("scheme `{}`: {e}", s.id)))
            })
            .collect()
    }
}

/// An in-memory folder whose files can vanish or fail between listing and reading.
#[derive(Clone, Default)]
pub struct MemSource(Arc<Mutex<Inner>>);

#[derive(Default)]
struct Inner {
    files: BTreeMap<String, Vec<u8>>,
    vanish_on_read: Vec<String>,
    fail_on_read: Vec<String>,
    list_fails: bool,
    reads: usize,
}

impl MemSource {
    pub fn put(&self, name: &str, text: &str) {
        self.0
            .lock()
            .unwrap()
            .files
            .insert(name.into(), text.as_bytes().to_vec());
    }
    pub fn put_bytes(&self, name: &str, bytes: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .files
            .insert(name.into(), bytes.to_vec());
    }
    pub fn remove(&self, name: &str) {
        self.0.lock().unwrap().files.remove(name);
    }
    /// Listed, but deleted by the time it is read.
    pub fn vanish_on_read(&self, name: &str) {
        self.0.lock().unwrap().vanish_on_read.push(name.into());
    }
    pub fn fail_on_read(&self, name: &str) {
        self.0.lock().unwrap().fail_on_read.push(name.into());
    }
    pub fn list_fails(&self, on: bool) {
        self.0.lock().unwrap().list_fails = on;
    }
    pub fn reads(&self) -> usize {
        self.0.lock().unwrap().reads
    }
}

impl PackSource for MemSource {
    fn list(&self) -> io::Result<Vec<String>> {
        let g = self.0.lock().unwrap();
        if g.list_fails {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok(g.files.keys().cloned().collect())
    }

    fn read(&self, name: &str, max: u64) -> io::Result<Vec<u8>> {
        let mut g = self.0.lock().unwrap();
        g.reads += 1;
        if g.vanish_on_read.iter().any(|n| n == name) {
            g.files.remove(name);
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        if g.fail_on_read.iter().any(|n| n == name) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        let f = g
            .files
            .get(name)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Ok(f.iter()
            .copied()
            .take(usize::try_from(max).unwrap_or(usize::MAX).saturating_add(1))
            .collect())
    }
}

pub const GOOD_CLEANUP: &str = r#"
format = 1
[[category]]
id = "logs"
name = "Old logs"
paths = ["{cache}/logs"]
"#;
