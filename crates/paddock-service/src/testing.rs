//! Fakes of the ports for tests (this crate's and the apps'): they record every command, file write and wait, keep
//! files in memory and never touch the OS (rule 7). Time is virtual: [`FakeSystem::now`] only moves when something
//! pauses, so bounded waits are tested without sleeping.

use crate::port::{
    AppService, AppServiceStatus, Elevator, Output, RegisterError, RunError, System,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// One thing a fake was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// A helper program: the program and its arguments joined by spaces.
    Run(String),
    /// A detached start.
    Spawn(String),
    /// A file write: the path and the text.
    Write(PathBuf, String),
    /// A file removal.
    Remove(PathBuf),
    /// A wait.
    Pause(Duration),
    /// An `SMAppService` or elevation call (`register`, `unregister`, `openLoginItems`, `elevated <program> <args>`).
    Os(String),
}

/// An answer to a command: given the arguments (program first) and the virtual time.
pub type Responder = Box<dyn FnMut(&[String], Duration) -> Result<Output, RunError> + Send>;

#[derive(Default)]
struct Inner {
    files: BTreeMap<PathBuf, String>,
    programs: BTreeSet<PathBuf>,
    calls: Vec<Call>,
    now: Duration,
    rules: Vec<(String, Responder)>,
    write_error: Option<std::io::ErrorKind>,
    remove_error: Option<std::io::ErrorKind>,
    read_error: Option<std::io::ErrorKind>,
    spawn_error: Option<String>,
    effects: Vec<(String, PathBuf, String)>,
}

/// A recording [`System`]. Clones share one state, so a test keeps a handle while the backend owns another. Commands
/// without a responder succeed with no output.
#[derive(Clone, Default)]
pub struct FakeSystem {
    inner: Arc<Mutex<Inner>>,
}

impl FakeSystem {
    /// An empty machine.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Answers every command line starting with `prefix` (program and arguments joined by spaces). A later responder
    /// for the same commands wins.
    pub fn on(
        &self,
        prefix: &str,
        respond: impl FnMut(&[String], Duration) -> Result<Output, RunError> + Send + 'static,
    ) -> &Self {
        self.lock()
            .rules
            .push((prefix.to_string(), Box::new(respond)));
        self
    }

    /// Answers every command line starting with `prefix` with `out`.
    pub fn answer(&self, prefix: &str, out: Output) -> &Self {
        self.on(prefix, move |_, _| Ok(out.clone()))
    }

    /// After every command starting with `prefix`, the file `path` holds `text` (something else rewrote it: a drop-in, a
    /// package upgrade). The effect applies once the command has been answered.
    pub fn rewrite_after(&self, prefix: &str, path: impl Into<PathBuf>, text: &str) -> &Self {
        self.lock()
            .effects
            .push((prefix.to_string(), path.into(), text.to_string()));
        self
    }

    /// Makes `reg add`, `reg delete` and `reg query` on the per-user `Run` key behave like a registry: a value added reads
    /// back as added (`REG_SZ`), a deleted one is gone (exit 1), and any other key reads as missing. A later
    /// `answer("reg query", ..)` still wins, so a test can show what someone else left there.
    pub fn emulate_run_registry(&self) -> &Self {
        let values: Arc<Mutex<BTreeMap<String, String>>> = Arc::default();
        let word = |args: &[String], flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let store = values.clone();
        self.on("reg add", move |args, _| {
            if let (Some(name), Some(data)) = (word(args, "/v"), word(args, "/d")) {
                store
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(name, data);
            }
            Ok(Output::code(0))
        });
        let store = values.clone();
        self.on("reg delete", move |args, _| {
            let removed = word(args, "/v").and_then(|name| {
                store
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&name)
            });
            Ok(Output::code(if removed.is_some() { 0 } else { 1 }))
        });
        self.on("reg query", move |args, _| {
            let data = args
                .get(2)
                .filter(|key| key.ends_with("CurrentVersion\\Run"))
                .and_then(|_| word(args, "/v"))
                .and_then(|name| {
                    values
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .get(&name)
                        .map(|data| (name, data.clone()))
                });
            Ok(match data {
                Some((name, data)) => Output::ok(&format!("    {name}    REG_SZ    {data}\r\n")),
                None => Output::code(1),
            })
        });
        self
    }

    /// Puts a file on the fake disk.
    pub fn put_file(&self, path: impl Into<PathBuf>, text: &str) -> &Self {
        self.lock().files.insert(path.into(), text.to_string());
        self
    }

    /// The file's text, if any.
    pub fn file(&self, path: &Path) -> Option<String> {
        self.lock().files.get(path).cloned()
    }

    /// Makes `path` an existing, non-empty program.
    pub fn put_program(&self, path: impl Into<PathBuf>) -> &Self {
        self.lock().programs.insert(path.into());
        self
    }

    /// Removes a program (an app moved or deleted).
    pub fn remove_program(&self, path: &Path) -> &Self {
        self.lock().programs.remove(path);
        self
    }

    /// Makes every write fail with `kind` (`None` clears it).
    pub fn fail_writes(&self, kind: Option<std::io::ErrorKind>) -> &Self {
        self.lock().write_error = kind;
        self
    }

    /// Makes every read fail with `kind` (`None` clears it).
    pub fn fail_reads(&self, kind: Option<std::io::ErrorKind>) -> &Self {
        self.lock().read_error = kind;
        self
    }

    /// Makes every removal fail with `kind` (`None` clears it).
    pub fn fail_removes(&self, kind: Option<std::io::ErrorKind>) -> &Self {
        self.lock().remove_error = kind;
        self
    }

    /// Makes every detached start fail.
    pub fn fail_spawns(&self, why: Option<&str>) -> &Self {
        self.lock().spawn_error = why.map(str::to_string);
        self
    }

    /// Everything asked so far.
    pub fn calls(&self) -> Vec<Call> {
        self.lock().calls.clone()
    }

    /// The commands run so far, as text.
    pub fn commands(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Run(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    /// Forgets the calls recorded so far.
    pub fn clear_calls(&self) {
        self.lock().calls.clear();
    }

    /// The virtual time: the sum of every pause.
    pub fn now(&self) -> Duration {
        self.lock().now
    }

    /// Records an OS call made by another fake that shares this log.
    pub fn record(&self, call: Call) {
        self.lock().calls.push(call);
    }
}

impl System for FakeSystem {
    fn run(&mut self, program: &str, args: &[&str]) -> Result<Output, RunError> {
        let mut words = vec![program.to_string()];
        words.extend(args.iter().map(|a| a.to_string()));
        let line = words.join(" ");
        let mut inner = self.lock();
        inner.calls.push(Call::Run(line.clone()));
        let now = inner.now;
        let rule = inner
            .rules
            .iter_mut()
            .rev()
            .find(|(p, _)| line.starts_with(p.as_str()));
        let answer = match rule {
            Some((_, respond)) => respond(&words, now),
            None => Ok(Output::code(0)),
        };
        let rewrites: Vec<(PathBuf, String)> = inner
            .effects
            .iter()
            .filter(|(p, _, _)| line.starts_with(p.as_str()))
            .map(|(_, path, text)| (path.clone(), text.clone()))
            .collect();
        inner.files.extend(rewrites);
        answer
    }

    fn spawn(&mut self, program: &Path, args: &[String]) -> Result<(), String> {
        let mut inner = self.lock();
        let line = std::iter::once(program.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        inner.calls.push(Call::Spawn(line));
        match &inner.spawn_error {
            Some(why) => Err(why.clone()),
            None => Ok(()),
        }
    }

    fn read(&mut self, path: &Path) -> std::io::Result<String> {
        let inner = self.lock();
        if let Some(kind) = inner.read_error {
            return Err(std::io::Error::from(kind));
        }
        inner
            .files
            .get(path)
            .cloned()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    }

    fn write(&mut self, path: &Path, text: &str) -> std::io::Result<()> {
        let mut inner = self.lock();
        inner
            .calls
            .push(Call::Write(path.to_path_buf(), text.to_string()));
        if let Some(kind) = inner.write_error {
            return Err(std::io::Error::from(kind));
        }
        inner.files.insert(path.to_path_buf(), text.to_string());
        Ok(())
    }

    fn remove(&mut self, path: &Path) -> std::io::Result<()> {
        let mut inner = self.lock();
        inner.calls.push(Call::Remove(path.to_path_buf()));
        if let Some(kind) = inner.remove_error {
            return Err(std::io::Error::from(kind));
        }
        inner
            .files
            .remove(path)
            .map(drop)
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    }

    fn is_program(&mut self, path: &Path) -> bool {
        self.lock().programs.contains(path)
    }

    fn pause(&mut self, how_long: Duration) {
        let mut inner = self.lock();
        inner.calls.push(Call::Pause(how_long));
        inner.now += how_long;
    }
}

#[derive(Default)]
struct AppInner {
    status: Option<AppServiceStatus>,
    register: VecDeque<Result<(), RegisterError>>,
    unregister_error: Option<String>,
    register_sets: Option<AppServiceStatus>,
}

/// A scripted `SMAppService`. Its calls go to the [`FakeSystem`] it is made with, so one log shows the order of
/// everything. `register` succeeds and makes the status `Enabled` unless scripted otherwise.
#[derive(Clone)]
pub struct FakeAppService {
    inner: Arc<Mutex<AppInner>>,
    log: FakeSystem,
}

impl FakeAppService {
    /// Not registered, logging into `log`.
    pub fn new(log: &FakeSystem) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AppInner {
                status: Some(AppServiceStatus::NotRegistered),
                register_sets: Some(AppServiceStatus::Enabled),
                ..AppInner::default()
            })),
            log: log.clone(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, AppInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets the status.
    pub fn set_status(&self, s: AppServiceStatus) -> &Self {
        self.lock().status = Some(s);
        self
    }

    /// Queues the result of the next `register` calls (then they succeed again).
    pub fn register_results(
        &self,
        results: impl IntoIterator<Item = Result<(), RegisterError>>,
    ) -> &Self {
        self.lock().register.extend(results);
        self
    }

    /// The status a successful `register` leaves (`None`: unchanged).
    pub fn set_register_sets(&self, status: Option<AppServiceStatus>) -> &Self {
        self.lock().register_sets = status;
        self
    }

    /// Makes `unregister` fail.
    pub fn fail_unregister(&self, why: Option<&str>) -> &Self {
        self.lock().unregister_error = why.map(str::to_string);
        self
    }
}

impl AppService for FakeAppService {
    fn status(&mut self) -> AppServiceStatus {
        self.lock()
            .status
            .unwrap_or(AppServiceStatus::NotRegistered)
    }

    fn register(&mut self) -> Result<(), RegisterError> {
        self.log.record(Call::Os("register".into()));
        let mut inner = self.lock();
        let result = inner.register.pop_front().unwrap_or(Ok(()));
        if result.is_ok() {
            inner.status = inner.register_sets;
        } else if result == Err(RegisterError::NeedsApproval) {
            inner.status = Some(AppServiceStatus::RequiresApproval);
        }
        result
    }

    fn unregister(&mut self) -> Result<(), String> {
        self.log.record(Call::Os("unregister".into()));
        let mut inner = self.lock();
        if let Some(why) = &inner.unregister_error {
            return Err(why.clone());
        }
        inner.status = Some(AppServiceStatus::NotRegistered);
        Ok(())
    }

    fn open_login_items(&mut self) {
        self.log.record(Call::Os("openLoginItems".into()));
    }
}

/// A scripted elevation prompt: records the call and answers with the queued results (then exit code 0).
#[derive(Clone)]
pub struct FakeElevator {
    results: Arc<Mutex<VecDeque<Result<i32, RegisterError>>>>,
    log: FakeSystem,
}

impl FakeElevator {
    /// Answers 0, logging into `log`.
    pub fn new(log: &FakeSystem) -> Self {
        Self {
            results: Arc::default(),
            log: log.clone(),
        }
    }

    /// Queues results.
    pub fn results(&self, r: impl IntoIterator<Item = Result<i32, RegisterError>>) -> &Self {
        self.results
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(r);
        self
    }
}

impl Elevator for FakeElevator {
    fn run_elevated(
        &mut self,
        program: &Path,
        args: &[&str],
        _timeout: Duration,
    ) -> Result<i32, RegisterError> {
        self.log.record(Call::Os(format!(
            "elevated {} {}",
            program.display(),
            args.join(" ")
        )));
        self.results
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .unwrap_or(Ok(0))
    }
}

/// A [`ProtectedFiles`](crate::privileged::ProtectedFiles) on an in-memory disk: records every call, keeps the owner and
/// mode of what it creates (root and the mode asked for, unless told otherwise) and hashes content with the real SHA-256.
#[derive(Clone, Default)]
pub struct FakeFiles {
    inner: Arc<Mutex<FilesInner>>,
}

#[derive(Default)]
struct FilesInner {
    entries: BTreeMap<PathBuf, crate::privileged::FileFacts>,
    content: BTreeMap<PathBuf, Vec<u8>>,
    calls: Vec<String>,
    no_owner: bool,
    new_owner: Option<u32>,
    tamper: bool,
    fail: Option<std::io::ErrorKind>,
}

impl FakeFiles {
    /// An empty disk on a Unix-like system; new files belong to root.
    pub fn new() -> Self {
        Self::default()
    }

    /// A disk that reports neither owner nor mode (Windows).
    pub fn windows() -> Self {
        let f = Self::default();
        f.lock().no_owner = true;
        f
    }

    fn lock(&self) -> MutexGuard<'_, FilesInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Puts a file with `content`, owner and mode.
    pub fn put_file(
        &self,
        path: impl Into<PathBuf>,
        content: &[u8],
        owner: u32,
        mode: u32,
    ) -> &Self {
        use crate::privileged::{FileFacts, FileKind};
        let path = path.into();
        let mut inner = self.lock();
        let facts = FileFacts {
            kind: FileKind::File,
            owner: (!inner.no_owner).then_some(owner),
            mode: (!inner.no_owner).then_some(mode),
            len: content.len() as u64,
        };
        inner.entries.insert(path.clone(), facts);
        inner.content.insert(path, content.to_vec());
        drop(inner);
        self
    }

    /// Puts a folder with owner and mode.
    pub fn put_dir(&self, path: impl Into<PathBuf>, owner: u32, mode: u32) -> &Self {
        use crate::privileged::{FileFacts, FileKind};
        let mut inner = self.lock();
        let facts = FileFacts {
            kind: FileKind::Dir,
            owner: (!inner.no_owner).then_some(owner),
            mode: (!inner.no_owner).then_some(mode),
            len: 0,
        };
        inner.entries.insert(path.into(), facts);
        drop(inner);
        self
    }

    /// Puts a symbolic link.
    pub fn put_symlink(&self, path: impl Into<PathBuf>) -> &Self {
        use crate::privileged::{FileFacts, FileKind};
        self.lock().entries.insert(
            path.into(),
            FileFacts {
                kind: FileKind::Symlink,
                owner: Some(0),
                mode: Some(0o777),
                len: 0,
            },
        );
        self
    }

    /// The owner new files and folders get (default: root, 0).
    pub fn new_files_belong_to(&self, uid: u32) -> &Self {
        self.lock().new_owner = Some(uid);
        self
    }

    /// Makes the next hash reads differ from what was written (a copy changed under the installer).
    pub fn tamper(&self, on: bool) -> &Self {
        self.lock().tamper = on;
        self
    }

    /// Makes every copy and folder creation fail with `kind` (`None` clears it).
    pub fn fail_with(&self, kind: Option<std::io::ErrorKind>) -> &Self {
        self.lock().fail = kind;
        self
    }

    /// What exists at `path`.
    pub fn facts_of(&self, path: &Path) -> Option<crate::privileged::FileFacts> {
        self.lock().entries.get(path).cloned()
    }

    /// Everything asked so far, as `verb path`.
    pub fn calls(&self) -> Vec<String> {
        self.lock().calls.clone()
    }
}

impl crate::privileged::ProtectedFiles for FakeFiles {
    fn facts(&mut self, path: &Path) -> std::io::Result<Option<crate::privileged::FileFacts>> {
        let mut inner = self.lock();
        inner.calls.push(format!("facts {}", path.display()));
        Ok(inner.entries.get(path).cloned())
    }

    fn create_dir(&mut self, path: &Path, mode: u32) -> std::io::Result<()> {
        use crate::privileged::{FileFacts, FileKind};
        let mut inner = self.lock();
        inner
            .calls
            .push(format!("create_dir {} {mode:o}", path.display()));
        if let Some(kind) = inner.fail {
            return Err(std::io::Error::from(kind));
        }
        let owner = inner.new_owner.unwrap_or(0);
        let no_owner = inner.no_owner;
        inner.entries.insert(
            path.to_path_buf(),
            FileFacts {
                kind: FileKind::Dir,
                owner: (!no_owner).then_some(owner),
                mode: (!no_owner).then_some(mode),
                len: 0,
            },
        );
        Ok(())
    }

    fn copy_program(&mut self, from: &Path, to: &Path, mode: u32) -> std::io::Result<String> {
        use crate::privileged::{FileFacts, FileKind};
        let mut inner = self.lock();
        inner
            .calls
            .push(format!("copy {} {}", from.display(), to.display()));
        if let Some(kind) = inner.fail {
            return Err(std::io::Error::from(kind));
        }
        let bytes = inner
            .content
            .get(from)
            .cloned()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
        let owner = inner.new_owner.unwrap_or(0);
        let no_owner = inner.no_owner;
        inner.entries.insert(
            to.to_path_buf(),
            FileFacts {
                kind: FileKind::File,
                owner: (!no_owner).then_some(owner),
                mode: (!no_owner).then_some(mode),
                len: bytes.len() as u64,
            },
        );
        let hash = crate::privileged::files::hash_bytes(&bytes);
        inner.content.insert(to.to_path_buf(), bytes);
        Ok(hash)
    }

    fn sha256(&mut self, path: &Path) -> std::io::Result<String> {
        let mut inner = self.lock();
        inner.calls.push(format!("sha256 {}", path.display()));
        let bytes = inner
            .content
            .get(path)
            .cloned()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
        let mut hash = crate::privileged::files::hash_bytes(&bytes);
        if inner.tamper {
            hash.replace_range(0..1, if hash.starts_with('0') { "1" } else { "0" });
        }
        Ok(hash)
    }

    fn read_checked(
        &mut self,
        path: &Path,
        max: u64,
    ) -> std::io::Result<Option<(crate::privileged::FileFacts, String)>> {
        let mut inner = self.lock();
        inner.calls.push(format!("read {}", path.display()));
        let Some(facts) = inner.entries.get(path).cloned() else {
            return Ok(None);
        };
        let bytes = inner.content.get(path).cloned().unwrap_or_default();
        if bytes.len() as u64 > max {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
        }
        Ok(Some((facts, String::from_utf8_lossy(&bytes).into_owned())))
    }

    fn remove_file(&mut self, path: &Path) -> std::io::Result<()> {
        let mut inner = self.lock();
        inner.calls.push(format!("remove_file {}", path.display()));
        inner.content.remove(path);
        inner
            .entries
            .remove(path)
            .map(drop)
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    }

    fn remove_dir(&mut self, path: &Path) -> std::io::Result<()> {
        let mut inner = self.lock();
        inner.calls.push(format!("remove_dir {}", path.display()));
        // Compared as text: a Windows path is not split by a Unix host's separators.
        let base = path.to_string_lossy().into_owned();
        let inside = |k: &Path| {
            let k = k.to_string_lossy();
            k.len() > base.len()
                && k.starts_with(base.as_str())
                && matches!(k.as_bytes()[base.len()], b'/' | b'\\')
        };
        if inner.entries.keys().any(|k| inside(k)) {
            return Err(std::io::Error::other("the folder is not empty"));
        }
        inner
            .entries
            .remove(path)
            .map(drop)
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    }
}

/// An [`OsLog`](crate::os_log::OsLog) that keeps every line (tests of a service's start-up and failures).
#[derive(Clone, Default)]
pub struct RecordingOsLog(Arc<Mutex<Vec<(log::Level, String)>>>);

impl RecordingOsLog {
    /// Every line written so far, with its level.
    pub fn lines(&self) -> Vec<(log::Level, String)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl crate::os_log::OsLog for RecordingOsLog {
    fn write(&self, level: log::Level, text: &str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((level, text.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_records_answers_and_keeps_files_in_memory() {
        let f = FakeSystem::new();
        let mut s = f.clone();
        f.answer("launchctl print", Output::ok("state = running"));
        f.on("launchctl print gui/1/x", |w, now| {
            Ok(Output::ok(&format!("{} {}", w.len(), now.as_secs())))
        });
        assert_eq!(
            s.run("launchctl", &["print", "gui/1/x"]).unwrap().stdout,
            "3 0"
        );
        assert_eq!(
            s.run("launchctl", &["print", "gui/1/y"]).unwrap().stdout,
            "state = running"
        );
        assert!(s.run("other", &[]).unwrap().success());
        s.pause(Duration::from_secs(2));
        assert_eq!(f.now(), Duration::from_secs(2));
        s.write(Path::new("/a"), "t").unwrap();
        assert_eq!(s.read(Path::new("/a")).unwrap(), "t");
        assert_eq!(f.file(Path::new("/a")).as_deref(), Some("t"));
        s.remove(Path::new("/a")).unwrap();
        assert_eq!(
            s.remove(Path::new("/a")).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        assert_eq!(
            s.read(Path::new("/a")).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        f.fail_writes(Some(std::io::ErrorKind::PermissionDenied));
        assert!(s.write(Path::new("/b"), "x").is_err());
        f.fail_removes(Some(std::io::ErrorKind::PermissionDenied))
            .put_file("/c", "x");
        assert!(s.remove(Path::new("/c")).is_err());
        f.put_program("/p");
        assert!(s.is_program(Path::new("/p")));
        f.remove_program(Path::new("/p"));
        assert!(!s.is_program(Path::new("/p")));
        assert!(s.spawn(Path::new("/p"), &["run".into()]).is_ok());
        f.fail_spawns(Some("no"));
        assert_eq!(s.spawn(Path::new("/p"), &[]).unwrap_err(), "no");
        assert_eq!(f.commands().len(), 3);
        assert!(f.calls().contains(&Call::Spawn("/p run".into())));
        f.clear_calls();
        assert!(f.calls().is_empty());
    }

    #[test]
    fn the_fake_app_service_and_elevator_follow_their_script() {
        let log = FakeSystem::new();
        let mut a = FakeAppService::new(&log);
        assert_eq!(a.status(), AppServiceStatus::NotRegistered);
        a.register_results([
            Err(RegisterError::NeedsApproval),
            Err(RegisterError::Other("x".into())),
        ]);
        assert_eq!(a.register(), Err(RegisterError::NeedsApproval));
        assert_eq!(a.status(), AppServiceStatus::RequiresApproval);
        assert!(a.register().is_err());
        assert!(a.register().is_ok());
        assert_eq!(a.status(), AppServiceStatus::Enabled);
        a.fail_unregister(Some("gone"));
        assert_eq!(a.unregister().unwrap_err(), "gone");
        a.fail_unregister(None)
            .set_status(AppServiceStatus::Enabled);
        assert!(a.unregister().is_ok());
        a.open_login_items();
        let mut e = FakeElevator::new(&log);
        e.results([Err(RegisterError::Declined)]);
        assert_eq!(
            e.run_elevated(Path::new("/p"), &["install"], Duration::ZERO),
            Err(RegisterError::Declined)
        );
        assert_eq!(
            e.run_elevated(Path::new("/p"), &["install"], Duration::ZERO),
            Ok(0)
        );
        assert!(log.calls().contains(&Call::Os("openLoginItems".into())));
        assert!(
            log.calls()
                .contains(&Call::Os("elevated /p install".into()))
        );
    }

    #[test]
    fn the_fake_disk_reads_with_facts_and_a_bound() {
        use crate::privileged::ProtectedFiles;
        let files = FakeFiles::new();
        let mut disk = files.clone();
        assert_eq!(disk.read_checked(Path::new("/p"), 9).unwrap(), None);
        files.put_file("/p", b"others", 0, 0o644);
        let (facts, text) = disk.read_checked(Path::new("/p"), 9).unwrap().unwrap();
        assert_eq!((facts.owner, text.as_str()), (Some(0), "others"));
        assert!(disk.read_checked(Path::new("/p"), 3).is_err());
        assert!(files.calls().iter().any(|c| c == "read /p"));
    }
}
