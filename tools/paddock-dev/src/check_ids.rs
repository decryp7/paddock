//! `./dev check new-id`: the next free conformance check id across the whole repository, not one tree (ER-21: the ids C110
//! (twice), C113 and C119 were each taken by two parallel branches; `check-ids-unique` sees one tree only).
//!
//! An id is taken when any of these names it: a file `conformance/src/checks/C<n>-*.mjs`, an `import C<n>` in that folder's
//! `index.mjs`, a `| C<n> |` row of `conformance/README.md`, a Rust check file `tools/paddock-dev/src/check/c<n>*.rs`, an
//! `id = "C<n>"` line of `docs/gates.toml`, or a reservation file `conformance/reserved/C<n>-<branch-slug>` (`--reserve <branch>`
//! creates one file per reservation, so two branches never touch the same file; the commit of that file is the reservation).
//! Two reservation files with the same id (two branches that reserved it) are an error: the scan lists both branches. The scan reads
//! the working tree and every distinct tip of every local branch and every `origin/*` branch: one `git for-each-ref`, one
//! `git ls-tree` per distinct tip, one `git cat-file --batch` for the distinct blobs; no checkout. It is bounded ([`MAX_REFS`])
//! and logs what it read and how long it took on standard error (ER-27). The id printed is the highest taken plus one.

use crate::input_hash::{Exec, Real};
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The folder of the check modules and their `index.mjs`.
const CHECKS_DIR: &str = "conformance/src/checks";
/// The folder of the reservation files, one per reservation.
pub const RESERVED_DIR: &str = "conformance/reserved";
/// The Rust-hosted checks (`c119.rs`).
const RUST_DIR: &str = "tools/paddock-dev/src/check";
/// The gate register, whose `id = "C<n>"` lines name checks too.
const GATES: &str = "docs/gates.toml";
/// The README whose table lists every check (Rust-hosted ones too).
const README: &str = "conformance/README.md";
/// The most distinct branch tips scanned; more is an error, not a silent cut (a stale-branch cleanup is the next step).
pub const MAX_REFS: usize = 3000;

/// The number of a check id word (`C121` gives 121).
fn id_number(word: &str) -> Option<u32> {
    let digits = word.strip_prefix('C')?;
    if digits.is_empty() || digits.len() > 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The id of a path that names one: a check module (`conformance/src/checks/C121-name.mjs`), a Rust check
/// (`tools/paddock-dev/src/check/c121.rs`) or a reservation file (`conformance/reserved/C121-slug`).
pub fn id_of_path(path: &str) -> Option<u32> {
    if let Some(name) = path.strip_prefix(RUST_DIR).and_then(|n| n.strip_prefix('/')) {
        let word = name.strip_prefix('c')?.split(['-', '.', '_']).next()?;
        return id_number(&format!("C{word}"));
    }
    let name = path
        .strip_prefix(CHECKS_DIR)
        .or_else(|| path.strip_prefix(RESERVED_DIR))?
        .strip_prefix('/')?;
    id_number(name.split(['-', '.']).next()?)
}

/// The branch slug of a reservation file name (`C121-feat-x` gives `feat-x`).
fn reservation_slug(path: &str) -> Option<&str> {
    path.strip_prefix(RESERVED_DIR)?.strip_prefix('/')?.split_once('-').map(|s| s.1)
}

/// A branch name as a file name part: lower-case letters, digits and dots, anything else a single `-`.
pub fn slug(branch: &str) -> String {
    let mut s = String::new();
    for c in branch.chars() {
        if c.is_ascii_alphanumeric() || c == '.' {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    s.trim_matches('-').to_string()
}

/// The ids of `id = "C<n>"` lines (the gate register).
pub fn ids_of_gates(text: &str) -> Vec<u32> {
    text.lines()
        .filter_map(|l| {
            let v = l.trim().strip_prefix("id")?.trim_start().strip_prefix('=')?.trim();
            id_number(v.trim_matches('"'))
        })
        .collect()
}

/// The ids imported by an `index.mjs` (`import C121 from ...`).
pub fn ids_of_index(text: &str) -> Vec<u32> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("import ")?.split_whitespace().next().and_then(id_number))
        .collect()
}

/// The ids of the README's table rows (`| C121 | ...`).
pub fn ids_of_readme(text: &str) -> Vec<u32> {
    text.lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix('|')?
                .trim_start()
                .split([' ', '|'])
                .next()
                .and_then(id_number)
        })
        .collect()
}

/// Where each taken id was seen (a few source names, for the report).
#[derive(Default)]
pub struct Taken {
    by_id: BTreeMap<u32, BTreeSet<String>>,
    /// Reservation file names per id (two different names for one id are a double reservation).
    reserved: BTreeMap<u32, BTreeSet<String>>,
}

impl Taken {
    fn add(&mut self, id: u32, source: &str) {
        self.by_id.entry(id).or_default().insert(source.to_string());
    }

    /// The highest id taken, or 0.
    pub fn max(&self) -> u32 {
        self.by_id.keys().next_back().copied().unwrap_or(0)
    }

    /// The first source that holds `id`.
    pub fn first_source(&self, id: u32) -> Option<&str> {
        self.by_id.get(&id)?.iter().next().map(String::as_str)
    }

    /// The ids reserved by more than one file: `(id, file names)`.
    pub fn double_reservations(&self) -> Vec<(u32, Vec<String>)> {
        self.reserved
            .iter()
            .filter(|(_, f)| f.len() > 1)
            .map(|(i, f)| (*i, f.iter().cloned().collect()))
            .collect()
    }

    fn add_file(&mut self, path: &str, text: &str, source: &str) {
        let ids: Vec<u32> = if path.ends_with("/index.mjs") {
            ids_of_index(text)
        } else if path == README {
            ids_of_readme(text)
        } else if path == GATES {
            ids_of_gates(text)
        } else {
            vec![]
        };
        for id in ids {
            self.add(id, source);
        }
    }
}

fn git(exec: &dyn Exec, root: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<String, CliError> {
    exec.run("git", args, root, stdin)
}

/// What one scan read, for the log line.
pub struct Scan {
    /// The ids found and where.
    pub taken: Taken,
    /// Branches listed.
    pub refs: usize,
    /// Distinct branch tips read.
    pub tips: usize,
    /// Distinct files read.
    pub blobs: usize,
}

/// Scans the working tree of `root` and every distinct branch tip (local and `origin/*`).
pub fn scan(exec: &dyn Exec, root: &Path) -> Result<Scan, CliError> {
    let mut taken = Taken::default();
    // The working tree: what this checkout has, committed or not.
    for dir in [CHECKS_DIR, RUST_DIR, RESERVED_DIR] {
        if let Ok(rd) = std::fs::read_dir(root.join(dir)) {
            for e in rd.flatten() {
                let path = format!("{dir}/{}", e.file_name().to_string_lossy());
                if let Some(id) = id_of_path(&path) {
                    taken.add(id, "working tree");
                    if dir == RESERVED_DIR {
                        taken.reserved.entry(id).or_default().insert(path);
                    }
                }
            }
        }
    }
    for rel in [format!("{CHECKS_DIR}/index.mjs"), README.to_string(), GATES.to_string()] {
        if let Ok(text) = std::fs::read_to_string(root.join(&rel)) {
            taken.add_file(&rel, &text, "working tree");
        }
    }
    let listing = git(
        exec,
        root,
        &[
            "for-each-ref",
            "--format=%(objectname) %(refname:short)",
            "refs/heads",
            "refs/remotes/origin",
        ],
        None,
    )?;
    // One name per distinct tip: branches that point at the same commit read the same tree.
    let mut tips: BTreeMap<String, String> = BTreeMap::new();
    let mut refs = 0usize;
    for line in listing.lines() {
        let Some((sha, name)) = line.split_once(' ') else { continue };
        if name == "origin" || name.ends_with("/HEAD") {
            continue;
        }
        refs += 1;
        tips.entry(sha.to_string()).or_insert_with(|| name.to_string());
    }
    if tips.len() > MAX_REFS {
        return Err(CliError::failure(format!(
            "::error::check new-id: {} distinct branch tips, more than the limit {MAX_REFS}.",
            tips.len()
        ))
        .with_hint("Delete merged branches (`./dev worktree clean`, `git branch -d`) and run it again."));
    }
    // Per tip: the files of the checks folder and the three listed files, as blob ids.
    let mut blobs: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for (sha, name) in &tips {
        let out = git(
            exec,
            root,
            &["ls-tree", "-r", sha, "--", CHECKS_DIR, RUST_DIR, RESERVED_DIR, README, GATES],
            None,
        )?;
        for l in out.lines() {
            let Some((meta, path)) = l.split_once('\t') else { continue };
            if let Some(id) = id_of_path(path) {
                taken.add(id, name);
                if path.starts_with(RESERVED_DIR) {
                    taken.reserved.entry(id).or_default().insert(path.to_string());
                }
            }
            let blob = meta.split_whitespace().nth(2).unwrap_or("");
            if path.ends_with("/index.mjs") || path == README || path == GATES {
                blobs.entry(blob.to_string()).or_default().push((path.to_string(), name.clone()));
            }
        }
    }
    let wanted: Vec<&String> = blobs.keys().collect();
    let texts = read_blobs(exec, root, &wanted)?;
    for (blob, users) in &blobs {
        let Some(text) = texts.get(blob) else {
            let at = users.first().map_or("?", |u| u.0.as_str());
            return Err(CliError::failure(format!(
                "::error::check new-id: git cat-file returned nothing for {at} (blob {blob}), so its ids were not read."
            ))
            .with_hint(
                "Fetch the repository (`git fetch origin`) or run `git fsck`, then run it again; a partial scan would hand out a taken id.",
            ));
        };
        for (path, name) in users {
            taken.add_file(path, text, name);
        }
    }
    Ok(Scan {
        taken,
        refs,
        tips: tips.len(),
        blobs: blobs.len(),
    })
}

/// The text of each blob, by one `git cat-file --batch`.
fn read_blobs(exec: &dyn Exec, root: &Path, blobs: &[&String]) -> Result<BTreeMap<String, String>, CliError> {
    let mut texts = BTreeMap::new();
    if blobs.is_empty() {
        return Ok(texts);
    }
    let input: String = blobs.iter().map(|b| format!("{b}\n")).collect();
    let out = git(exec, root, &["cat-file", "--batch"], Some(input.as_bytes()))?;
    // `<sha> blob <size>\n<content>\n` per blob; the content is text (a size counts bytes).
    let bytes = out.as_bytes();
    let mut at = 0usize;
    while at < bytes.len() {
        let Some(nl) = bytes[at..].iter().position(|b| *b == b'\n') else {
            break;
        };
        let head = String::from_utf8_lossy(&bytes[at..at + nl]).into_owned();
        let mut w = head.split_whitespace();
        let (Some(sha), Some(kind), Some(size)) = (w.next(), w.next(), w.next().and_then(|s| s.parse::<usize>().ok())) else {
            at += nl + 1;
            continue;
        };
        let start = at + nl + 1;
        if kind == "blob" && start + size <= bytes.len() {
            texts.insert(sha.to_string(), String::from_utf8_lossy(&bytes[start..start + size]).into_owned());
        }
        at = start + size + 1;
    }
    Ok(texts)
}

/// `./dev check new-id [--reserve <branch>]`.
pub fn command(root: Option<PathBuf>, reserve: Option<String>) -> Result<Report, CliError> {
    command_with(&Real, root, reserve, &crate::check::today_utc())
}

/// [`command`] with its ports (the git runner and today's date).
pub fn command_with(exec: &dyn Exec, root: Option<PathBuf>, reserve: Option<String>, today: &str) -> Result<Report, CliError> {
    let root = match root {
        Some(r) => r,
        None => {
            let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
            PathBuf::from(git(exec, &cwd, &["rev-parse", "--show-toplevel"], None)?.trim())
        }
    };
    let started = Instant::now();
    let s = scan(exec, &root).map_err(|e| e.with_hint("Run it inside a clone of the repository that has its origin remote fetched."))?;
    let note = format!(
        "check new-id: read {} branches ({} distinct tips, {} files) and the working tree in {} ms",
        s.refs,
        s.tips,
        s.blobs,
        started.elapsed().as_millis()
    );
    eprintln!("{note}");
    let max = s.taken.max();
    let doubles = s.taken.double_reservations();
    if !doubles.is_empty() {
        let lines: Vec<String> = doubles
            .iter()
            .map(|(id, files)| {
                let who: Vec<&str> = files.iter().map(|f| reservation_slug(f).unwrap_or(f)).collect();
                format!(
                    "::error::check new-id: C{id} is reserved by more than one branch ({}).",
                    who.join(", ")
                )
            })
            .collect();
        return Err(CliError::refused(lines.join("\n"))
            .with_hint("The later branch renumbers: delete its file under conformance/reserved/ and run `./dev check new-id --reserve <branch>` again."));
    }
    let (id, existing) = match reserve.as_deref() {
        Some(branch) => {
            let slug = slug(branch);
            if slug.is_empty() {
                return Err(CliError::usage("--reserve needs a branch name with letters or digits.")
                    .with_hint("Pass the branch that will add the check."));
            }
            let dir = root.join(RESERVED_DIR);
            let mine = std::fs::read_dir(&dir).ok().and_then(|rd| {
                rd.flatten().find_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let path = format!("{RESERVED_DIR}/{name}");
                    (reservation_slug(&path) == Some(slug.as_str()))
                        .then(|| id_of_path(&path))
                        .flatten()
                })
            });
            if let Some(id) = mine {
                (id, true)
            } else {
                let next = max + 1;
                std::fs::create_dir_all(&dir)
                    .and_then(|()| std::fs::write(dir.join(format!("C{next}-{slug}")), format!("C{next} {branch} {today}\n")))
                    .map_err(|e| {
                        CliError::failure(format!("::error::check new-id: {RESERVED_DIR} cannot be written: {e}."))
                            .with_hint("Fix the folder's rights and run it again.")
                    })?;
                (next, false)
            }
        }
        None => (max + 1, false),
    };
    let text = match (&reserve, existing) {
        (Some(b), true) => format!("C{id}\nalready reserved for {b} in {RESERVED_DIR}"),
        (Some(b), false) => format!(
            "C{id}\nreserved for {b}: commit {RESERVED_DIR}/C{id}-{} (the commit is the reservation)",
            slug(b)
        ),
        (None, _) => format!("C{id}"),
    };
    Ok(Report::new(
        json!({ "id": format!("C{id}"), "highest_taken": max, "branches": s.refs, "tips": s.tips, "reserved": reserve.is_some() }),
        text,
    ))
}
