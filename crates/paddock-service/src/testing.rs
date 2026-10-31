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
    cpu_times: BTreeMap<u32, Duration>,
    run_states: BTreeMap<u32, paddock_proc::RunState>,
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

    /// Makes `pid` have used `cpu` of CPU time (what [`System::cpu_time`] answers; other pids are not known).
    pub fn set_cpu_time(&self, pid: u32, cpu: Duration) -> &Self {
        self.lock().cpu_times.insert(pid, cpu);
        self
    }

    /// Makes `pid` have the run state `state` (what [`System::run_state`] answers; other pids are `Unknown`).
    pub fn set_run_state(&self, pid: u32, state: paddock_proc::RunState) -> &Self {
        self.lock().run_states.insert(pid, state);
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
        match rule {
            Some((_, respond)) => respond(&words, now),
            None => Ok(Output::code(0)),
        }
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

    fn cpu_time(&mut self, pid: u32) -> Option<Duration> {
        self.lock().cpu_times.get(&pid).copied()
    }

    fn run_state(&mut self, pid: u32) -> paddock_proc::RunState {
        self.lock()
            .run_states
            .get(&pid)
            .copied()
            .unwrap_or(paddock_proc::RunState::Unknown)
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
        _ceiling: Duration,
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
}
