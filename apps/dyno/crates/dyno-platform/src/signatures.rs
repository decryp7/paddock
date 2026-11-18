//! Who signed a program file, for many files in one call (rule 85; ADR 0108 DY-5 and decision D-DY2: `codesign` with a cache and
//! the batch shape). [`CachedSignatures`] is the one [`SignatureProbe`] of a system: it answers a file it has checked before from
//! a cache keyed by the file's path, size and modification time, and asks a [`SignatureLookup`] for the rest in one call. The
//! lookups: [`CodesignLookup`] on macOS (one `codesign` run for the batch, continued after a file it cannot read), the package
//! managers on Linux (`crate::linux::packages`), none on Windows yet (the details there have never shown a signature).
//!
//! The cache is shared by the agent's look at new programs and the details a person opens ([`system`]), and its lock is never
//! held while a tool runs.

use dyno_core::ports::{PlatformError, SignatureProbe};
use dyno_core::process::SignatureInfo;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, SystemTime};

/// Answers kept; the cache is emptied when it grows past this (a bound on a machine that runs many short-lived programs).
pub const CACHE_LIMIT: usize = 1024;
/// How long one `codesign` run may take (it reads the signature, it does not verify the whole file).
const CODESIGN_TIMEOUT: Duration = Duration::from_secs(8);
const CODESIGN: &str = "/usr/bin/codesign";

/// One file's answer from a [`SignatureLookup`]: kept while the file is unchanged, or asked again next time (a tool that timed
/// out or could not start may answer later).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Looked {
    /// The file's answer as long as it is unchanged.
    Kept(SignatureInfo),
    /// This time's answer only.
    Once(SignatureInfo),
}

impl Looked {
    fn info(&self) -> &SignatureInfo {
        match self {
            Looked::Kept(i) | Looked::Once(i) => i,
        }
    }
}

/// The OS-specific check behind [`CachedSignatures`]: one answer per file, in order, for as few tool runs as the tool allows.
pub trait SignatureLookup: Send + Sync {
    /// Checks every file of `exes` now.
    fn lookup(&self, exes: &[&Path]) -> Vec<Looked>;
}

/// A file as the cache knows it: unchanged while its path, size and modification time are.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileKey {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
}

fn key_of(path: &Path) -> Option<FileKey> {
    if !path.is_absolute() {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    Some(FileKey {
        path: path.to_path_buf(),
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// The cached probe: files checked before come from the cache, the rest from one lookup call.
pub struct CachedSignatures {
    lookup: Box<dyn SignatureLookup>,
    cache: Mutex<HashMap<FileKey, SignatureInfo>>,
}

impl CachedSignatures {
    /// A probe over `lookup` with an empty cache.
    pub fn new(lookup: Box<dyn SignatureLookup>) -> CachedSignatures {
        CachedSignatures {
            lookup,
            cache: Mutex::new(HashMap::new()),
        }
    }
}

impl SignatureProbe for CachedSignatures {
    fn signatures(&self, exes: &[&Path]) -> Vec<SignatureInfo> {
        let keys: Vec<Option<FileKey>> = exes.iter().map(|p| key_of(p)).collect();
        let mut out: Vec<Option<SignatureInfo>> = {
            let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            keys.iter().map(|k| k.as_ref().and_then(|k| cache.get(k).cloned())).collect()
        };
        // Each file not in the cache once, in first-seen order; the tool runs without the cache's lock.
        let mut wanted: Vec<&Path> = Vec::new();
        for (i, p) in exes.iter().enumerate() {
            if out[i].is_none() && !wanted.contains(p) {
                wanted.push(p);
            }
        }
        if !wanted.is_empty() {
            let looked = self.lookup.lookup(&wanted);
            let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            for (j, path) in wanted.iter().enumerate() {
                let answer = looked
                    .get(j)
                    .cloned()
                    .unwrap_or_else(|| Looked::Once(SignatureInfo::Unknown("The signature check gave no answer for this file".into())));
                for (i, p) in exes.iter().enumerate() {
                    if p == path && out[i].is_none() {
                        out[i] = Some(answer.info().clone());
                        if let (Looked::Kept(info), Some(key)) = (&answer, &keys[i]) {
                            if cache.len() >= CACHE_LIMIT {
                                cache.clear();
                            }
                            cache.insert(key.clone(), info.clone());
                        }
                    }
                }
            }
        }
        out.into_iter()
            .map(|a| a.unwrap_or_else(|| SignatureInfo::Unknown("The signature check gave no answer for this file".into())))
            .collect()
    }
}

/// What a system without a signature check says for every file.
pub struct NoSignatures;

impl SignatureProbe for NoSignatures {
    fn signatures(&self, exes: &[&Path]) -> Vec<SignatureInfo> {
        exes.iter()
            .map(|_| SignatureInfo::Unknown("Not available on this OS".into()))
            .collect()
    }
}

/// One `codesign -dv --verbose=2 <paths>` run: whether it exited successfully, and its error output (where codesign reports).
pub trait CodesignRun: Send + Sync {
    /// Runs codesign over `paths`.
    fn run(&self, paths: &[String]) -> Result<(bool, String), PlatformError>;
}

/// The real `/usr/bin/codesign`, with a timeout and bounded output.
pub struct SystemCodesign;

impl CodesignRun for SystemCodesign {
    fn run(&self, paths: &[String]) -> Result<(bool, String), PlatformError> {
        let mut cmd = crate::process::command(CODESIGN);
        cmd.args(["-dv", "--verbose=2"]).args(paths);
        let out = crate::process::run_with_timeout(cmd, CODESIGN_TIMEOUT)?;
        Ok((out.status.success(), String::from_utf8_lossy(&out.stderr).into_owned()))
    }
}

/// macOS: `codesign` over the whole batch. It stops at the first file it cannot read (an unsigned one), so the run is repeated
/// for the files after it: one run for a batch of signed programs, one more per unsigned file before the last.
pub struct CodesignLookup {
    run: Box<dyn CodesignRun>,
}

impl CodesignLookup {
    /// A lookup through `run`.
    pub fn new(run: Box<dyn CodesignRun>) -> CodesignLookup {
        CodesignLookup { run }
    }
}

impl SignatureLookup for CodesignLookup {
    fn lookup(&self, exes: &[&Path]) -> Vec<Looked> {
        let paths: Vec<String> = exes.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let mut out: Vec<Looked> = Vec::with_capacity(paths.len());
        let mut runs = 0usize;
        while out.len() < paths.len() {
            let rest = &paths[out.len()..];
            runs += 1;
            match self.run.run(rest) {
                Ok((ok, text)) => {
                    let answered = crate::parse::mac_tools::split_codesign(rest, &text, ok);
                    // split_codesign answers at least one file of a non-empty batch, so every run moves on.
                    out.extend(answered.into_iter().take(rest.len()).map(Looked::Kept));
                }
                Err(e) => {
                    log::warn!(files = rest.len(); "codesign could not be run for a signature check: {e}");
                    out.extend(rest.iter().map(|_| Looked::Once(SignatureInfo::Unknown(e.to_string()))));
                }
            }
        }
        log::debug!(files = paths.len(), runs = runs; "Signatures checked with codesign");
        out
    }
}

/// This system's probe, one for the whole process: the agent's look at new programs and the details of a process share its
/// cache (two `Platform`s are built, one for the sampler and one for details, and both hand out this one).
pub fn system() -> Arc<dyn SignatureProbe> {
    static SYSTEM: OnceLock<Arc<dyn SignatureProbe>> = OnceLock::new();
    Arc::clone(SYSTEM.get_or_init(|| {
        #[cfg(target_os = "macos")]
        let probe: Arc<dyn SignatureProbe> = Arc::new(CachedSignatures::new(Box::new(CodesignLookup::new(Box::new(SystemCodesign)))));
        #[cfg(target_os = "linux")]
        let probe: Arc<dyn SignatureProbe> = Arc::new(CachedSignatures::new(Box::new(crate::linux::packages::PackageLookup::new(
            Arc::new(crate::linux::tools::SystemTools),
        ))));
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let probe: Arc<dyn SignatureProbe> = Arc::new(NoSignatures);
        probe
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_proc::starts::{assert_starts, at_most_or_none};

    /// A codesign that answers from a script and makes, per run, the `Command` the real one would (so starts are counted).
    struct Scripted(fn(&[String]) -> (bool, String));
    impl CodesignRun for Scripted {
        fn run(&self, paths: &[String]) -> Result<(bool, String), PlatformError> {
            let _counted = crate::process::command(CODESIGN);
            Ok((self.0)(paths))
        }
    }

    /// Signed unless the name has "plain" in it; stops at the first unsigned file, as codesign does.
    fn codesign(paths: &[String]) -> (bool, String) {
        let mut text = String::new();
        for p in paths {
            if p.contains("plain") {
                text.push_str(&format!("{p}: code object is not signed at all\n"));
                return (false, text);
            }
            text.push_str(&format!("Executable={p}\nAuthority=Signer of {p}\n"));
        }
        (true, text)
    }

    fn files(dir: &Path, names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|n| {
                let p = dir.join(n);
                std::fs::write(&p, n.as_bytes()).unwrap();
                p
            })
            .collect()
    }

    /// DY-5 (ADR 0108, guard D): new programs are checked in one `codesign` run, not one each; a file checked before costs
    /// nothing while it is unchanged.
    #[test]
    fn a_batch_of_signed_programs_is_one_codesign_run_and_then_cached() {
        let dir = paddock_testkit::private_tempdir().unwrap();
        let exes = files(dir.path(), &["a", "b", "c"]);
        let refs: Vec<&Path> = exes.iter().map(PathBuf::as_path).collect();
        let probe = CachedSignatures::new(Box::new(CodesignLookup::new(Box::new(Scripted(codesign)))));
        let got = assert_starts(1, || probe.signatures(&refs));
        assert_eq!(
            got,
            exes.iter()
                .map(|p| SignatureInfo::Signed(format!("Signer of {}", p.display())))
                .collect::<Vec<_>>()
        );
        let again = assert_starts(at_most_or_none(0, "every file is cached"), || probe.signatures(&refs));
        assert_eq!(again, got);
        // A changed file is checked again; the others still come from the cache.
        std::fs::write(&exes[1], b"a new version").unwrap();
        assert_starts(1, || probe.signatures(&refs));
    }

    #[test]
    fn an_unsigned_file_costs_one_more_run_and_every_file_gets_its_own_answer_in_order() {
        let dir = paddock_testkit::private_tempdir().unwrap();
        let exes = files(dir.path(), &["a", "plain", "c", "c"]);
        let mut refs: Vec<&Path> = exes.iter().map(PathBuf::as_path).collect();
        refs.push(&exes[0]);
        let probe = CachedSignatures::new(Box::new(CodesignLookup::new(Box::new(Scripted(codesign)))));
        let got = assert_starts(2, || probe.signatures(&refs));
        assert_eq!(got.len(), 5);
        assert!(matches!(&got[0], SignatureInfo::Signed(_)));
        assert_eq!(got[1], SignatureInfo::Unsigned);
        assert!(matches!(&got[2], SignatureInfo::Signed(s) if s.ends_with("/c")));
        assert_eq!(got[4], got[0], "a file named twice is checked once");
    }

    #[test]
    fn a_tool_that_cannot_run_is_asked_again_next_time() {
        struct Broken;
        impl CodesignRun for Broken {
            fn run(&self, _: &[String]) -> Result<(bool, String), PlatformError> {
                let _counted = crate::process::command(CODESIGN);
                Err(PlatformError::Timeout("codesign did not finish within 8 s.".into()))
            }
        }
        let dir = paddock_testkit::private_tempdir().unwrap();
        let exes = files(dir.path(), &["a", "b"]);
        let refs: Vec<&Path> = exes.iter().map(PathBuf::as_path).collect();
        let probe = CachedSignatures::new(Box::new(CodesignLookup::new(Box::new(Broken))));
        let got = assert_starts(1, || probe.signatures(&refs));
        assert!(got.iter().all(|s| matches!(s, SignatureInfo::Unknown(m) if m.contains("8 s"))));
        assert_starts(1, || probe.signatures(&refs));
    }

    #[test]
    fn files_that_cannot_be_keyed_are_checked_but_never_cached() {
        let probe = CachedSignatures::new(Box::new(CodesignLookup::new(Box::new(Scripted(codesign)))));
        let gone = Path::new("/no/such/dyno-program");
        assert_starts(1, || probe.signatures(&[gone]));
        assert_starts(1, || probe.signatures(&[gone]));
        assert!(probe.signatures(&[]).is_empty());
        assert_eq!(
            NoSignatures.signatures(&[gone]),
            [SignatureInfo::Unknown("Not available on this OS".into())]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_real_codesign_reads_a_system_program_and_an_unsigned_file_in_one_batch() {
        let dir = paddock_testkit::private_tempdir().unwrap();
        let plain = dir.path().join("plain");
        std::fs::write(&plain, b"not a program").unwrap();
        let probe = CachedSignatures::new(Box::new(CodesignLookup::new(Box::new(SystemCodesign))));
        let got = probe.signatures(&[Path::new("/bin/ls"), &plain, Path::new("/bin/cat")]);
        assert!(matches!(got[0], SignatureInfo::Signed(_) | SignatureInfo::Invalid(_)), "{got:?}");
        assert!(!matches!(got[1], SignatureInfo::Signed(_)), "{got:?}");
        assert!(matches!(got[2], SignatureInfo::Signed(_) | SignatureInfo::Invalid(_)), "{got:?}");
    }
}
