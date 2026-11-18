//! One shared batched reader of a git repository's objects (ADR 0108 G2).
//!
//! A program that reads many files or trees at a revision (`git show <rev>:<path>`, `git cat-file blob`, `git ls-tree -r`) pays one
//! git start per item: about 5 ms on the Mac and 30 to 45 ms on the Windows CI runner, where an empty program alone takes about 30 ms
//! to start (docs/benchmarks.md, 2026-11-18). [`Reader`] answers every such read through **one** `git cat-file --batch-command`
//! process, started on the first read and kept for the reader's life: blobs ([`Reader::file_at`], [`Reader::files_at`]), any object
//! ([`Reader::object`]), existence and type ([`Reader::info`]), one tree ([`Reader::tree`]) and a whole tree recursively
//! ([`Reader::walk`], the `ls-tree -r` of a revision). [`Reader::starts`] counts the git processes it started, for spawn-budget tests.
//!
//! A missing object or path is `None`, never an error. A failure of the git process (it ended, did not answer in time, answered
//! something unparseable) is an error naming the request and git's last words (ER-27); the next read starts a new process.
//!
//! The caller passes the `git` command to use ([`Reader::new`]): the program, the repository (`-C <root>`) and the environment are
//! its policy (paddock-dev removes the repository-selecting variables a hook sets; ADR 0100). Reading is all a reader does: it never
//! writes to the repository.

#![deny(missing_docs)]

use paddock_proc::{Session, SessionError, SessionLimits};
use std::process::Command;
use std::time::Duration;

/// The longest one answer may take before the process is ended (a hang guard: a blob of the largest tracked file reads in well
/// under a second; ER-12).
pub const READ_TIMEOUT: Duration = Duration::from_secs(120);
/// The largest object one read returns (a tracked file over it is an error, never a cut answer).
pub const MAX_OBJECT: usize = 256 * 1024 * 1024;

/// An object's type, as git names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A file's content.
    Blob,
    /// A folder listing.
    Tree,
    /// A commit.
    Commit,
    /// An annotated tag.
    Tag,
}

impl Kind {
    fn parse(word: &str) -> Option<Kind> {
        match word {
            "blob" => Some(Kind::Blob),
            "tree" => Some(Kind::Tree),
            "commit" => Some(Kind::Commit),
            "tag" => Some(Kind::Tag),
            _ => None,
        }
    }
}

/// What `info` says of an object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// The full object id (hex).
    pub oid: String,
    /// Its type.
    pub kind: Kind,
    /// Its size in bytes.
    pub size: usize,
}

/// An object and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    /// Its id, type and size.
    pub info: Info,
    /// Its raw content (a tree in git's binary form; see [`Reader::tree`]).
    pub data: Vec<u8>,
}

/// One entry of a tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The mode as git writes it (`100644`, `100755`, `120000`, `40000`, `160000`).
    pub mode: String,
    /// The name (one path component, or, from [`Reader::walk`], the path from the walked tree with `/`), as bytes may not be
    /// UTF-8: lossy.
    pub path: String,
    /// The object id (hex).
    pub oid: String,
}

impl Entry {
    /// A subtree.
    pub fn is_tree(&self) -> bool {
        self.mode == "40000"
    }

    /// A submodule (a commit in a tree).
    pub fn is_submodule(&self) -> bool {
        self.mode == "160000"
    }
}

/// Why a read failed.
#[derive(Debug)]
pub enum GitError {
    /// The git process failed, ended, or did not answer in time.
    Process(SessionError),
    /// git answered something this reader cannot read.
    Protocol(String),
    /// The object is not what the read asked for (a tree where a file was asked).
    WrongKind {
        /// The request.
        spec: String,
        /// What it is.
        kind: Kind,
    },
    /// The request holds a newline, which the batch protocol cannot carry.
    BadRequest(String),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::Process(e) => write!(f, "git cat-file: {e}"),
            GitError::Protocol(s) => write!(f, "git cat-file answered something unreadable: {s}"),
            GitError::WrongKind { spec, kind } => write!(f, "{spec} is a {kind:?}, not a file"),
            GitError::BadRequest(s) => write!(
                f,
                "{s:?} holds a newline, which git cat-file cannot be asked"
            ),
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GitError::Process(e) => Some(e),
            _ => None,
        }
    }
}

impl From<SessionError> for GitError {
    fn from(e: SessionError) -> GitError {
        GitError::Process(e)
    }
}

/// The batched reader (see the crate).
pub struct Reader {
    gix: Option<Gix>,
    make: Box<dyn Fn() -> Command + Send>,
    session: Option<Session>,
    starts: usize,
    limits: SessionLimits,
}

impl std::fmt::Debug for Reader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reader")
            .field("starts", &self.starts)
            .finish()
    }
}

impl Reader {
    /// A reader that starts `git` as `make()` builds it (the program, `-C <root>`, the environment), with `cat-file
    /// --batch-command` appended. Nothing starts until the first read.
    pub fn new(make: impl Fn() -> Command + Send + 'static) -> Reader {
        Reader {
            gix: None,
            make: Box::new(make),
            session: None,
            starts: 0,
            limits: SessionLimits::new(READ_TIMEOUT).max_answer(MAX_OBJECT),
        }
    }

    /// A reader of the repository at `root` that answers in process with gix where it can (a revision named by its full object
    /// id: `<oid>`, `<oid>^{tree}`, `<oid>:<path>`), and through `make()`'s git otherwise: a ref name, a short id, or an object
    /// gix does not have (a partial clone's omitted blob, which git fetches from the promisor remote). Nothing starts until git is
    /// needed; a folder gix cannot open is read through git alone.
    pub fn open(root: &std::path::Path, make: impl Fn() -> Command + Send + 'static) -> Reader {
        Reader {
            gix: Gix::open(root),
            ..Reader::new(make)
        }
    }

    /// Whether reads are answered in process (gix opened the repository).
    pub fn in_process(&self) -> bool {
        self.gix.is_some()
    }

    /// The same reader with another limit per answer (tests).
    pub fn with_read_timeout(mut self, timeout: Duration) -> Reader {
        self.limits.read_timeout = timeout;
        self
    }

    /// How many git processes this reader has started.
    pub fn starts(&self) -> usize {
        self.starts
    }

    fn session(&mut self) -> Result<&mut Session, GitError> {
        if self.session.is_none() {
            let mut cmd = (self.make)();
            cmd.args(["cat-file", "--batch-command"]);
            self.starts += 1;
            self.session = Some(Session::start(cmd, self.limits)?);
        }
        // The arm above filled it.
        self.session
            .as_mut()
            .ok_or_else(|| GitError::Protocol("no process".into()))
    }

    /// Sends one command and reads its header line; on any failure the process is dropped (the next read starts a new one).
    fn ask(&mut self, command: &str, spec: &str) -> Result<Option<Info>, GitError> {
        if spec.contains('\n') {
            return Err(GitError::BadRequest(spec.to_string()));
        }
        let r = (|| {
            let s = self.session()?;
            s.send(format!("{command} {spec}\n").as_bytes())?;
            let line = String::from_utf8_lossy(&s.read_line()?).into_owned();
            parse_header(&line, spec)
        })();
        if r.is_err() {
            self.session = None;
        }
        r
    }

    /// The id, type and size of `spec` (any revision expression: `<rev>:<path>`, `<oid>`, `<rev>^{tree}`), `None` when it does not
    /// exist.
    ///
    /// # Errors
    ///
    /// When git fails or answers something unreadable.
    pub fn info(&mut self, spec: &str) -> Result<Option<Info>, GitError> {
        match self.gix.as_ref().map(|g| g.lookup(spec)) {
            Some(Said::Found(o)) => Ok(Some(o.info)),
            Some(Said::Absent) => Ok(None),
            Some(Said::AskGit) | None => self.ask("info", spec),
        }
    }

    /// The object `spec` names, `None` when it does not exist.
    ///
    /// # Errors
    ///
    /// When git fails or answers something unreadable.
    pub fn object(&mut self, spec: &str) -> Result<Option<Object>, GitError> {
        if spec.contains('\n') {
            return Err(GitError::BadRequest(spec.to_string()));
        }
        match self.gix.as_ref().map(|g| g.lookup(spec)) {
            Some(Said::Found(o)) => return Ok(Some(o)),
            Some(Said::Absent) => return Ok(None),
            Some(Said::AskGit) | None => {}
        }
        let Some(info) = self.ask("contents", spec)? else {
            return Ok(None);
        };
        let r = (|| {
            let s = self.session()?;
            let mut data = s.read_exact(info.size + 1)?;
            if data.pop() != Some(b'\n') {
                return Err(GitError::Protocol(format!(
                    "the content of {spec} does not end with a newline"
                )));
            }
            Ok(data)
        })();
        match r {
            Ok(data) => Ok(Some(Object { info, data })),
            Err(e) => {
                self.session = None;
                Err(e)
            }
        }
    }

    /// The bytes of `path` at `rev`, `None` when the revision has no such file.
    ///
    /// # Errors
    ///
    /// When git fails, or the path is a folder or a submodule.
    pub fn file_at(&mut self, rev: &str, path: &str) -> Result<Option<Vec<u8>>, GitError> {
        let spec = format!("{rev}:{path}");
        match self.object(&spec)? {
            None => Ok(None),
            Some(o) if o.info.kind == Kind::Blob => Ok(Some(o.data)),
            Some(o) => Err(GitError::WrongKind {
                spec,
                kind: o.info.kind,
            }),
        }
    }

    /// [`Reader::file_at`] for each `(rev, path)`, in order, through the one process.
    pub fn files_at(&mut self, items: &[(&str, &str)]) -> Vec<Result<Option<Vec<u8>>, GitError>> {
        items
            .iter()
            .map(|(rev, path)| self.file_at(rev, path))
            .collect()
    }

    /// The entries of the tree `spec` names (a tree, or a commit's `<rev>^{tree}`), one level, `None` when it does not exist.
    ///
    /// # Errors
    ///
    /// When git fails, the object is not a tree, or the tree is malformed.
    pub fn tree(&mut self, spec: &str) -> Result<Option<Vec<Entry>>, GitError> {
        let Some(o) = self.object(spec)? else {
            return Ok(None);
        };
        if o.info.kind != Kind::Tree {
            return Err(GitError::WrongKind {
                spec: spec.to_string(),
                kind: o.info.kind,
            });
        }
        parse_tree(&o.data, o.info.oid.len() / 2).map(Some)
    }

    /// Every file of the tree `spec` names, recursively, with paths from that tree joined by `/` (the `git ls-tree -r` of a
    /// revision: subtrees are walked, not listed; submodules are listed). `None` when it does not exist.
    ///
    /// # Errors
    ///
    /// As [`Reader::tree`].
    pub fn walk(&mut self, spec: &str) -> Result<Option<Vec<Entry>>, GitError> {
        let Some(top) = self.tree(spec)? else {
            return Ok(None);
        };
        let mut out = Vec::new();
        let mut stack: Vec<(String, Vec<Entry>)> = vec![(String::new(), top)];
        while let Some((prefix, entries)) = stack.pop() {
            for e in entries {
                let path = if prefix.is_empty() {
                    e.path.clone()
                } else {
                    format!("{prefix}/{}", e.path)
                };
                if e.is_tree() {
                    let sub = self.tree(&e.oid)?.ok_or_else(|| {
                        GitError::Protocol(format!("the subtree {path} ({}) is missing", e.oid))
                    })?;
                    stack.push((path, sub));
                } else {
                    out.push(Entry { path, ..e });
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Some(out))
    }
}

/// The in-process side of a [`Reader`]: gix on the repository's folder, for the reads whose revision is a full object id.
struct Gix {
    repo: gix::Repository,
}

/// What gix could say about a request.
enum Said {
    /// The object, read in process.
    Found(Object),
    /// The revision's tree has no such path: absent, as `cat-file` would say.
    Absent,
    /// gix cannot answer (a ref name, a short id, an object it does not have: a partial clone's omitted blob): ask git.
    AskGit,
}

impl Gix {
    fn open(root: &std::path::Path) -> Option<Gix> {
        let mut repo = gix::open(root).ok()?;
        repo.object_cache_size_if_unset(16 * 1024 * 1024);
        Some(Gix { repo })
    }

    fn id(hex: &str) -> Option<gix::ObjectId> {
        (matches!(hex.len(), 40 | 64)
            && hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
        .then(|| gix::ObjectId::from_hex(hex.as_bytes()).ok())
        .flatten()
    }

    fn object(o: gix::Object<'_>) -> Option<Object> {
        let kind = match o.kind {
            gix::object::Kind::Blob => Kind::Blob,
            gix::object::Kind::Tree => Kind::Tree,
            gix::object::Kind::Commit => Kind::Commit,
            gix::object::Kind::Tag => Kind::Tag,
        };
        let o = o.detach();
        Some(Object {
            info: Info {
                oid: o.id.to_hex().to_string(),
                kind,
                size: o.data.len(),
            },
            data: o.data,
        })
    }

    /// `<oid>`, `<oid>^{tree}`, `<oid>^{commit}` and `<oid>:<path>`; anything else is git's to answer.
    fn lookup(&self, spec: &str) -> Said {
        let (rev, path) = match spec.split_once(':') {
            Some((r, p)) => (r, Some(p)),
            None => (spec, None),
        };
        let (hex, peel) = match rev.split_once("^{") {
            Some((h, p)) => (h, Some(p.trim_end_matches('}'))),
            None => (rev, None),
        };
        let Some(id) = Gix::id(hex) else {
            return Said::AskGit;
        };
        let Ok(Some(obj)) = self.repo.try_find_object(id) else {
            return Said::AskGit;
        };
        let obj = match peel {
            None => obj,
            Some("tree") => match obj.peel_to_kind(gix::object::Kind::Tree) {
                Ok(t) => t,
                Err(_) => return Said::AskGit,
            },
            Some("commit") => match obj.peel_to_kind(gix::object::Kind::Commit) {
                Ok(c) => c,
                Err(_) => return Said::AskGit,
            },
            Some(_) => return Said::AskGit,
        };
        let Some(path) = path else {
            return Gix::object(obj).map_or(Said::AskGit, Said::Found);
        };
        if path.is_empty() {
            return match obj.peel_to_kind(gix::object::Kind::Tree) {
                Ok(t) => Gix::object(t).map_or(Said::AskGit, Said::Found),
                Err(_) => Said::AskGit,
            };
        }
        let Ok(tree) = obj.peel_to_tree() else {
            return Said::AskGit;
        };
        match tree.lookup_entry_by_path(path) {
            Ok(None) => Said::Absent,
            Ok(Some(entry)) => match entry.object() {
                Ok(o) => Gix::object(o).map_or(Said::AskGit, Said::Found),
                Err(_) => Said::AskGit,
            },
            Err(_) => Said::AskGit,
        }
    }
}

/// `<oid> <type> <size>` or `<spec> missing` (also `ambiguous`): `None` for a missing object.
fn parse_header(line: &str, spec: &str) -> Result<Option<Info>, GitError> {
    if line.ends_with(" missing") || line.ends_with(" ambiguous") {
        return Ok(None);
    }
    let mut w = line.split(' ');
    let (Some(oid), Some(kind), Some(size), None) = (w.next(), w.next(), w.next(), w.next()) else {
        return Err(GitError::Protocol(format!("{line:?} for {spec}")));
    };
    let kind =
        Kind::parse(kind).ok_or_else(|| GitError::Protocol(format!("type {kind:?} for {spec}")))?;
    let size = size
        .parse()
        .map_err(|_| GitError::Protocol(format!("size {size:?} for {spec}")))?;
    if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(GitError::Protocol(format!("object id {oid:?} for {spec}")));
    }
    Ok(Some(Info {
        oid: oid.to_string(),
        kind,
        size,
    }))
}

/// git's binary tree: `<mode> <name>\0<id bytes>` repeated, ids of `id_len` bytes.
fn parse_tree(data: &[u8], id_len: usize) -> Result<Vec<Entry>, GitError> {
    let bad = |at: usize| GitError::Protocol(format!("a malformed tree at byte {at}"));
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let sp = data[i..]
            .iter()
            .position(|b| *b == b' ')
            .ok_or_else(|| bad(i))?
            + i;
        let nul = data[sp..]
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| bad(sp))?
            + sp;
        let end = nul + 1 + id_len;
        let id = data.get(nul + 1..end).ok_or_else(|| bad(nul))?;
        out.push(Entry {
            mode: String::from_utf8_lossy(&data[i..sp]).into_owned(),
            path: String::from_utf8_lossy(&data[sp + 1..nul]).into_owned(),
            oid: id.iter().map(|b| format!("{b:02x}")).collect(),
        });
        i = end;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_parse_and_a_missing_object_is_none() {
        let oid = "a".repeat(40);
        assert_eq!(
            parse_header(&format!("{oid} blob 12"), "x").unwrap(),
            Some(Info {
                oid: oid.clone(),
                kind: Kind::Blob,
                size: 12
            })
        );
        assert_eq!(
            parse_header("HEAD:nope missing", "HEAD:nope").unwrap(),
            None
        );
        assert_eq!(parse_header("abc ambiguous", "abc").unwrap(), None);
        for bad in [
            "",
            "x y",
            &format!("{oid} thing 1"),
            &format!("{oid} blob -1"),
            "zz blob 1",
        ] {
            assert!(parse_header(bad, "s").is_err(), "{bad:?}");
        }
    }

    #[test]
    fn trees_parse_and_a_cut_tree_is_an_error() {
        let mut t = b"100644 a.txt\0".to_vec();
        t.extend([0x11; 20]);
        t.extend(b"40000 d\0");
        t.extend([0x22; 20]);
        let e = parse_tree(&t, 20).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(
            (e[0].mode.as_str(), e[0].path.as_str(), e[0].oid.as_str()),
            ("100644", "a.txt", "11".repeat(20).as_str())
        );
        assert!(e[1].is_tree() && !e[0].is_tree());
        assert!(parse_tree(&t[..t.len() - 3], 20).is_err());
        assert!(parse_tree(b"100644 no-nul", 20).is_err());
    }
}
