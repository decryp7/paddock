use crate::source::io_error;
use crate::{Limits, LoadedPack, Origin, PackKind, PackSource, Problem, Snapshot, parse_pack};
use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io;
use std::marker::PhantomData;

/// A pack shipped inside the app: its name and text (`include_str!`).
#[derive(Debug, Clone, Copy)]
pub struct BuiltinPack {
    /// The name shown in problems (`builtin.toml`).
    pub name: &'static str,
    /// The TOML text.
    pub text: &'static str,
}

/// What the loader remembers about one user file, so an unchanged file is not parsed again and a file that turned bad
/// keeps its last good version.
struct Entry<P> {
    hash: u64,
    good: Option<P>,
    problem: Option<String>,
    warnings: Vec<String>,
}

/// Loads the built-in packs and the user's folder into a [`Snapshot`]. Holds the per-file memory; one loader per kind.
pub struct PackLoader<K: PackKind> {
    builtin: Vec<BuiltinPack>,
    source: Box<dyn PackSource>,
    limits: Limits,
    seen: BTreeMap<String, Entry<K::Pack>>,
    revision: u64,
    last: Snapshot<K::Pack>,
    kind: PhantomData<K>,
}

fn hash(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

impl<K: PackKind> PackLoader<K> {
    /// A loader over `source` with the app's built-in packs.
    pub fn new(builtin: Vec<BuiltinPack>, source: Box<dyn PackSource>, limits: Limits) -> Self {
        Self {
            builtin,
            source,
            limits,
            seen: BTreeMap::new(),
            revision: 0,
            last: Snapshot::default(),
            kind: PhantomData,
        }
    }

    /// The snapshot from the last [`reload`](Self::reload) (empty before the first).
    pub fn current(&self) -> &Snapshot<K::Pack> {
        &self.last
    }

    /// Reads everything again and builds a new snapshot. Returns `None` when nothing changed since the last one (same
    /// files, same bytes), so a reload storm that changes nothing is silent; the revision counts real changes only.
    pub fn reload(&mut self) -> Option<&Snapshot<K::Pack>> {
        let mut packs = Vec::new();
        let mut problems = Vec::new();
        let mut warnings = Vec::new();

        for b in &self.builtin {
            match parse_pack::<K>(b.text.as_bytes(), Origin::Builtin, &self.limits) {
                Ok(p) => {
                    note(&mut warnings, b.name, &p.warnings);
                    packs.push(LoadedPack {
                        name: b.name.to_string(),
                        origin: Origin::Builtin,
                        pack: p.pack,
                    });
                }
                Err(e) => problems.push(Problem {
                    file: b.name.to_string(),
                    message: e.to_string(),
                }),
            }
        }

        let mut names = match self.source.list() {
            Ok(n) => n,
            Err(e) => {
                problems.push(Problem {
                    file: "packs".into(),
                    message: format!("could not list the packs folder: {}", e.kind()),
                });
                Vec::new()
            }
        };
        names.sort();
        names.dedup();
        if names.len() > self.limits.max_packs {
            let skipped = names.split_off(self.limits.max_packs);
            problems.push(Problem {
                file: "packs".into(),
                message: format!(
                    "{} pack files are over the limit of {} and were not read",
                    skipped.len(),
                    self.limits.max_packs
                ),
            });
        }

        let mut next: BTreeMap<String, Entry<K::Pack>> = BTreeMap::new();
        for name in names {
            let bytes = match self.source.read(&name, self.limits.max_file_bytes) {
                Ok(b) => b,
                // Deleted between the listing and the read: it is gone, not a problem.
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => {
                    let old = self.seen.remove(&name);
                    let problem = Some(io_error_text(&e, &name));
                    next.insert(
                        name,
                        Entry {
                            hash: 0,
                            good: old.and_then(|o| o.good),
                            problem,
                            warnings: vec![],
                        },
                    );
                    continue;
                }
            };
            let h = hash(&bytes);
            let old = self.seen.remove(&name);
            let entry = match old {
                Some(o) if o.hash == h => o,
                old => {
                    let keep = old.and_then(|o| o.good);
                    match parse_pack::<K>(&bytes, Origin::User, &self.limits) {
                        Ok(p) => Entry {
                            hash: h,
                            good: Some(p.pack),
                            problem: None,
                            warnings: p.warnings,
                        },
                        Err(e) => Entry {
                            hash: h,
                            good: keep,
                            problem: Some(e.to_string()),
                            warnings: vec![],
                        },
                    }
                }
            };
            next.insert(name, entry);
        }

        for (name, e) in &next {
            if let Some(p) = &e.problem {
                let kept = if e.good.is_some() {
                    "; the previous version stays in use"
                } else {
                    ""
                };
                problems.push(Problem {
                    file: name.clone(),
                    message: format!("{p}{kept}"),
                });
            }
            if let Some(g) = &e.good {
                note(&mut warnings, name, &e.warnings);
                packs.push(LoadedPack {
                    name: name.clone(),
                    origin: Origin::User,
                    pack: g.clone(),
                });
            }
        }
        self.seen = next;

        let candidate = Snapshot {
            revision: self.last.revision,
            packs,
            problems,
            warnings,
        };
        if self.revision > 0 && candidate == self.last {
            return None;
        }
        self.revision += 1;
        self.last = Snapshot {
            revision: self.revision,
            ..candidate
        };
        Some(&self.last)
    }
}

fn note(out: &mut Vec<Problem>, file: &str, messages: &[String]) {
    out.extend(messages.iter().map(|m| Problem {
        file: file.to_string(),
        message: m.clone(),
    }));
}

fn io_error_text(e: &io::Error, name: &str) -> String {
    io_error(name, e).to_string()
}
