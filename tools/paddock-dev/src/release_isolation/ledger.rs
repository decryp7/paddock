//! The ledger: one line per action kind, written only by [`append`] (the `record` command and `setup` both call it).
//!
//! ```text
//! #job<TAB><job folder><TAB><job start: birth of the job folder, ns since the epoch>
//! <kind><TAB><path or -><TAB><job|run|off><TAB><dev><TAB><ino><TAB><birth ns>     (last three empty for `off`)
//! ```
//!
//! Modes (ADR 0085 §5 item 1): `job` is a folder made by this run inside the job folder, `run` a folder made by this run
//! outside it (the `paddock-dev` tool folder), `off` a cache the run switched off (no path, no identity).

use super::identity::{Identity, identity_of};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

/// What an action chose: every folder or cache a release build could read from, except what `setup` itself makes unique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// The tagged clone.
    Workspace,
    /// The Rust build folder.
    Target,
    /// `CARGO_HOME`: registry index, `.crate` archives, extracted sources.
    CargoHome,
    /// The npm download cache.
    Npm,
    /// `node_modules`.
    NodeModules,
    /// Tauri's tool folder (`<build folder>/.tauri`, through `useLocalToolsDir`).
    TauriTools,
    /// The folder `paddock-dev` was built into for this run.
    DevTool,
    /// The compile cache: always `off` in release mode.
    Sccache,
    /// The Angular build cache.
    Angular,
    /// Debrief's `libduckdb`.
    Libduckdb,
}

impl Kind {
    /// Every kind.
    pub const ALL: [Kind; 10] = [
        Kind::Workspace,
        Kind::Target,
        Kind::CargoHome,
        Kind::Npm,
        Kind::NodeModules,
        Kind::TauriTools,
        Kind::DevTool,
        Kind::Sccache,
        Kind::Angular,
        Kind::Libduckdb,
    ];

    /// The kinds a job expects unless it names its own list (everything but Debrief's `libduckdb`).
    pub const DEFAULT_EXPECTED: [Kind; 9] = [
        Kind::Workspace,
        Kind::Target,
        Kind::CargoHome,
        Kind::Npm,
        Kind::NodeModules,
        Kind::TauriTools,
        Kind::DevTool,
        Kind::Sccache,
        Kind::Angular,
    ];

    /// The name used in the ledger and on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Workspace => "workspace",
            Kind::Target => "target",
            Kind::CargoHome => "cargo-home",
            Kind::Npm => "npm",
            Kind::NodeModules => "node-modules",
            Kind::TauriTools => "tauri-tools",
            Kind::DevTool => "dev-tool",
            Kind::Sccache => "sccache",
            Kind::Angular => "angular",
            Kind::Libduckdb => "libduckdb",
        }
    }

    /// The modes a kind may be recorded with (ADR 0085 Rust-tool amendment): `run` only for `dev-tool`, `off` only for
    /// `sccache` and, where the job does not expect it, `libduckdb`.
    pub fn allowed_modes(self) -> &'static [Mode] {
        match self {
            Kind::Sccache => &[Mode::Off],
            Kind::DevTool => &[Mode::Run],
            Kind::Libduckdb => &[Mode::Job, Mode::Off],
            _ => &[Mode::Job],
        }
    }

    /// Parses a name from [`Kind::name`].
    pub fn parse(text: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == text)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How an action chose its folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Made by this run inside the job folder.
    Job,
    /// Made by this run outside the job folder.
    Run,
    /// Switched off.
    Off,
}

impl Mode {
    /// The name used in the ledger.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Job => "job",
            Mode::Run => "run",
            Mode::Off => "off",
        }
    }

    /// Parses `job`, `run` or `off`.
    pub fn parse(text: &str) -> Option<Mode> {
        [Mode::Job, Mode::Run, Mode::Off].into_iter().find(|m| m.name() == text)
    }
}

/// One ledger line as read: the words are kept as written so that `verify` can name a bad one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The kind as written.
    pub kind: String,
    /// The path as written (`-` for `off`).
    pub path: String,
    /// The mode as written.
    pub mode: String,
    /// The identity taken when the line was written (none for `off`).
    pub id: Option<Identity>,
    /// The line number, from 1, for messages.
    pub line: usize,
}

/// The ledger of one job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger {
    /// The job's folder (`$RUNNER_TEMP`), canonical.
    pub job_root: PathBuf,
    /// When the job started: the birth time of the job's folder, ns since the epoch.
    pub started_ns: i128,
    /// The lines, in order.
    pub entries: Vec<Entry>,
}

impl Ledger {
    /// The entry of `kind`, if any.
    pub fn get(&self, kind: Kind) -> Option<&Entry> {
        self.entries.iter().find(|e| e.kind == kind.name())
    }

    /// Reads a ledger file. An unreadable file or a damaged header is an error with the next step.
    pub fn read(path: &Path) -> Result<Ledger, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("the ledger {} cannot be read: {e}", path.display()))?;
        Ledger::parse(&text).map_err(|e| format!("the ledger {} is damaged: {e}", path.display()))
    }

    /// Parses ledger text. Entry lines are kept as written even when their words are wrong; only a missing or damaged header,
    /// or a line without its columns, is an error.
    pub fn parse(text: &str) -> Result<Ledger, String> {
        let mut lines = text.lines().enumerate();
        let (_, first) = lines.next().ok_or("it is empty")?;
        let head: Vec<&str> = first.split('\t').collect();
        if head.len() != 3 || head[0] != "#job" {
            return Err(format!("line 1 is not a `#job<TAB>folder<TAB>started` header: {first:?}"));
        }
        let started_ns = head[2]
            .parse::<i128>()
            .map_err(|_| format!("line 1: the start {:?} is not a number", head[2]))?;
        let mut entries = Vec::new();
        for (i, line) in lines {
            if line.is_empty() {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() != 6 {
                return Err(format!("line {}: {} columns instead of 6", i + 1, cols.len()));
            }
            let id = if cols[3].is_empty() && cols[4].is_empty() && cols[5].is_empty() {
                None
            } else {
                Some(Identity {
                    dev: cols[3]
                        .parse::<u64>()
                        .map_err(|_| format!("line {}: {:?} is not a number", i + 1, cols[3]))?,
                    ino: cols[4]
                        .parse::<u128>()
                        .map_err(|_| format!("line {}: {:?} is not a number", i + 1, cols[4]))?,
                    birth_ns: cols[5]
                        .parse::<i128>()
                        .map_err(|_| format!("line {}: {:?} is not a number", i + 1, cols[5]))?,
                })
            };
            entries.push(Entry {
                kind: cols[0].to_string(),
                path: cols[1].to_string(),
                mode: cols[2].to_string(),
                id,
                line: i + 1,
            });
        }
        Ok(Ledger {
            job_root: PathBuf::from(head[1]),
            started_ns,
            entries,
        })
    }
}

/// A path without Windows' verbatim prefix (`\\?\C:\x` becomes `C:\x`, `\\?\UNC\s\x` becomes `\\s\x`), as `canonicalize`
/// returns it there: tools that do not know the prefix (git bash, npm, cmd) cannot use it. Other paths are returned as given.
pub fn simplified(path: &Path) -> PathBuf {
    PathBuf::from(simplify_text(&path.to_string_lossy()))
}

/// [`simplified`] on text, so that it is testable on every OS. The prefix comes off through the shared
/// [`paddock_repo_graph::strip_verbatim`]; `\\?\Volume{..}\` and other forms that name no drive or share stay as they are, because
/// without the prefix they would name nothing.
pub fn simplify_text(text: &str) -> String {
    let drive_or_unc = |rest: &str| {
        let b = rest.as_bytes();
        rest.starts_with(r"UNC\") || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\')
    };
    match text.strip_prefix(r"\\?\") {
        Some(rest) if drive_or_unc(rest) => paddock_repo_graph::strip_verbatim(Path::new(text)).to_string_lossy().into_owned(),
        _ => text.to_string(),
    }
}

/// Starts a ledger file (only `setup` calls this): refuses when one exists, because a second `setup` would orphan the
/// folders of the first.
pub fn create(path: &Path, job_root: &Path, started_ns: i128) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| {
        format!(
            "the ledger {} cannot be created: {e}. `setup` runs once per job; a second run would orphan the first one's folders.",
            path.display()
        )
    })?;
    writeln!(file, "#job\t{}\t{started_ns}", job_root.display()).map_err(|e| format!("the ledger cannot be written: {e}"))
}

/// The one writer: appends the line for `kind` and returns the entry written. Refuses a second line for a kind, a path that
/// is not an existing folder (unless the mode is `off`), and a ledger that does not exist (run `setup` first).
pub fn append(ledger_path: &Path, kind: Kind, mode: Mode, path: Option<&Path>) -> Result<Entry, String> {
    let text = std::fs::read_to_string(ledger_path).map_err(|e| {
        format!(
            "the ledger {} cannot be read: {e}. Run `./dev ci release-isolation setup` first.",
            ledger_path.display()
        )
    })?;
    let ledger = Ledger::parse(&text).map_err(|e| format!("the ledger {} is damaged: {e}", ledger_path.display()))?;
    if !kind.allowed_modes().contains(&mode) {
        let names: Vec<&str> = kind.allowed_modes().iter().map(|m| m.name()).collect();
        return Err(format!(
            "`{kind}` may only be recorded as {}, not `{}`.",
            names.join(" or "),
            mode.name()
        ));
    }
    if ledger.get(kind).is_some() {
        return Err(format!("the ledger already has a line for `{kind}`: one line per kind."));
    }
    let (path_text, id) = match mode {
        Mode::Off => ("-".to_string(), None),
        Mode::Job | Mode::Run => {
            let given = path.ok_or_else(|| format!("`{kind}` with mode `{}` needs a path.", mode.name()))?;
            let canonical = given
                .canonicalize()
                .map_err(|e| format!("`{kind}`: {} is not an existing folder: {e}", given.display()))?;
            if !canonical.is_dir() {
                return Err(format!("`{kind}`: {} is not a folder.", canonical.display()));
            }
            let id =
                identity_of(&canonical).map_err(|e| format!("`{kind}`: the identity of {} cannot be read: {e}", canonical.display()))?;
            (simplified(&canonical).display().to_string(), Some(id))
        }
    };
    if path_text.contains(['\t', '\n']) {
        return Err(format!("`{kind}`: the path contains a tab or a newline."));
    }
    let (dev, ino, ns) = match id {
        Some(i) => (i.dev.to_string(), i.ino.to_string(), i.birth_ns.to_string()),
        None => (String::new(), String::new(), String::new()),
    };
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(ledger_path)
        .map_err(|e| format!("the ledger cannot be opened for writing: {e}"))?;
    let line = format!("{kind}\t{path_text}\t{}\t{dev}\t{ino}\t{ns}\n", mode.name());
    file.write_all(line.as_bytes())
        .map_err(|e| format!("the ledger cannot be written: {e}"))?;
    Ok(Entry {
        kind: kind.name().to_string(),
        path: path_text,
        mode: mode.name().to_string(),
        id,
        line: ledger.entries.len() + 2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_windows_prefixes_are_removed_only_where_that_is_safe() {
        assert_eq!(simplify_text(r"\\?\C:\job\x"), r"C:\job\x");
        assert_eq!(simplify_text(r"\\?\UNC\srv\share\x"), r"\\srv\share\x");
        assert_eq!(simplify_text(r"\\?\Volume{1234}\x"), r"\\?\Volume{1234}\x");
        assert_eq!(simplify_text("/tmp/x"), "/tmp/x");
        // Through the path form too, UNC included, and the same answer as the shared function for the forms both handle.
        assert_eq!(simplified(Path::new(r"\\?\UNC\srv\share\x")), Path::new(r"\\srv\share\x"));
        assert_eq!(simplified(Path::new(r"\\?\C:\job")), Path::new(r"C:\job"));
        assert_eq!(simplified(Path::new(r"\\srv\share")), Path::new(r"\\srv\share"), "already plain");
        assert_eq!(
            simplify_text(r"\\?\UNC\srv\share"),
            paddock_repo_graph::strip_verbatim(Path::new(r"\\?\UNC\srv\share")).to_string_lossy()
        );
    }

    #[test]
    fn append_refuses_a_mode_the_kind_may_not_have() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("ledger.tsv");
        create(&ledger, dir.path(), 1).unwrap();
        assert!(
            append(&ledger, Kind::Npm, Mode::Off, None)
                .unwrap_err()
                .contains("only be recorded as job")
        );
        assert!(append(&ledger, Kind::Npm, Mode::Run, Some(dir.path())).is_err());
        assert!(append(&ledger, Kind::Sccache, Mode::Job, Some(dir.path())).is_err());
        assert!(append(&ledger, Kind::Libduckdb, Mode::Off, None).is_ok());
    }

    #[test]
    fn append_writes_one_line_per_kind_and_refuses_a_second() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("ledger.tsv");
        create(&ledger, dir.path(), 5).unwrap();
        assert!(create(&ledger, dir.path(), 6).is_err(), "setup twice");
        let folder = dir.path().join("f");
        std::fs::create_dir(&folder).unwrap();
        append(&ledger, Kind::Npm, Mode::Job, Some(&folder)).unwrap();
        append(&ledger, Kind::Sccache, Mode::Off, None).unwrap();
        let err = append(&ledger, Kind::Npm, Mode::Job, Some(&folder)).unwrap_err();
        assert!(err.contains("one line per kind"), "{err}");
        let read = Ledger::read(&ledger).unwrap();
        assert_eq!(read.started_ns, 5);
        assert_eq!(read.entries.len(), 2);
        assert_eq!(read.get(Kind::Sccache).unwrap().path, "-");
        assert!(read.get(Kind::Npm).unwrap().id.is_some());
    }

    #[test]
    fn append_refuses_a_missing_folder_and_a_missing_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = dir.path().join("ledger.tsv");
        assert!(
            append(&ledger, Kind::Npm, Mode::Job, Some(dir.path()))
                .unwrap_err()
                .contains("setup")
        );
        create(&ledger, dir.path(), 1).unwrap();
        assert!(append(&ledger, Kind::Npm, Mode::Job, Some(&dir.path().join("gone"))).is_err());
        assert!(append(&ledger, Kind::Npm, Mode::Job, None).is_err());
    }

    #[test]
    fn parse_keeps_wrong_words_for_verify_to_name_and_rejects_a_broken_shape() {
        let l = Ledger::parse("#job\t/t\t9\nbogus\t/x\tshared\t1\t2\t3\n").unwrap();
        assert_eq!(l.entries[0].kind, "bogus");
        assert_eq!(l.entries[0].mode, "shared");
        assert!(Ledger::parse("").is_err());
        assert!(Ledger::parse("kind\tpath\n").is_err());
        assert!(Ledger::parse("#job\t/t\t9\nnpm\t/x\tjob\n").is_err());
        assert!(Ledger::parse("#job\t/t\tsoon\n").is_err());
    }
}
