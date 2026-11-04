//! `apply-update`: the elevated hand-off (ADR 0053 section 4.2). The sequence and every decision are here, over an
//! [`UpdatePort`]; the Win32 calls are the glue's (`native::apply`).
//!
//! Trust anchor: the minisign key compiled into the program, which sits in `$INSTDIR`. The key is the family's, so the
//! signed file name is bound to the product ([`bind_product`]): another family app's validly signed installer is refused.
//! Untrusted: the download, the signature text, the version and every other argument; paths are parsed into
//! [`LocalPath`] before anything is opened. Steps, in order, holding `Global\<identifier>.setup`:
//!
//! 1. `$INSTDIR\updates\` and a run folder `updates\run-<pid>-<time>\` (Administrators and SYSTEM only) with its `tmp\`
//!    are made or checked; earlier run folders are removed unless something in them is still in use;
//! 2. the download is opened once for identification only (`SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION`,
//!    `FILE_FLAG_OPEN_REPARSE_POINT`) and must be a regular file on a local fixed disk with one hard link
//!    ([`check_source`]);
//! 3. it is copied by that handle into a new Administrators-and-SYSTEM-only file in the run folder (bounded by the announced
//!    size, at most 1 GiB), the write handle is closed;
//! 4. the copy is reopened read-only with read-only sharing and **that handle stays open until the installer has
//!    started**; the copy (never the original) is verified with `paddock_updater::signature::verify_file`, including the
//!    signed version, the signed file name must belong to this product, and the signed version must not be a downgrade;
//! 5. the copy is started with the fixed arguments `/P /UPDATE /R` and `TEMP`/`TMP` set to the run folder's `tmp\`;
//!    `Global\<identifier>.setup` is handed over to the installer's own `hold-installing` (no clock: the wait ends when
//!    the installer holds the mutex or its process has ended); then the handle is released.
//!
//! After any refusal that follows the copy, the copy and its run folder are deleted.

use crate::checkdir::{components, is_within};
use crate::localpath::LocalPath;
use crate::log::Sink;
use semver::Version;

/// The largest update accepted whatever `latest.json` says (1 GiB).
pub const MAX_UPDATE_BYTES: u64 = 1 << 30;
/// The longest signature text accepted (a minisign signature file in base64 is about 400 bytes).
pub const MAX_SIGNATURE_TEXT: usize = 1024;
/// The installer's fixed arguments: passive, update, restart. Nothing from the caller reaches the installer.
pub const INSTALLER_ARGS: [&str; 3] = ["/P", "/UPDATE", "/R"];
/// Tauri's NSIS installer file name ends like this (`<productName>_<version>_<arch>-setup.exe`).
pub const INSTALLER_SUFFIX: &str = "-setup.exe";

/// Why `apply-update` refused. Each has an exit code and a sentence the app shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The build has no update key.
    #[error("no update key in this build")]
    NoKey,
    /// An installer of the app already runs (the mutex is held by an administrator's process).
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
    /// The signed version is older than the installed one.
    #[error("not newer than the installed version")]
    NotNewer,
    /// `CreateProcessW` failed.
    #[error("the installer could not be started")]
    StartFailed,
    /// Validly signed, but for another product of the family.
    #[error("the update is signed for another product")]
    WrongProduct,
    /// The mutex name is taken by an object that no administrator made.
    #[error("the installer mutex name is taken by a program that is not an administrator's")]
    Squatted,
}

impl Refusal {
    /// Every refusal.
    pub const ALL: [Refusal; 11] = [
        Refusal::NoKey,
        Refusal::Busy,
        Refusal::InstallDir,
        Refusal::SourceNotLocal,
        Refusal::TooLarge,
        Refusal::Io,
        Refusal::SignatureInvalid,
        Refusal::NotNewer,
        Refusal::StartFailed,
        Refusal::WrongProduct,
        Refusal::Squatted,
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
            Refusal::WrongProduct => 29,
            Refusal::Squatted => 30,
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
            Refusal::InstallDir => {
                "This copy is not installed in a protected Program Files folder."
            }
            Refusal::SourceNotLocal => "The update must be on this PC's own disk.",
            Refusal::TooLarge => {
                "The downloaded update is larger than announced. Download it again."
            }
            Refusal::Io => {
                "The update could not be copied. The reason is in setup.log in the install folder's updates folder."
            }
            Refusal::SignatureInvalid => {
                "The downloaded update did not pass the signature check. Download it again."
            }
            Refusal::NotNewer => "The update is not newer than the installed version.",
            Refusal::StartFailed => {
                "The installer could not be started. The reason is in setup.log in the install folder's updates folder."
            }
            Refusal::WrongProduct => "The downloaded update is for another app. Download it again.",
            Refusal::Squatted => {
                "Another program is using this app's installer lock. An administrator can restart the PC and try again."
            }
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
    /// `nNumberOfLinks`: a file with another hard link can be changed through the other name.
    pub links: u32,
}

/// `C:\` of a drive-letter path `C:\x` or `\\?\C:\x`, `None` for anything else (a share, `\\?\UNC\`, a pipe).
pub fn volume_root(final_path: &str) -> Option<String> {
    let p = final_path.strip_prefix(r"\\?\").unwrap_or(final_path);
    let mut c = p.chars();
    match (c.next(), c.next(), c.next()) {
        (Some(d), Some(':'), Some('\\')) if d.is_ascii_alphabetic() => Some(format!("{d}:\\")),
        _ => None,
    }
}

/// The bound for the copy: the announced size, never more than [`MAX_UPDATE_BYTES`], and 1 GiB when none was announced.
pub fn copy_limit(announced: Option<u64>) -> u64 {
    announced.unwrap_or(MAX_UPDATE_BYTES).min(MAX_UPDATE_BYTES)
}

/// The rule for the opened download (section 4.2 step 2, and the size bound).
pub fn check_source(facts: &SourceFacts, limit: u64) -> Result<(), Refusal> {
    if !facts.is_disk_file
        || facts.is_directory
        || facts.is_reparse
        || volume_root(&facts.final_path).is_none()
        || !facts.volume_fixed
        || facts.links != 1
    {
        return Err(Refusal::SourceNotLocal);
    }
    if facts.size > limit {
        return Err(Refusal::TooLarge);
    }
    Ok(())
}

/// The signed trusted comment must carry `file:<product>_...-setup.exe`: the update key is the family's, so without this a
/// validly signed installer of another app would be accepted for this one. Tauri signs
/// `timestamp:<unix>\tfile:<name of the signed file>`; the NSIS installer is named `<productName>_<version>_<arch>-setup.exe`.
pub fn bind_product(trusted_comment: &str, product: &str) -> Result<(), Refusal> {
    let file = trusted_comment
        .split('\t')
        .find_map(|f| f.strip_prefix("file:"))
        .ok_or(Refusal::WrongProduct)?;
    let prefix = format!("{product}_");
    if !product.is_empty()
        && file.len() > prefix.len() + INSTALLER_SUFFIX.len()
        && file.starts_with(&prefix)
        && file.ends_with(INSTALLER_SUFFIX)
        && !file.contains(['\\', '/'])
    {
        Ok(())
    } else {
        Err(Refusal::WrongProduct)
    }
}

/// Whether handing `announced` to a copy whose main binary's version resource says `installed` is a downgrade. The resource
/// holds only `major.minor.patch`, so a prerelease of the installed version is unknown: an announced release with the same
/// numbers is allowed (it is the release of an installed prerelease, or the same release again, which is not a downgrade),
/// an announced prerelease of the same numbers is not, and build metadata never counts.
pub fn is_downgrade(installed: &Version, announced: &Version) -> bool {
    let key = |v: &Version| (v.major, v.minor, v.patch);
    match key(announced).cmp(&key(installed)) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Equal => !announced.pre.is_empty(),
        std::cmp::Ordering::Greater => false,
    }
}

/// A name for one run's folder, unique without a clock comparison: process id and the time the run started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunId(String);

impl RunId {
    /// `run-<pid>-<unix ms in hex>`.
    pub fn new(pid: u32, unix_ms: u64) -> RunId {
        RunId(format!("run-{pid}-{unix_ms:x}"))
    }
    /// The folder name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Whether `name` is the name of some run's folder.
    pub fn is_run_folder(name: &str) -> bool {
        name.strip_prefix("run-").is_some_and(|rest| {
            let mut parts = rest.splitn(2, '-');
            matches!((parts.next(), parts.next()), (Some(p), Some(t)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && !t.is_empty() && t.bytes().all(|b| b.is_ascii_hexdigit()))
        })
    }
}

/// The paths `apply-update` writes, all derived from `$INSTDIR` and the run id alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// `$INSTDIR`.
    pub instdir: String,
    /// `$INSTDIR\updates`.
    pub updates: String,
    /// `$INSTDIR\updates\setup.log`.
    pub log: String,
}

/// One run's folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPaths {
    /// `$INSTDIR\updates\run-<pid>-<time>`.
    pub dir: String,
    /// `...\tmp`, the installer's `TEMP` and `TMP`.
    pub tmp: String,
}

impl RunPaths {
    /// The staged copy for `version`.
    pub fn staged(&self, version: &Version) -> String {
        format!(r"{}\update-{version}.exe", self.dir)
    }
}

impl Layout {
    /// The layout under `instdir` (no trailing backslash kept).
    pub fn new(instdir: &str) -> Layout {
        let instdir = instdir.trim_end_matches('\\').to_string();
        let updates = format!(r"{instdir}\updates");
        Layout {
            log: format!(r"{updates}\setup.log"),
            updates,
            instdir,
        }
    }

    /// The folders of one run.
    pub fn run(&self, id: &RunId) -> RunPaths {
        let dir = format!(r"{}\{}", self.updates, id.as_str());
        RunPaths {
            tmp: format!(r"{dir}\tmp"),
            dir,
        }
    }

    /// Whether `path` lies inside `updates\`.
    pub fn owns(&self, path: &str) -> bool {
        is_within(path, &self.updates) && components(path).len() > components(&self.updates).len()
    }
}

/// How the installer took over the mutex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handover {
    /// The installer's own `hold-installing` holds `Global\<identifier>.setup` now.
    InstallerHoldsIt,
    /// The installer process ended before it held the mutex (the reason is in its own log).
    InstallerEnded,
}

/// Everything `apply-update` needs from the machine.
pub trait UpdatePort {
    /// A key is compiled in.
    fn has_key(&self) -> bool;
    /// `$INSTDIR` passes the `check-dir` rule, `updates\` is made or checked and this run's folder and `tmp\` are made with
    /// an Administrators-and-SYSTEM-only descriptor.
    fn prepare_folders(&mut self, run: &RunPaths) -> Result<(), Refusal>;
    /// Takes `Global\<identifier>.setup`; `Busy` when an administrator's process holds it, `Squatted` when something else made it.
    fn acquire_mutex(&mut self) -> Result<(), Refusal>;
    /// Removes earlier run folders; one with something still in use is kept (that is not a failure).
    fn clean_previous(&mut self, keep: &RunPaths);
    /// The version resource of `$INSTDIR\<main>.exe`.
    fn installed_version(&mut self) -> Result<Version, Refusal>;
    /// Opens the download for identification only and reports what it is.
    fn open_source(&mut self, path: &LocalPath) -> Result<SourceFacts, Refusal>;
    /// Copies by the open handle into a new Administrators-and-SYSTEM-only file at `dest`, at most `limit` bytes, then closes
    /// the write handle and the source.
    fn copy_source(&mut self, dest: &str, limit: u64) -> Result<u64, Refusal>;
    /// Reopens `dest` read-only with read-only sharing and keeps the handle until [`UpdatePort::release`].
    fn hold_copy(&mut self, dest: &str) -> Result<(), Refusal>;
    /// Verifies the held copy against the key (`verify_file`), requiring the signed version to be `announced`; returns the
    /// signed trusted comment.
    fn verify(
        &mut self,
        dest: &str,
        signature: &str,
        announced: &Version,
    ) -> Result<String, Refusal>;
    /// Starts `dest` with [`INSTALLER_ARGS`] and `TEMP`/`TMP` set to `run.tmp`.
    fn start(&mut self, dest: &str, run: &RunPaths) -> Result<(), Refusal>;
    /// Releases the mutex to the started installer and waits until it holds it or its process has ended.
    fn handover(&mut self) -> Handover;
    /// Closes the held copy and deletes the copy and this run's folder (after a refusal).
    fn discard(&mut self, run: &RunPaths);
    /// Closes the held copy and releases the mutex handle.
    fn release(&mut self);
}

/// What the caller handed over (untrusted until the steps above have passed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The download.
    pub file: LocalPath,
    /// The signature text (base64 of the minisign signature file).
    pub signature: String,
    /// The announced version.
    pub version: Version,
    /// The announced size.
    pub size: Option<u64>,
    /// This build's product name (baked in).
    pub product: String,
    /// This run's folder name.
    pub run: RunId,
}

/// Runs the sequence. Whatever happens after the mutex was taken, [`UpdatePort::release`] runs last; after a refusal that
/// follows the preparation, [`UpdatePort::discard`] runs first.
pub fn run(
    port: &mut dyn UpdatePort,
    layout: &Layout,
    req: &Request,
    log: &mut dyn Sink,
) -> Result<(), Refusal> {
    let refuse = |log: &mut dyn Sink, r: Refusal| {
        log.line("apply-update", &format!("refused: {}", r.sentence()));
        Err(r)
    };
    if !port.has_key() {
        return refuse(log, Refusal::NoKey);
    }
    log.line("apply-update", &format!("start, version {}", req.version));
    let paths = layout.run(&req.run);
    if let Err(r) = port.prepare_folders(&paths) {
        return refuse(log, r);
    }
    if let Err(r) = port.acquire_mutex() {
        port.discard(&paths);
        return refuse(log, r);
    }
    let result = locked(port, &paths, req, log);
    match result {
        Ok(()) => {
            let how = port.handover();
            log.line(
                "apply-update",
                match how {
                    Handover::InstallerHoldsIt => "handed off: the installer holds the lock",
                    Handover::InstallerEnded => {
                        "handed off: the installer ended before it held the lock"
                    }
                },
            );
            port.release();
            Ok(())
        }
        Err(r) => {
            port.discard(&paths);
            port.release();
            refuse(log, r)
        }
    }
}

/// Steps 1 to 5 after the mutex is held.
fn locked(
    port: &mut dyn UpdatePort,
    paths: &RunPaths,
    req: &Request,
    log: &mut dyn Sink,
) -> Result<(), Refusal> {
    port.clean_previous(paths);
    let installed = port.installed_version()?;
    let facts = port.open_source(&req.file)?;
    let limit = copy_limit(req.size);
    check_source(&facts, limit)?;
    let dest = paths.staged(&req.version);
    let copied = port.copy_source(&dest, limit)?;
    log.line(
        "apply-update",
        &format!("copied {copied} bytes into the protected folder"),
    );
    port.hold_copy(&dest)?;
    let comment = port.verify(&dest, &req.signature, &req.version)?;
    log.line(
        "apply-update",
        "signature and signed version verified on the held copy",
    );
    bind_product(&comment, &req.product)?;
    if is_downgrade(&installed, &req.version) {
        return Err(Refusal::NotNewer);
    }
    port.start(&dest, paths)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const INST: &str = r"C:\Program Files\Dyno";

    fn good_source() -> SourceFacts {
        SourceFacts {
            is_disk_file: true,
            is_directory: false,
            is_reparse: false,
            final_path: r"\\?\C:\Users\x\AppData\Local\Temp\Dyno_1.2.0_x64-setup.exe".into(),
            volume_fixed: true,
            size: 1000,
            links: 1,
        }
    }
    const COMMENT: &str = "timestamp:1\tfile:Dyno_1.2.0_x64-setup.exe";

    #[test]
    fn refusal_codes_round_trip_and_have_sentences() {
        let mut seen = BTreeSet::new();
        for r in Refusal::ALL {
            assert_eq!(Refusal::from_code(r.code()), Some(r));
            assert!(r.code() >= 20 && seen.insert(r.code()));
            assert!(r.sentence().ends_with('.'));
        }
        assert_eq!(Refusal::from_code(1), None);
        assert_eq!(
            Refusal::SignatureInvalid.sentence(),
            "The downloaded update did not pass the signature check. Download it again."
        );
        assert_eq!(
            Refusal::SourceNotLocal.sentence(),
            "The update must be on this PC's own disk."
        );
        assert_eq!(
            Refusal::NoKey.sentence(),
            "Updates are not available in this build."
        );
        assert_eq!(Refusal::WrongProduct.code(), 29);
        assert_eq!(Refusal::Squatted.code(), 30);
    }

    #[test]
    fn volume_roots() {
        assert_eq!(volume_root(r"C:\a\b.exe").as_deref(), Some(r"C:\"));
        assert_eq!(volume_root(r"\\?\C:\a\b.exe").as_deref(), Some(r"C:\"));
        assert_eq!(volume_root(r"d:\x").as_deref(), Some(r"d:\"));
        for p in [
            r"\\server\share\b.exe",
            r"\\?\UNC\server\share\b.exe",
            r"\\.\pipe\x",
            "",
            "C:",
        ] {
            assert_eq!(volume_root(p), None, "{p}");
        }
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
            (
                "unc",
                Box::new(|f| f.final_path = r"\\?\UNC\srv\share\u.exe".into()),
            ),
            (
                "share",
                Box::new(|f| f.final_path = r"\\srv\share\u.exe".into()),
            ),
            ("network volume", Box::new(|f| f.volume_fixed = false)),
            ("a second hard link", Box::new(|f| f.links = 2)),
            ("no link count", Box::new(|f| f.links = 0)),
        ];
        for (name, edit) in cases {
            let mut f = good_source();
            edit(&mut f);
            assert_eq!(
                check_source(&f, 1000),
                Err(Refusal::SourceNotLocal),
                "{name}"
            );
        }
    }

    #[test]
    fn a_source_larger_than_the_limit_is_too_large() {
        let mut f = good_source();
        f.size = 1001;
        assert_eq!(check_source(&f, 1000), Err(Refusal::TooLarge));
    }

    #[test]
    fn the_signed_file_name_must_belong_to_the_product() {
        assert_eq!(bind_product(COMMENT, "Dyno"), Ok(()));
        assert_eq!(
            bind_product(
                "timestamp:1\tfile:Dyno_1.2.0_x64-setup.exe\tversion:1.2.0",
                "Dyno"
            ),
            Ok(())
        );
        assert_eq!(
            bind_product("file:Dyno_1.2.0_arm64-setup.exe", "Dyno"),
            Ok(())
        );
        for (comment, why) in [
            (
                "timestamp:1\tfile:Spotter_1.2.0_x64-setup.exe",
                "another app",
            ),
            (
                "timestamp:1\tfile:DynoHelper_1.2.0_x64-setup.exe",
                "a longer product name",
            ),
            ("timestamp:1\tfile:dyno_1.2.0_x64-setup.exe", "case differs"),
            (
                "timestamp:1\tfile:Dyno_1.2.0_x64.msi",
                "not the NSIS installer",
            ),
            ("timestamp:1\tfile:Dyno_-setup.exe", "no version or arch"),
            (
                "timestamp:1\tfile:Dyno_1.2.0_x64-setup.exe.bin",
                "other suffix",
            ),
            ("timestamp:1\tfile:Dyno_..\\evil-setup.exe", "a path"),
            ("timestamp:1", "no file field"),
            ("", "empty"),
            ("timestamp:1\tfile:", "empty name"),
        ] {
            assert_eq!(
                bind_product(comment, "Dyno"),
                Err(Refusal::WrongProduct),
                "{why}"
            );
        }
        assert_eq!(bind_product(COMMENT, ""), Err(Refusal::WrongProduct));
    }

    #[test]
    fn downgrades_ignore_the_unknown_prerelease_and_build_metadata() {
        let v = |s: &str| Version::parse(s).unwrap();
        assert!(is_downgrade(&v("1.2.0"), &v("1.1.9")));
        assert!(is_downgrade(&v("2.0.0"), &v("1.9.9")));
        assert!(
            is_downgrade(&v("1.2.0"), &v("1.2.0-rc.1")),
            "a prerelease of the same numbers is older than the release"
        );
        assert!(!is_downgrade(&v("1.2.0"), &v("1.2.1")));
        assert!(!is_downgrade(&v("1.2.0"), &v("1.3.0-rc.1")));
        assert!(
            !is_downgrade(&v("1.2.0"), &v("1.2.0")),
            "the release over an installed prerelease (the resource has no prerelease)"
        );
        assert!(!is_downgrade(&v("1.2.0"), &v("1.2.0+build.7")));
        assert!(
            !is_downgrade(&v("1.2.0+build.9"), &v("1.2.0+build.1")),
            "build metadata does not order"
        );
    }

    #[test]
    fn run_ids_are_recognised() {
        let id = RunId::new(4242, 0x019a_5bf3_1c00);
        assert_eq!(id.as_str(), "run-4242-19a5bf31c00");
        assert!(RunId::is_run_folder(id.as_str()));
        for n in [
            "run-",
            "run-x-1",
            "run-1-",
            "run-1-zz",
            "tmp",
            "setup.log",
            "run-12",
            "xrun-1-2",
        ] {
            assert!(!RunId::is_run_folder(n), "{n}");
        }
    }

    #[test]
    fn layout_is_inside_the_install_folder() {
        let l = Layout::new(r"C:\Program Files\Dyno\");
        assert_eq!(l.instdir, INST);
        assert_eq!(l.updates, r"C:\Program Files\Dyno\updates");
        assert_eq!(l.log, r"C:\Program Files\Dyno\updates\setup.log");
        let run = l.run(&RunId::new(7, 0xabc));
        assert_eq!(run.dir, r"C:\Program Files\Dyno\updates\run-7-abc");
        assert_eq!(run.tmp, r"C:\Program Files\Dyno\updates\run-7-abc\tmp");
        assert_eq!(
            run.staged(&Version::new(1, 2, 0)),
            r"C:\Program Files\Dyno\updates\run-7-abc\update-1.2.0.exe"
        );
        assert!(l.owns(&l.log) && l.owns(&run.tmp) && l.owns(&run.staged(&Version::new(1, 2, 0))));
        assert!(!l.owns(&l.updates));
        assert!(!l.owns(r"C:\ProgramData\Paddock\update.exe"));
        assert!(!l.owns(r"C:\Program Files\Dyno\updates2\x"));
    }

    #[test]
    fn any_parseable_version_names_a_file_inside_the_run_folder() {
        let l = Layout::new(INST);
        let run = l.run(&RunId::new(1, 1));
        for text in [
            "1.0.0",
            "1.0.0-rc.1",
            "1.0.0-a-b.c",
            "1.0.0+build.a-b",
            "1.0.0-x+y.z",
            "0.0.0-0",
        ] {
            let v = Version::parse(text).unwrap();
            let staged = run.staged(&v);
            assert!(l.owns(&staged), "{staged}");
            assert!(
                is_within(&staged, &run.dir)
                    && components(&staged).len() == components(&run.dir).len() + 1,
                "{staged}"
            );
        }
        // Text that could climb out of the folder is not a version at all.
        for hostile in [
            r"1.0.0-..\..\evil",
            "1.0.0-../x",
            "1.0.0-a/b",
            r"1.0.0+a\b",
            "1.0.0 -x",
        ] {
            assert!(Version::parse(hostile).is_err(), "{hostile}");
        }
    }

    // ---- the sequence over a fake port ----

    #[derive(Default)]
    struct Fake {
        events: Vec<String>,
        /// Files and folders that exist on the fake disk.
        disk: BTreeSet<String>,
        writes: Vec<String>,
        key: bool,
        folders: Option<Refusal>,
        mutex: Option<Refusal>,
        installed: Option<Version>,
        source: Option<SourceFacts>,
        copy: Option<Refusal>,
        hold: Option<Refusal>,
        verify: Option<Refusal>,
        comment: String,
        start: Option<Refusal>,
        handover: Option<Handover>,
        held: bool,
        held_at_start: Option<bool>,
        args_at_start: Vec<String>,
        tmp_at_start: Option<String>,
        opened: Vec<String>,
    }
    impl Fake {
        fn ok() -> Fake {
            Fake {
                key: true,
                installed: Some(Version::new(1, 1, 0)),
                source: Some(good_source()),
                comment: COMMENT.into(),
                ..Fake::default()
            }
        }
        fn ev(&mut self, s: &str) {
            self.events.push(s.to_string());
        }
    }
    impl UpdatePort for Fake {
        fn has_key(&self) -> bool {
            self.key
        }
        fn prepare_folders(&mut self, run: &RunPaths) -> Result<(), Refusal> {
            self.ev("prepare");
            self.writes.push(run.dir.clone());
            self.writes.push(run.tmp.clone());
            self.disk.insert(run.dir.clone());
            self.folders.map_or(Ok(()), Err)
        }
        fn acquire_mutex(&mut self) -> Result<(), Refusal> {
            self.ev("mutex");
            self.mutex.map_or(Ok(()), Err)
        }
        fn clean_previous(&mut self, _keep: &RunPaths) {
            self.ev("clean");
        }
        fn installed_version(&mut self) -> Result<Version, Refusal> {
            self.ev("installed");
            self.installed.clone().ok_or(Refusal::Io)
        }
        fn open_source(&mut self, path: &LocalPath) -> Result<SourceFacts, Refusal> {
            self.ev("open");
            self.opened.push(path.as_str().to_string());
            self.source.clone().ok_or(Refusal::Io)
        }
        fn copy_source(&mut self, dest: &str, limit: u64) -> Result<u64, Refusal> {
            self.ev(&format!("copy limit={limit}"));
            self.writes.push(dest.into());
            self.disk.insert(dest.into());
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
        fn verify(
            &mut self,
            _dest: &str,
            signature: &str,
            announced: &Version,
        ) -> Result<String, Refusal> {
            self.ev(&format!("verify {announced} {signature}"));
            assert!(self.held, "verified without the held handle");
            self.verify.map_or(Ok(self.comment.clone()), Err)
        }
        fn start(&mut self, dest: &str, run: &RunPaths) -> Result<(), Refusal> {
            self.ev("start");
            self.held_at_start = Some(self.held);
            self.writes.push(dest.into());
            self.tmp_at_start = Some(run.tmp.clone());
            self.args_at_start = INSTALLER_ARGS.iter().map(|s| s.to_string()).collect();
            self.start.map_or(Ok(()), Err)
        }
        fn handover(&mut self) -> Handover {
            self.ev("handover");
            assert!(
                self.held,
                "the copy must still be held while the installer takes over"
            );
            self.handover.unwrap_or(Handover::InstallerHoldsIt)
        }
        fn discard(&mut self, run: &RunPaths) {
            self.ev("discard");
            self.held = false;
            self.disk.retain(|p| !is_within(p, &run.dir));
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
        Request {
            file: LocalPath::parse(r"C:\Users\x\AppData\Local\Temp\Dyno_1.2.0_x64-setup.exe")
                .unwrap(),
            signature: "c2ln".into(),
            version: Version::new(1, 2, 0),
            size: Some(1000),
            product: "Dyno".into(),
            run: RunId::new(10, 0x20),
        }
    }
    fn go(port: &mut Fake) -> (Result<(), Refusal>, Vec<String>) {
        let mut log = Lines::default();
        let r = run(port, &Layout::new(INST), &req(), &mut log);
        (r, log.0)
    }
    fn run_dir() -> String {
        Layout::new(INST).run(&req().run).dir
    }

    #[test]
    fn the_happy_path_order() {
        let mut p = Fake::ok();
        let (r, _) = go(&mut p);
        assert_eq!(r, Ok(()));
        assert_eq!(
            p.events,
            [
                "prepare",
                "mutex",
                "clean",
                "installed",
                "open",
                "copy limit=1000",
                "hold",
                "verify 1.2.0 c2ln",
                "start",
                "handover",
                "release"
            ]
        );
        assert!(
            p.disk.contains(
                &Layout::new(INST)
                    .run(&req().run)
                    .staged(&Version::new(1, 2, 0))
            ),
            "the installer's copy stays for the installer"
        );
    }

    #[test]
    fn the_copy_is_held_read_only_until_the_start_returns_and_the_installer_has_the_lock() {
        let mut p = Fake::ok();
        go(&mut p).0.unwrap();
        assert_eq!(p.held_at_start, Some(true));
        assert!(!p.held, "released at the end");
        let at = |name: &str| p.events.iter().position(|e| e == name).unwrap();
        assert!(
            at("hold") < at("start")
                && at("start") < at("handover")
                && at("handover") < at("release")
        );
    }

    #[test]
    fn the_installer_gets_only_the_fixed_arguments_and_the_run_folders_temp() {
        let mut p = Fake::ok();
        go(&mut p).0.unwrap();
        assert_eq!(p.args_at_start, ["/P", "/UPDATE", "/R"]);
        assert_eq!(
            p.tmp_at_start.as_deref(),
            Some(format!(r"{}\tmp", run_dir()).as_str())
        );
    }

    #[test]
    fn an_installer_that_ends_before_it_holds_the_lock_is_logged_not_waited_for() {
        let mut p = Fake::ok();
        p.handover = Some(Handover::InstallerEnded);
        let (r, log) = go(&mut p);
        assert_eq!(r, Ok(()));
        assert!(
            log.iter()
                .any(|l| l.contains("the installer ended before it held the lock")),
            "{log:?}"
        );
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
        assert!(
            p.disk.is_empty(),
            "the run folder made for this run is removed again"
        );
    }

    #[test]
    fn a_squatted_mutex_name_is_refused_and_logged() {
        let mut p = Fake::ok();
        p.mutex = Some(Refusal::Squatted);
        let (r, log) = go(&mut p);
        assert_eq!(r, Err(Refusal::Squatted));
        assert!(
            log.iter()
                .any(|l| l.contains("refused") && l.contains("installer lock")),
            "{log:?}"
        );
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
    fn a_source_with_another_hard_link_never_gets_copied() {
        let mut p = Fake::ok();
        p.source.as_mut().unwrap().links = 2;
        assert_eq!(go(&mut p).0, Err(Refusal::SourceNotLocal));
        assert!(!p.events.iter().any(|e| e.starts_with("copy")));
    }

    #[test]
    fn an_oversize_source_is_refused_before_the_copy() {
        let mut p = Fake::ok();
        p.source.as_mut().unwrap().size = 5000;
        assert_eq!(go(&mut p).0, Err(Refusal::TooLarge));
        assert!(!p.events.iter().any(|e| e.starts_with("copy")));
    }

    #[test]
    fn only_the_parsed_local_path_is_opened() {
        let mut p = Fake::ok();
        go(&mut p).0.unwrap();
        assert_eq!(
            p.opened,
            [r"C:\Users\x\AppData\Local\Temp\Dyno_1.2.0_x64-setup.exe"]
        );
    }

    fn assert_nothing_left(p: &Fake) {
        assert!(p.disk.is_empty(), "left on disk: {:?}", p.disk);
        assert!(!p.held);
        assert_eq!(p.events.last().unwrap(), "release");
        assert!(!p.events.contains(&"start".to_string()));
    }

    #[test]
    fn a_bad_signature_never_starts_the_installer_and_leaves_nothing() {
        let mut p = Fake::ok();
        p.verify = Some(Refusal::SignatureInvalid);
        assert_eq!(go(&mut p).0, Err(Refusal::SignatureInvalid));
        assert_nothing_left(&p);
    }

    #[test]
    fn an_installer_signed_for_another_product_is_refused_and_leaves_nothing() {
        let mut p = Fake::ok();
        p.comment = "timestamp:1\tfile:Spotter_1.2.0_x64-setup.exe".into();
        assert_eq!(go(&mut p).0, Err(Refusal::WrongProduct));
        assert_nothing_left(&p);
    }

    #[test]
    fn a_downgrade_is_refused_after_the_signature_passed_and_leaves_nothing() {
        for installed in [Version::new(1, 2, 1), Version::new(2, 0, 0)] {
            let mut p = Fake::ok();
            p.installed = Some(installed);
            assert_eq!(go(&mut p).0, Err(Refusal::NotNewer));
            assert_nothing_left(&p);
        }
    }

    #[test]
    fn a_release_over_an_installed_release_or_its_prerelease_is_allowed() {
        let mut p = Fake::ok();
        p.installed = Some(Version::new(1, 2, 0));
        assert_eq!(go(&mut p).0, Ok(()));
    }

    #[test]
    fn a_prerelease_of_the_installed_version_is_a_downgrade() {
        let mut p = Fake::ok();
        p.installed = Some(Version::new(1, 2, 0));
        let mut log = Lines::default();
        let r = Request {
            version: Version::parse("1.2.0-beta.1").unwrap(),
            ..req()
        };
        assert_eq!(
            run(&mut p, &Layout::new(INST), &r, &mut log),
            Err(Refusal::NotNewer)
        );
        assert_nothing_left(&p);
    }

    #[test]
    fn a_failed_start_is_reported_and_leaves_nothing() {
        let mut p = Fake::ok();
        p.start = Some(Refusal::StartFailed);
        assert_eq!(go(&mut p).0, Err(Refusal::StartFailed));
        assert_eq!(p.events.last().unwrap(), "release");
        assert!(p.disk.is_empty() && !p.held);
    }

    #[test]
    fn a_failed_copy_and_a_failed_hold_leave_nothing() {
        let mut p = Fake::ok();
        p.copy = Some(Refusal::Io);
        assert_eq!(go(&mut p).0, Err(Refusal::Io));
        assert_nothing_left(&p);
        let mut p = Fake::ok();
        p.hold = Some(Refusal::Io);
        assert_eq!(go(&mut p).0, Err(Refusal::Io));
        assert_nothing_left(&p);
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
        for needle in [
            "apply-update: start, version 1.2.0",
            "apply-update: copied 1000 bytes into the protected folder",
            "apply-update: signature and signed version verified on the held copy",
            "apply-update: handed off: the installer holds the lock",
        ] {
            assert!(log.iter().any(|l| l == needle), "{needle} not in {log:?}");
        }
        for l in &log {
            assert!(!l.contains("Users") && !l.contains("AppData"), "{l}");
        }
        let mut p = Fake::ok();
        p.verify = Some(Refusal::SignatureInvalid);
        let (_, log) = go(&mut p);
        assert!(
            log.iter().any(|l| l
                == &format!(
                    "apply-update: refused: {}",
                    Refusal::SignatureInvalid.sentence()
                )),
            "{log:?}"
        );
    }
}
