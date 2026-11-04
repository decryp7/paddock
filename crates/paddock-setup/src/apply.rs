//! `apply-update`: the elevated hand-off (ADR 0053 section 4.2). The sequence and every decision are here, over an
//! [`UpdatePort`]; the Win32 calls are the glue's (`native::apply`).
//!
//! Trust anchor: the minisign key compiled into the program, which sits in `$INSTDIR`. Untrusted: the download, the
//! signature file, the version and every other argument. Steps, in order, holding `Global\<identifier>.setup`:
//!
//! 1. `$INSTDIR\updates\` and `updates\tmp\` are made or checked (explicit descriptor; a wrong owner or a reparse point is
//!    renamed aside), the previous staged copy is removed;
//! 2. the download is opened once for identification only (`SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION`,
//!    `FILE_FLAG_OPEN_REPARSE_POINT`) and must be a regular file on a local fixed disk ([`check_source`]);
//! 3. it is copied by that handle into a new file in `updates\` (bounded by the announced size, at most 1 GiB), the write
//!    handle is closed;
//! 4. the copy is reopened read-only with read-only sharing and **that handle stays open until the installer has
//!    started**; the copy (never the original) is verified with `paddock_updater::signature::verify_file`, including the
//!    signed version; the signed version must be newer than the installed one (no downgrade, no equal);
//! 5. the copy is started with the fixed arguments `/P /UPDATE /R` and `TEMP`/`TMP` set to `updates\tmp\`; then the handle
//!    and the mutex are released.

use crate::checkdir::{components, is_within};
use crate::log::Sink;
use semver::Version;

/// The largest update accepted whatever `latest.json` says (1 GiB).
pub const MAX_UPDATE_BYTES: u64 = 1 << 30;
/// The longest signature text read (the updater's own bound).
pub const MAX_SIGNATURE_BYTES: u64 = 8192;
/// The installer's fixed arguments: passive, update, restart. Nothing from the caller reaches the installer.
pub const INSTALLER_ARGS: [&str; 3] = ["/P", "/UPDATE", "/R"];

/// Why `apply-update` refused. Each has an exit code and a sentence the app shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The build has no update key.
    #[error("no update key in this build")]
    NoKey,
    /// An installer of the app already runs (the mutex exists).
    #[error("another install is running")]
    Busy,
    /// `$INSTDIR` is not an allowed folder or `updates\` could not be made safe.
    #[error("the install folder is not allowed")]
    InstallDir,
    /// The download is not a regular file on this PC's own disk.
    #[error("the update is not a regular local file")]
    SourceNotLocal,
    /// Larger than announced or than 1 GiB.
    #[error("the update is larger than announced")]
    TooLarge,
    /// A read, write or system call failed (the log says which).
    #[error("a file operation failed")]
    Io,
    /// Wrong key, damaged file, legacy signature or the wrong signed version.
    #[error("the signature check failed")]
    SignatureInvalid,
    /// The signed version is not newer than the installed one.
    #[error("not newer than the installed version")]
    NotNewer,
    /// `CreateProcessW` failed.
    #[error("the installer could not be started")]
    StartFailed,
}

impl Refusal {
    /// Every refusal.
    pub const ALL: [Refusal; 9] = [
        Refusal::NoKey,
        Refusal::Busy,
        Refusal::InstallDir,
        Refusal::SourceNotLocal,
        Refusal::TooLarge,
        Refusal::Io,
        Refusal::SignatureInvalid,
        Refusal::NotNewer,
        Refusal::StartFailed,
    ];

    /// The exit code (20 and up).
    pub fn code(self) -> i32 {
        match self {
            Refusal::NoKey => 20,
            Refusal::Busy => 21,
            Refusal::InstallDir => 22,
            Refusal::SourceNotLocal => 23,
            Refusal::TooLarge => 24,
            Refusal::Io => 25,
            Refusal::SignatureInvalid => 26,
            Refusal::NotNewer => 27,
            Refusal::StartFailed => 28,
        }
    }

    /// The refusal for an exit code.
    pub fn from_code(code: i32) -> Option<Refusal> {
        Refusal::ALL.into_iter().find(|r| r.code() == code)
    }

    /// The sentence the app shows after "Not installed:".
    pub fn sentence(self) -> &'static str {
        match self {
            Refusal::NoKey => "Updates are not available in this build.",
            Refusal::Busy => "Another install of this app is already running.",
            Refusal::InstallDir => "This copy is not installed in a protected Program Files folder.",
            Refusal::SourceNotLocal => "The update must be on this PC's own disk.",
            Refusal::TooLarge => "The downloaded update is larger than announced. Download it again.",
            Refusal::Io => "The update could not be copied. The reason is in setup.log in the install folder's updates folder.",
            Refusal::SignatureInvalid => "The downloaded update did not pass the signature check. Download it again.",
            Refusal::NotNewer => "The update is not newer than the installed version.",
            Refusal::StartFailed => "The installer could not be started. The reason is in setup.log in the install folder's updates folder.",
        }
    }
}

/// What the glue found out about the opened download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFacts {
    /// `GetFileType` is `FILE_TYPE_DISK` (not a pipe, not a character device).
    pub is_disk_file: bool,
    /// Directory attribute.
    pub is_directory: bool,
    /// Reparse-point attribute.
    pub is_reparse: bool,
    /// `GetFinalPathNameByHandleW`.
    pub final_path: String,
    /// `GetDriveTypeW` of the volume is `DRIVE_FIXED`.
    pub volume_fixed: bool,
    /// The end-of-file size.
    pub size: u64,
}

/// Whether `final_path` is a drive-letter path (`C:\...` or `\\?\C:\...`), not a share.
pub fn is_drive_letter_path(final_path: &str) -> bool {
    let p = final_path.strip_prefix(r"\\?\").unwrap_or(final_path);
    let mut c = p.chars();
    matches!((c.next(), c.next(), c.next()), (Some(d), Some(':'), Some('\\')) if d.is_ascii_alphabetic())
}

/// The bound for the copy: the announced size, never more than [`MAX_UPDATE_BYTES`], and 1 GiB when none was announced.
pub fn copy_limit(announced: Option<u64>) -> u64 {
    announced.unwrap_or(MAX_UPDATE_BYTES).min(MAX_UPDATE_BYTES)
}

/// The rule for the opened download (section 4.2 step 2, and the size bound).
pub fn check_source(facts: &SourceFacts, limit: u64) -> Result<(), Refusal> {
    if !facts.is_disk_file || facts.is_directory || facts.is_reparse || !is_drive_letter_path(&facts.final_path) || !facts.volume_fixed {
        return Err(Refusal::SourceNotLocal);
    }
    if facts.size > limit {
        return Err(Refusal::TooLarge);
    }
    Ok(())
}

/// The paths `apply-update` writes, all derived from `$INSTDIR` alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// `$INSTDIR`.
    pub instdir: String,
    /// `$INSTDIR\updates`.
    pub updates: String,
    /// `$INSTDIR\updates\tmp`.
    pub tmp: String,
    /// `$INSTDIR\updates\setup.log`.
    pub log: String,
}

impl Layout {
    /// The layout under `instdir` (no trailing backslash kept).
    pub fn new(instdir: &str) -> Layout {
        let instdir = instdir.trim_end_matches('\\').to_string();
        let updates = format!(r"{instdir}\updates");
        Layout { tmp: format!(r"{updates}\tmp"), log: format!(r"{updates}\setup.log"), updates, instdir }
    }

    /// The staged copy for `version`.
    pub fn staged(&self, version: &Version) -> String {
        format!(r"{}\update-{version}.exe", self.updates)
    }

    /// Whether `path` lies inside `updates\`.
    pub fn owns(&self, path: &str) -> bool {
        is_within(path, &self.updates) && components(path).len() > components(&self.updates).len()
    }
}

/// Everything `apply-update` needs from the machine.
pub trait UpdatePort {
    /// A key is compiled in.
    fn has_key(&self) -> bool;
    /// `$INSTDIR` passes the `check-dir` rule and `updates\` and `updates\tmp\` are made or checked (renaming a bad one aside).
    fn prepare_folders(&mut self) -> Result<(), Refusal>;
    /// Takes `Global\<identifier>.setup`; `Busy` when it exists.
    fn acquire_mutex(&mut self) -> Result<(), Refusal>;
    /// Removes the previous staged copy and the contents of `updates\tmp\`.
    fn clean_previous(&mut self);
    /// The version resource of `$INSTDIR\<main>.exe`.
    fn installed_version(&mut self) -> Result<Version, Refusal>;
    /// Reads the signature file (bounded to [`MAX_SIGNATURE_BYTES`]).
    fn read_signature(&mut self, path: &str) -> Result<String, Refusal>;
    /// Opens the download for identification only and reports what it is.
    fn open_source(&mut self, path: &str) -> Result<SourceFacts, Refusal>;
    /// Copies by the open handle into a new file at `dest`, at most `limit` bytes, then closes the write handle and the source.
    fn copy_source(&mut self, dest: &str, limit: u64) -> Result<u64, Refusal>;
    /// Reopens `dest` read-only with read-only sharing and keeps the handle until [`UpdatePort::release`].
    fn hold_copy(&mut self, dest: &str) -> Result<(), Refusal>;
    /// Verifies the held copy against the key (`verify_file`), requiring the signed version to be `announced`.
    fn verify(&mut self, dest: &str, signature: &str, announced: &Version) -> Result<(), Refusal>;
    /// Starts `dest` with [`INSTALLER_ARGS`] and `TEMP`/`TMP` set to `updates\tmp\`.
    fn start(&mut self, dest: &str) -> Result<(), Refusal>;
    /// Closes the held copy and releases the mutex.
    fn release(&mut self);
}

/// What the caller handed over (untrusted until the steps above have passed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The download.
    pub file: String,
    /// The signature file.
    pub signature: String,
    /// The announced version.
    pub version: Version,
    /// The announced size.
    pub size: Option<u64>,
}

/// Steps 1 to 5 after the mutex is held.
fn locked(port: &mut dyn UpdatePort, layout: &Layout, req: &Request, log: &mut dyn Sink) -> Result<(), Refusal> {
    port.clean_previous();
    let installed = port.installed_version()?;
    let signature = port.read_signature(&req.signature)?;
    let facts = port.open_source(&req.file)?;
    let limit = copy_limit(req.size);
    check_source(&facts, limit)?;
    let dest = layout.staged(&req.version);
    let copied = port.copy_source(&dest, limit)?;
    log.line("apply-update", &format!("copied {copied} bytes into the protected folder"));
    port.hold_copy(&dest)?;
    port.verify(&dest, &signature, &req.version)?;
    log.line("apply-update", "signature and signed version verified on the held copy");
    if req.version <= installed {
        return Err(Refusal::NotNewer);
    }
    port.start(&dest)?;
    Ok(())
}

/// Runs the sequence. Whatever happens after the mutex was taken, [`UpdatePort::release`] runs last.
pub fn run(port: &mut dyn UpdatePort, layout: &Layout, req: &Request, log: &mut dyn Sink) -> Result<(), Refusal> {
    let refuse = |log: &mut dyn Sink, r: Refusal| {
        log.line("apply-update", &format!("refused: {}", r.sentence()));
        Err(r)
    };
    if !port.has_key() {
        return refuse(log, Refusal::NoKey);
    }
    log.line("apply-update", &format!("start, version {}", req.version));
    if let Err(r) = port.prepare_folders() {
        return refuse(log, r);
    }
    if let Err(r) = port.acquire_mutex() {
        return refuse(log, r);
    }
    let result = locked(port, layout, req, log);
    port.release();
    match result {
        Ok(()) => {
            log.line("apply-update", "handed off: the installer was started");
            Ok(())
        }
        Err(r) => refuse(log, r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INST: &str = r"C:\Program Files\Dyno";

    fn good_source() -> SourceFacts {
        SourceFacts { is_disk_file: true, is_directory: false, is_reparse: false, final_path: r"C:\Users\x\AppData\Local\Temp\Dyno_1.2.0_x64-setup.exe".into(), volume_fixed: true, size: 1000 }
    }

    #[test]
    fn refusal_codes_round_trip_and_have_sentences() {
        for r in Refusal::ALL {
            assert_eq!(Refusal::from_code(r.code()), Some(r));
            assert!(r.code() >= 20);
            assert!(r.sentence().ends_with('.'));
        }
        assert_eq!(Refusal::from_code(1), None);
        assert_eq!(Refusal::SignatureInvalid.sentence(), "The downloaded update did not pass the signature check. Download it again.");
        assert_eq!(Refusal::SourceNotLocal.sentence(), "The update must be on this PC's own disk.");
        assert_eq!(Refusal::NoKey.sentence(), "Updates are not available in this build.");
    }

    #[test]
    fn drive_letter_paths() {
        assert!(is_drive_letter_path(r"C:\a\b.exe"));
        assert!(is_drive_letter_path(r"\\?\C:\a\b.exe"));
        assert!(is_drive_letter_path(r"d:\x"));
        assert!(!is_drive_letter_path(r"\\server\share\b.exe"));
        assert!(!is_drive_letter_path(r"\\?\UNC\server\share\b.exe"));
        assert!(!is_drive_letter_path(r"\\.\pipe\x"));
        assert!(!is_drive_letter_path(""));
        assert!(!is_drive_letter_path("C:"));
    }

    #[test]
    fn copy_limit_is_the_announced_size_capped() {
        assert_eq!(copy_limit(Some(1234)), 1234);
        assert_eq!(copy_limit(None), MAX_UPDATE_BYTES);
        assert_eq!(copy_limit(Some(u64::MAX)), MAX_UPDATE_BYTES);
        assert_eq!(copy_limit(Some(0)), 0);
    }

    #[test]
    fn a_regular_local_file_passes() {
        assert_eq!(check_source(&good_source(), 1000), Ok(()));
    }

    #[test]
    fn every_other_kind_of_source_is_refused() {
        type Edit = Box<dyn Fn(&mut SourceFacts)>;
        let cases: Vec<(&str, Edit)> = vec![
            ("pipe or device", Box::new(|f| f.is_disk_file = false)),
            ("directory", Box::new(|f| f.is_directory = true)),
            ("reparse point", Box::new(|f| f.is_reparse = true)),
            ("unc", Box::new(|f| f.final_path = r"\\?\UNC\srv\share\u.exe".into())),
            ("share", Box::new(|f| f.final_path = r"\\srv\share\u.exe".into())),
            ("network volume", Box::new(|f| f.volume_fixed = false)),
        ];
        for (name, edit) in cases {
            let mut f = good_source();
            edit(&mut f);
            assert_eq!(check_source(&f, 1000), Err(Refusal::SourceNotLocal), "{name}");
        }
    }

    #[test]
    fn a_source_larger_than_the_limit_is_too_large() {
        let mut f = good_source();
        f.size = 1001;
        assert_eq!(check_source(&f, 1000), Err(Refusal::TooLarge));
    }

    #[test]
    fn layout_is_inside_the_install_folder() {
        let l = Layout::new(r"C:\Program Files\Dyno\");
        assert_eq!(l.instdir, INST);
        assert_eq!(l.updates, r"C:\Program Files\Dyno\updates");
        assert_eq!(l.tmp, r"C:\Program Files\Dyno\updates\tmp");
        assert_eq!(l.log, r"C:\Program Files\Dyno\updates\setup.log");
        assert_eq!(l.staged(&Version::new(1, 2, 0)), r"C:\Program Files\Dyno\updates\update-1.2.0.exe");
        assert!(l.owns(&l.log) && l.owns(&l.tmp) && l.owns(&l.staged(&Version::new(1, 2, 0))));
        assert!(!l.owns(&l.updates));
        assert!(!l.owns(r"C:\ProgramData\Paddock\update.exe"));
        assert!(!l.owns(r"C:\Program Files\Dyno\updates2\x"));
    }

    #[test]
    fn a_hostile_version_cannot_leave_the_updates_folder() {
        let l = Layout::new(INST);
        let v = Version::parse("1.0.0-..\\..\\evil").map(|v| l.staged(&v));
        if let Ok(path) = v {
            assert!(l.owns(&path), "{path}");
        }
        let weird = Version::parse("1.0.0+build.a-b").unwrap();
        assert!(l.owns(&l.staged(&weird)));
    }

    // ---- the sequence over a fake port ----

    #[derive(Default)]
    struct Fake {
        events: Vec<String>,
        writes: Vec<String>,
        key: bool,
        folders: Option<Refusal>,
        mutex: Option<Refusal>,
        installed: Option<Version>,
        source: Option<SourceFacts>,
        copy: Option<Refusal>,
        hold: Option<Refusal>,
        verify: Option<Refusal>,
        start: Option<Refusal>,
        held: bool,
        held_at_start: Option<bool>,
        args_at_start: Vec<String>,
    }
    impl Fake {
        fn ok() -> Fake {
            Fake { key: true, installed: Some(Version::new(1, 1, 0)), source: Some(good_source()), ..Fake::default() }
        }
        fn ev(&mut self, s: &str) {
            self.events.push(s.to_string());
        }
    }
    impl UpdatePort for Fake {
        fn has_key(&self) -> bool {
            self.key
        }
        fn prepare_folders(&mut self) -> Result<(), Refusal> {
            self.ev("prepare");
            self.folders.map_or(Ok(()), Err)
        }
        fn acquire_mutex(&mut self) -> Result<(), Refusal> {
            self.ev("mutex");
            self.mutex.map_or(Ok(()), Err)
        }
        fn clean_previous(&mut self) {
            self.ev("clean");
        }
        fn installed_version(&mut self) -> Result<Version, Refusal> {
            self.ev("installed");
            self.installed.clone().ok_or(Refusal::Io)
        }
        fn read_signature(&mut self, _path: &str) -> Result<String, Refusal> {
            self.ev("signature");
            Ok("sig".into())
        }
        fn open_source(&mut self, _path: &str) -> Result<SourceFacts, Refusal> {
            self.ev("open");
            self.source.clone().ok_or(Refusal::Io)
        }
        fn copy_source(&mut self, dest: &str, limit: u64) -> Result<u64, Refusal> {
            self.ev(&format!("copy limit={limit}"));
            self.writes.push(dest.into());
            self.copy.map_or(Ok(1000), Err)
        }
        fn hold_copy(&mut self, dest: &str) -> Result<(), Refusal> {
            self.ev("hold");
            self.writes.push(dest.into());
            if let Some(r) = self.hold {
                return Err(r);
            }
            self.held = true;
            Ok(())
        }
        fn verify(&mut self, _dest: &str, _sig: &str, announced: &Version) -> Result<(), Refusal> {
            self.ev(&format!("verify {announced}"));
            assert!(self.held, "verified without the held handle");
            self.verify.map_or(Ok(()), Err)
        }
        fn start(&mut self, dest: &str) -> Result<(), Refusal> {
            self.ev("start");
            self.held_at_start = Some(self.held);
            self.writes.push(dest.into());
            self.args_at_start = INSTALLER_ARGS.iter().map(|s| s.to_string()).collect();
            self.start.map_or(Ok(()), Err)
        }
        fn release(&mut self) {
            self.ev("release");
            self.held = false;
        }
    }
    #[derive(Default)]
    struct Lines(Vec<String>);
    impl Sink for Lines {
        fn line(&mut self, phase: &str, message: &str) {
            self.0.push(format!("{phase}: {message}"));
        }
    }
    fn req() -> Request {
        Request { file: r"C:\Users\x\AppData\Local\Temp\Dyno_1.2.0_x64-setup.exe".into(), signature: r"C:\Users\x\AppData\Local\Temp\sig.txt".into(), version: Version::new(1, 2, 0), size: Some(1000) }
    }
    fn go(port: &mut Fake) -> (Result<(), Refusal>, Vec<String>) {
        let mut log = Lines::default();
        let r = run(port, &Layout::new(INST), &req(), &mut log);
        (r, log.0)
    }

    #[test]
    fn the_happy_path_order() {
        let mut p = Fake::ok();
        let (r, _) = go(&mut p);
        assert_eq!(r, Ok(()));
        assert_eq!(p.events, ["prepare", "mutex", "clean", "installed", "signature", "open", "copy limit=1000", "hold", "verify 1.2.0", "start", "release"]);
    }

    #[test]
    fn the_copy_is_held_read_only_until_the_start_returns() {
        let mut p = Fake::ok();
        go(&mut p).0.unwrap();
        assert_eq!(p.held_at_start, Some(true));
        assert!(!p.held, "released at the end");
        let start = p.events.iter().position(|e| e == "start").unwrap();
        let release = p.events.iter().position(|e| e == "release").unwrap();
        let hold = p.events.iter().position(|e| e == "hold").unwrap();
        assert!(hold < start && start < release);
    }

    #[test]
    fn the_installer_gets_only_the_fixed_arguments() {
        let mut p = Fake::ok();
        go(&mut p).0.unwrap();
        assert_eq!(p.args_at_start, ["/P", "/UPDATE", "/R"]);
    }

    #[test]
    fn no_key_refuses_before_touching_anything() {
        let mut p = Fake::ok();
        p.key = false;
        assert_eq!(go(&mut p).0, Err(Refusal::NoKey));
        assert!(p.events.is_empty());
    }

    #[test]
    fn a_bad_install_folder_refuses_before_the_mutex() {
        let mut p = Fake::ok();
        p.folders = Some(Refusal::InstallDir);
        assert_eq!(go(&mut p).0, Err(Refusal::InstallDir));
        assert_eq!(p.events, ["prepare"]);
    }

    #[test]
    fn a_running_installer_is_busy_and_nothing_is_released_that_was_not_taken() {
        let mut p = Fake::ok();
        p.mutex = Some(Refusal::Busy);
        assert_eq!(go(&mut p).0, Err(Refusal::Busy));
        assert!(!p.events.contains(&"release".to_string()));
        assert!(!p.events.contains(&"open".to_string()));
    }

    #[test]
    fn a_source_that_is_not_local_never_gets_copied() {
        let mut p = Fake::ok();
        p.source.as_mut().unwrap().final_path = r"\\?\UNC\srv\share\u.exe".into();
        assert_eq!(go(&mut p).0, Err(Refusal::SourceNotLocal));
        assert!(!p.events.iter().any(|e| e.starts_with("copy")));
        assert_eq!(p.events.last().unwrap(), "release");
    }

    #[test]
    fn an_oversize_source_is_refused_before_the_copy() {
        let mut p = Fake::ok();
        p.source.as_mut().unwrap().size = 5000;
        assert_eq!(go(&mut p).0, Err(Refusal::TooLarge));
        assert!(!p.events.iter().any(|e| e.starts_with("copy")));
    }

    #[test]
    fn a_bad_signature_never_starts_the_installer() {
        let mut p = Fake::ok();
        p.verify = Some(Refusal::SignatureInvalid);
        assert_eq!(go(&mut p).0, Err(Refusal::SignatureInvalid));
        assert!(!p.events.contains(&"start".to_string()));
        assert_eq!(p.events.last().unwrap(), "release");
    }

    #[test]
    fn an_older_or_equal_version_is_refused_after_the_signature_passed() {
        for installed in [Version::new(1, 2, 0), Version::new(2, 0, 0), Version::parse("1.2.1").unwrap()] {
            let mut p = Fake::ok();
            p.installed = Some(installed);
            assert_eq!(go(&mut p).0, Err(Refusal::NotNewer));
            assert!(!p.events.contains(&"start".to_string()));
        }
    }

    #[test]
    fn a_prerelease_of_the_installed_version_is_older() {
        let mut p = Fake::ok();
        p.installed = Some(Version::new(1, 2, 0));
        let mut log = Lines::default();
        let r = Request { version: Version::parse("1.2.0-beta.1").unwrap(), ..req() };
        assert_eq!(run(&mut p, &Layout::new(INST), &r, &mut log), Err(Refusal::NotNewer));
    }

    #[test]
    fn a_failed_start_is_reported_and_released() {
        let mut p = Fake::ok();
        p.start = Some(Refusal::StartFailed);
        assert_eq!(go(&mut p).0, Err(Refusal::StartFailed));
        assert_eq!(p.events.last().unwrap(), "release");
    }

    #[test]
    fn a_failed_copy_and_a_failed_hold_release() {
        let mut p = Fake::ok();
        p.copy = Some(Refusal::Io);
        assert_eq!(go(&mut p).0, Err(Refusal::Io));
        assert_eq!(p.events.last().unwrap(), "release");
        let mut p = Fake::ok();
        p.hold = Some(Refusal::Io);
        assert_eq!(go(&mut p).0, Err(Refusal::Io));
        assert_eq!(p.events.last().unwrap(), "release");
    }

    #[test]
    fn only_the_updates_folder_is_written_even_with_a_hostile_programdata() {
        // %ProgramData%\Paddock exists, made by a standard user and holding a junction. The run never learns about it: every
        // write path comes from the layout of $INSTDIR.
        let mut p = Fake::ok();
        go(&mut p).0.unwrap();
        let l = Layout::new(INST);
        assert!(!p.writes.is_empty());
        for w in &p.writes {
            assert!(l.owns(w), "{w}");
            assert!(!w.to_ascii_lowercase().contains("programdata"));
        }
    }

    #[test]
    fn the_log_names_phases_and_outcomes_and_no_user_paths() {
        let mut p = Fake::ok();
        let (r, log) = go(&mut p);
        r.unwrap();
        assert!(log.iter().any(|l| l.contains("phase") || l.starts_with("apply-update")));
        for l in &log {
            assert!(!l.contains("Users") && !l.contains("AppData"), "{l}");
        }
        let mut p = Fake::ok();
        p.verify = Some(Refusal::SignatureInvalid);
        let (_, log) = go(&mut p);
        assert!(log.iter().any(|l| l.contains("refused") && l.contains("signature")), "{log:?}");
    }
}
