//! macOS: the app's own agent runs as a launchd job, never as the app's child (Dyno ADR 0026, amendment "the app starts its
//! agent through launchd").
//!
//! On macOS 27 the kernel's socket list is cut down to a process's own sockets when that process is the child of an ad-hoc
//! signed app that LaunchServices started; the same program started by launchd sees every socket. So [`LaunchdLauncher`]
//! writes a small job definition into Dyno's data folder (not `~/Library/LaunchAgents`: nothing starts it at the next login)
//! and loads it with `launchctl bootstrap gui/<uid>`; launchd starts the agent once (`RunAtLoad`, no `KeepAlive`). The job's
//! label is not the login service's ([`dyno_agent::service::LABEL`]), so the service probe, its repair and the hand-over never
//! mistake one for the other.
//!
//! The [`Launcher`] contract holds as for [`super::ProcessLauncher`]: one agent at a time, its pid while it runs, its exit
//! code once it has exited, nothing launched once closed, and `finish` ends exactly the job this launcher loaded. The process
//! is launchd's child, so it is ended through launchd (`launchctl bootout` of the job, never a signal to a pid, rule 53); the
//! pid comes from launchd's report of the job and its identity is taken from the live process at once ([`running_or_stopped`]).
//! Every launchctl call goes through the [`System`] port, so the tests run on a fake launchd, never the real one (rule 7).

use super::ports::{running_or_stopped, Child, Launcher};
use paddock_service::launchctl::{bootout, job_report};
use paddock_service::parse::JobReport;
use paddock_service::paths::LAUNCHCTL;
use paddock_service::port::System;
use paddock_service::render::xml_escape;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// The label of the job the app loads. Not the login service's label: the two must never be taken for each other.
pub const LABEL: &str = "net.decryptology.dyno.agent.ondemand";

/// The job definition's file name, in Dyno's data folder.
pub const PLIST_FILE: &str = "agent-ondemand.plist";

/// How long launchd gets to forget an old job before a new one is loaded, and how often it is asked meanwhile.
// cap: launchd drops a job it booted out within a second here; the bound only keeps a launchd that never answers from holding
// the launcher thread (the launch then fails with a message and the machine backs off).
const GONE_WAIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(50);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every guarded value stays consistent if a holder panicked.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The environment the agent needs from the app: a child inherits it, a launchd job does not. `DYNO_DATA_DIR` keeps the agent
/// on the app's data folder (tools and tests move it); nothing else of the app's environment is passed.
pub fn agent_env(var: impl Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    ["DYNO_DATA_DIR"]
        .into_iter()
        .filter_map(|k| var(k).filter(|v| !v.is_empty()).map(|v| (k.to_string(), v)))
        .collect()
}

/// The job definition: the agent, started once when loaded, never restarted by launchd (the app's link decides about a new
/// one), with `ExitTimeOut` as the grace a bootout gives it before launchd ends it, and `env` ([`agent_env`]). No
/// `AssociatedBundleIdentifiers` or `BundleProgram`: the job is a plain launchd job of the user, not one that macOS attributes
/// to the app.
pub fn job_plist(label: &str, exe: &Path, exit_grace: Duration, env: &[(String, String)]) -> String {
    let exe = xml_escape(&exe.to_string_lossy());
    let label = xml_escape(label);
    let grace = exit_grace.as_secs().max(1);
    let env = if env.is_empty() {
        String::new()
    } else {
        let pairs: String = env
            .iter()
            .map(|(k, v)| format!("\t\t<key>{}</key>\n\t\t<string>{}</string>\n", xml_escape(k), xml_escape(v)))
            .collect();
        format!("\t<key>EnvironmentVariables</key>\n\t<dict>\n{pairs}\t</dict>\n")
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <!-- Written by Dyno each time it starts its agent (Dyno ADR 0026). Loaded with launchctl bootstrap, never from \
         LaunchAgents: nothing starts it at login. -->\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>Label</key>\n\t<string>{label}</string>\n\
         \t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{exe}</string>\n\t\t<string>run</string>\n\t</array>\n\
         \t<key>RunAtLoad</key>\n\t<true/>\n\
         \t<key>KeepAlive</key>\n\t<false/>\n\
         \t<key>ExitTimeOut</key>\n\t<integer>{grace}</integer>\n\
         \t<key>ProcessType</key>\n\t<string>Standard</string>\n\
         \t<key>StandardInPath</key>\n\t<string>/dev/null</string>\n\
         \t<key>StandardOutPath</key>\n\t<string>/dev/null</string>\n\
         \t<key>StandardErrorPath</key>\n\t<string>/dev/null</string>\n\
         {env}</dict>\n</plist>\n"
    )
}

/// What this launcher knows of the job it loaded.
#[derive(Default)]
struct Slot {
    /// A job this launcher loaded may still run.
    loaded: bool,
    /// The pid launchd last reported for it (kept when launchd cannot be asked).
    pid: Option<u32>,
    /// How the last one exited, once seen (`Some(None)`: a signal, or launchd dropped it).
    exited: Option<Option<i32>>,
}

/// Starts the agent as a launchd job in the user's GUI domain (macOS).
pub struct LaunchdLauncher {
    exe: Box<dyn Fn() -> Result<PathBuf, String> + Send + Sync>,
    plist: PathBuf,
    domain: String,
    target: String,
    grace: Duration,
    env: Vec<(String, String)>,
    sys: Mutex<Box<dyn System>>,
    slot: Mutex<Slot>,
    closed: AtomicBool,
}

impl LaunchdLauncher {
    /// A launcher for the user `uid` whose program is found by `exe` each time (a reinstalled app is picked up), writing its job
    /// definition to `plist` and running launchctl through `sys`. `grace` is the job's `ExitTimeOut`, `env` the agent's environment
    /// ([`agent_env`]).
    pub fn new(
        exe: impl Fn() -> Result<PathBuf, String> + Send + Sync + 'static,
        plist: PathBuf,
        uid: u32,
        grace: Duration,
        env: Vec<(String, String)>,
        sys: Box<dyn System>,
    ) -> LaunchdLauncher {
        let domain = format!("gui/{uid}");
        LaunchdLauncher {
            exe: Box::new(exe),
            plist,
            target: format!("{domain}/{LABEL}"),
            domain,
            grace,
            env,
            sys: Mutex::new(sys),
            slot: Mutex::new(Slot::default()),
            closed: AtomicBool::new(false),
        }
    }

    /// launchd's report of the job (`None`: launchd could not be asked).
    fn report(&self) -> Option<JobReport> {
        job_report(lock(&self.sys).as_mut(), &self.target)
    }

    /// What the job is now, told once when it has exited (as [`super::ProcessLauncher`] does).
    fn look(&self, slot: &mut Slot) -> Child {
        if slot.loaded {
            match self.report() {
                // launchd cannot be asked: what was last known.
                None => return slot.pid.map_or(Child::None, running_or_stopped),
                Some(r) if !r.found => {
                    // Booted out by someone else (or launchd forgot it): it has ended, and how is not known.
                    log::warn!(Label = LABEL; "The agent's launchd job is gone");
                    slot.loaded = false;
                    slot.pid = None;
                    slot.exited = Some(None);
                }
                Some(r) if r.running => {
                    return match r.pid {
                        Some(pid) => {
                            slot.pid = Some(pid);
                            running_or_stopped(pid)
                        }
                        None => Child::None,
                    };
                }
                Some(r) if r.last_exit_code.is_some() || r.last_signal => {
                    match r.last_exit_code {
                        Some(code) => log::warn!(ExitCode = code, Via = "launchd"; "The agent this app started exited"),
                        None => log::warn!(Signal = true, Via = "launchd"; "The agent this app started was ended by a signal"),
                    }
                    slot.loaded = false;
                    slot.pid = None;
                    slot.exited = Some(r.last_exit_code);
                }
                // Loaded, not started yet (`spawn scheduled`, never exited): starting.
                Some(_) => return Child::None,
            }
        }
        match slot.exited {
            Some(code) => Child::Exited { code },
            None => Child::None,
        }
    }

    /// Boots the job out and waits (bounded) until launchd has dropped it.
    fn boot_out(&self) -> Result<(), String> {
        let mut sys = lock(&self.sys);
        bootout(sys.as_mut(), &self.target)?;
        let mut waited = Duration::ZERO;
        loop {
            match job_report(sys.as_mut(), &self.target) {
                Some(r) if !r.found => return Ok(()),
                _ if waited >= GONE_WAIT => {
                    return Err(format!(
                        "launchd still holds the job {LABEL} {} s after it was asked to drop it",
                        GONE_WAIT.as_secs()
                    ))
                }
                _ => {
                    sys.pause(POLL);
                    waited += POLL;
                }
            }
        }
    }
}

impl Launcher for LaunchdLauncher {
    fn launch(&self) -> Result<(), String> {
        // Held across the start: a quit (`close`, `finish`) waits for it and then sees the new job.
        let mut slot = lock(&self.slot);
        if self.closed.load(Ordering::SeqCst) {
            return Err("Dyno is closing.".into());
        }
        if matches!(self.look(&mut slot), Child::Running { .. } | Child::Stopped { .. }) || slot.loaded {
            // The one it loaded runs, or launchd is still starting it: never a second one.
            return Ok(());
        }
        let exe = (self.exe)()?;
        let began = std::time::Instant::now();
        match self.report() {
            None => return Err("launchd did not answer, so the Dyno agent was not started; Dyno tries again.".into()),
            Some(r) if r.found && r.running => {
                // A job of this label that this launcher did not load: an earlier Dyno started it and quit without ending
                // it. It is not ours to end; the link connects to it once it holds the data folder, or starts one after it.
                log::warn!(Label = LABEL, Pid = r.pid.unwrap_or(0); "An agent job of an earlier Dyno still runs; not starting another");
                return Err("An agent an earlier Dyno started still runs; Dyno waits for it.".into());
            }
            // An old job that has exited (or launchd's record of one): dropped first, or the new definition is not loaded.
            Some(r) if r.found => self.boot_out()?,
            Some(_) => {}
        }
        let text = job_plist(LABEL, &exe, self.grace, &self.env);
        let mut sys = lock(&self.sys);
        sys.write(&self.plist, &text).map_err(|e| {
            format!(
                "The Dyno agent could not be started: {} could not be written: {e}",
                self.plist.display()
            )
        })?;
        let plist = self.plist.to_string_lossy().into_owned();
        let out = sys
            .run(LAUNCHCTL, &["bootstrap", &self.domain, &plist])
            .map_err(|e| format!("The Dyno agent ({}) could not be started: launchctl bootstrap {e}", exe.display()))?;
        if !out.success() {
            return Err(format!(
                "The Dyno agent ({}) could not be started: launchctl bootstrap: {}",
                exe.display(),
                out.reason()
            ));
        }
        drop(sys);
        log::info!(
            Via = "launchd",
            Label = LABEL,
            Domain = self.domain.as_str(),
            BootstrapMs = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
            "Agent job loaded"
        );
        slot.loaded = true;
        slot.pid = None;
        slot.exited = None;
        Ok(())
    }

    fn child(&self) -> Child {
        let mut slot = lock(&self.slot);
        self.look(&mut slot)
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    fn finish(&self, wait: Duration) {
        // Held throughout: nothing is loaded while the job this launcher loaded is being ended.
        // cap: the grace the agent gets to exit by itself (after `Shutdown`) before launchd ends it (ADR 0031 class S: a quit
        // must end); `wait` is that grace, passed by the driver (3 s), and the job's `ExitTimeOut` is the same.
        let mut slot = lock(&self.slot);
        if !slot.loaded {
            return;
        }
        let mut waited = Duration::ZERO;
        while waited < wait && matches!(self.look(&mut slot), Child::Running { .. } | Child::Stopped { .. } | Child::None) && slot.loaded {
            lock(&self.sys).pause(POLL);
            waited += POLL;
        }
        // Dropped from launchd whether it exited or not: a running agent is ended by launchd (SIGTERM, then SIGKILL after
        // `ExitTimeOut`), and an exited one leaves no record behind.
        let ended_running = slot.loaded;
        let result = self.boot_out();
        log::info!(
            Via = "launchd",
            Label = LABEL,
            EndedByLaunchd = ended_running,
            WaitedMs = u64::try_from(waited.as_millis()).unwrap_or(u64::MAX),
            Error = result.as_ref().err().map(String::as_str).unwrap_or("");
            "Agent job ended"
        );
        if slot.loaded {
            slot.loaded = false;
            slot.pid = None;
            slot.exited = Some(None);
        }
    }

    fn via(&self) -> &'static str {
        "launchd"
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! A fake launchd for the launcher's tests: it answers `launchctl print`, `bootstrap` and `bootout` of the gui domain from a
    //! small state, over [`paddock_service::testing::FakeSystem`] (rule 7: never the real launchd). Its "process" is the test's
    //! own, so the pid it reports names a live process.

    use super::*;
    use paddock_service::port::Output;
    use paddock_service::testing::FakeSystem;
    use std::sync::Arc;

    /// The uid of the fake's domain.
    pub const UID: u32 = 501;

    /// What the fake launchd holds for the job.
    #[derive(Debug, Default)]
    pub struct Job {
        /// The job is loaded.
        pub loaded: bool,
        /// Its process runs (else it has not started yet, or exited).
        pub running: bool,
        /// It was loaded but launchd has not started it yet (`spawn scheduled`).
        pub scheduled: bool,
        /// How the last process exited.
        pub last_exit: Option<i32>,
        /// A signal ended the last process.
        pub signalled: bool,
        /// How many times it was loaded.
        pub bootstraps: u32,
        /// How many times it was booted out.
        pub bootouts: u32,
        /// The process ignores the request to exit: only a bootout ends it.
        pub stubborn: bool,
        /// launchd does not answer `print` (a timeout).
        pub mute: bool,
    }

    /// The fake launchd and the system that talks to it.
    pub struct Launchd {
        /// The job's state.
        pub job: Arc<Mutex<Job>>,
        /// The recording system.
        pub sys: FakeSystem,
    }

    impl Launchd {
        pub fn new() -> Launchd {
            let job = Arc::new(Mutex::new(Job::default()));
            let sys = FakeSystem::new();
            let target = format!("{LAUNCHCTL} print gui/{UID}/{LABEL}");
            let j = job.clone();
            sys.on(&target, move |_, _| {
                let j = lock(&j);
                if j.mute {
                    return Err(paddock_service::port::RunError::TimedOut("launchctl".into()));
                }
                if !j.loaded {
                    return Ok(Output::code(113));
                }
                let state = if j.running {
                    "running"
                } else if j.scheduled {
                    "spawn scheduled"
                } else {
                    "not running"
                };
                let mut text = format!("gui/{UID}/{LABEL} = {{\n\tstate = {state}\n");
                if j.running {
                    text.push_str(&format!("\tpid = {}\n", std::process::id()));
                }
                match j.last_exit {
                    Some(c) => text.push_str(&format!("\tlast exit code = {c}\n")),
                    None => text.push_str("\tlast exit code = (never exited)\n"),
                }
                if j.signalled {
                    text.push_str("\tlast terminating signal = Terminated: 15\n");
                }
                text.push_str("\tresource coalition = {\n\t\tstate = active\n\t\tpid = 1\n\t}\n}\n");
                Ok(Output::ok(&text))
            });
            let j = job.clone();
            sys.on(&format!("{LAUNCHCTL} bootstrap gui/{UID}"), move |_, _| {
                let mut j = lock(&j);
                if j.loaded {
                    return Ok(Output {
                        code: Some(5),
                        stderr: "Bootstrap failed: 5: Input/output error".into(),
                        ..Output::default()
                    });
                }
                *j = Job {
                    loaded: true,
                    running: !j.scheduled,
                    bootstraps: j.bootstraps + 1,
                    bootouts: j.bootouts,
                    stubborn: j.stubborn,
                    scheduled: j.scheduled,
                    ..Job::default()
                };
                Ok(Output::ok(""))
            });
            let j = job.clone();
            sys.on(&format!("{LAUNCHCTL} bootout gui/{UID}/{LABEL}"), move |_, _| {
                let mut j = lock(&j);
                if !j.loaded {
                    return Ok(Output::code(3));
                }
                j.bootouts += 1;
                j.loaded = false;
                j.running = false;
                Ok(Output::ok(""))
            });
            Launchd { job, sys }
        }

        /// A launcher on this fake, for the agent `exe`.
        pub fn launcher(&self, exe: PathBuf) -> LaunchdLauncher {
            LaunchdLauncher::new(
                move || Ok(exe.clone()),
                PathBuf::from("/data/Dyno").join(PLIST_FILE),
                UID,
                Duration::from_secs(3),
                Vec::new(),
                Box::new(self.sys.clone()),
            )
        }

        /// The agent's process exits with `code` (`None`: a signal ends it).
        pub fn exit(&self, code: Option<i32>) {
            let mut j = lock(&self.job);
            j.running = false;
            j.scheduled = false;
            j.last_exit = code;
            j.signalled = code.is_none();
        }

        /// launchd starts the scheduled job now.
        pub fn start(&self) {
            let mut j = lock(&self.job);
            j.scheduled = false;
            j.running = j.loaded;
        }

        /// How many times the job was loaded.
        pub fn bootstraps(&self) -> u32 {
            lock(&self.job).bootstraps
        }

        /// How many times it was booted out.
        pub fn bootouts(&self) -> u32 {
            lock(&self.job).bootouts
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{Launchd, UID};
    use super::*;

    fn agent() -> PathBuf {
        PathBuf::from("/Applications/Dyno.app/Contents/MacOS/dyno-agent")
    }

    /// The bug (Dyno 1.0.0 on macOS 27.2, ADR 0026 amendment): the app's own agent was its child and saw only its own sockets.
    /// It is now a launchd job of the gui domain, loaded from a definition in the data folder, never from LaunchAgents.
    #[test]
    fn the_agent_is_loaded_as_a_launchd_job_of_the_users_gui_domain() {
        let fake = Launchd::new();
        let l = fake.launcher(agent());
        assert_eq!(l.child(), Child::None);
        l.launch().unwrap();
        let plist = PathBuf::from("/data/Dyno").join(PLIST_FILE);
        let cmds = fake.sys.commands();
        assert!(
            cmds.iter()
                .any(|c| c == &format!("/bin/launchctl bootstrap gui/{UID} {}", plist.display())),
            "{cmds:?}"
        );
        let text = fake.sys.file(&plist).expect("the job definition is written");
        assert_eq!(text, job_plist(LABEL, &agent(), Duration::from_secs(3), &[]));
        assert_eq!(l.child(), Child::Running { pid: std::process::id() });
        assert_eq!(l.via(), "launchd");
    }

    #[test]
    fn the_job_definition_starts_the_agent_once_and_is_not_the_login_services() {
        let text = job_plist(
            LABEL,
            Path::new("/A & B/<Dyno>.app/Contents/MacOS/dyno-agent"),
            Duration::from_secs(3),
            &[],
        );
        assert!(
            text.contains("<string>/A &amp; B/&lt;Dyno&gt;.app/Contents/MacOS/dyno-agent</string>\n\t\t<string>run</string>"),
            "{text}"
        );
        assert!(
            text.contains("<key>RunAtLoad</key>\n\t<true/>") && text.contains("<key>KeepAlive</key>\n\t<false/>"),
            "{text}"
        );
        assert!(text.contains("<key>ExitTimeOut</key>\n\t<integer>3</integer>"), "{text}");
        // Not attributed to the app, and not the login service's label (whose probe, repair and hand-over must never see it).
        assert!(
            !text.contains("AssociatedBundleIdentifiers") && !text.contains("BundleProgram"),
            "{text}"
        );
        assert_ne!(LABEL, dyno_agent::service::LABEL);
        assert_eq!(
            job_plist(LABEL, Path::new("/x"), Duration::ZERO, &[])
                .matches("<integer>1</integer>")
                .count(),
            1,
            "at least 1 s"
        );
    }

    /// A launchd job does not inherit the app's environment: the data folder the app uses (moved by `DYNO_DATA_DIR`) is passed, so
    /// the agent never opens another folder than the app's; nothing else is.
    #[test]
    fn the_agent_gets_the_apps_data_folder_and_nothing_else_of_its_environment() {
        assert!(!job_plist(LABEL, Path::new("/x"), Duration::from_secs(3), &[]).contains("EnvironmentVariables"));
        let env = agent_env(|k| match k {
            "DYNO_DATA_DIR" => Some("/tmp/dyno <test> & data".into()),
            _ => Some("leak".into()),
        });
        assert_eq!(env, [("DYNO_DATA_DIR".to_string(), "/tmp/dyno <test> & data".to_string())]);
        let text = job_plist(LABEL, Path::new("/x"), Duration::from_secs(3), &env);
        assert!(
            text.contains(
                "<key>EnvironmentVariables</key>\n\t<dict>\n\t\t<key>DYNO_DATA_DIR</key>\n\t\t<string>/tmp/dyno &lt;test&gt; &amp; data</string>\n\t</dict>"
            ),
            "{text}"
        );
        assert!(agent_env(|_| None).is_empty() && agent_env(|_| Some(String::new())).is_empty());
    }

    /// launchd may not have started the job when `bootstrap` returns: the launcher says "none yet", and a second launch in that
    /// moment loads nothing more.
    #[test]
    fn a_job_launchd_has_not_started_yet_is_starting_not_missing() {
        let fake = Launchd::new();
        lock(&fake.job).scheduled = true;
        let l = fake.launcher(agent());
        l.launch().unwrap();
        assert_eq!(l.child(), Child::None);
        l.launch().unwrap();
        assert_eq!(fake.bootstraps(), 1, "a second job was loaded while launchd was starting the first");
        fake.start();
        assert_eq!(l.child(), Child::Running { pid: std::process::id() });
    }

    #[test]
    fn a_signal_or_a_job_dropped_by_someone_else_is_an_exit_without_a_code() {
        let fake = Launchd::new();
        let l = fake.launcher(agent());
        l.launch().unwrap();
        fake.exit(None);
        assert_eq!(l.child(), Child::Exited { code: None });
        l.launch().unwrap();
        assert_eq!(
            fake.bootouts(),
            1,
            "the record of the exited job is dropped before the new one is loaded"
        );
        lock(&fake.job).loaded = false; // `launchctl bootout` by someone else
        assert_eq!(l.child(), Child::Exited { code: None });
    }

    /// An earlier Dyno quit (or crashed) while its agent job ran: that agent is not this launcher's to end, so no second one is
    /// loaded beside it and it is not booted out; the link connects to it once it holds the data folder.
    #[test]
    fn a_running_job_an_earlier_app_loaded_is_left_alone() {
        let fake = Launchd::new();
        {
            let mut j = lock(&fake.job);
            j.loaded = true;
            j.running = true;
        }
        let l = fake.launcher(agent());
        let e = l.launch().unwrap_err();
        assert!(e.contains("earlier Dyno"), "{e}");
        assert_eq!((fake.bootstraps(), fake.bootouts()), (0, 0));
        assert_eq!(l.child(), Child::None, "not this launcher's");
        l.finish(Duration::from_secs(3));
        assert_eq!(fake.bootouts(), 0, "a quit never ends a job this launcher did not load");
    }

    #[test]
    fn launchd_that_does_not_answer_fails_the_launch_and_keeps_what_was_known() {
        let fake = Launchd::new();
        let l = fake.launcher(agent());
        lock(&fake.job).mute = true;
        assert!(l.launch().unwrap_err().contains("launchd did not answer"));
        assert_eq!(fake.bootstraps(), 0);
        lock(&fake.job).mute = false;
        l.launch().unwrap();
        assert!(matches!(l.child(), Child::Running { .. }));
        lock(&fake.job).mute = true;
        assert_eq!(
            l.child(),
            Child::Running { pid: std::process::id() },
            "the last pid launchd reported"
        );
        l.launch().unwrap();
        assert_eq!(fake.bootstraps(), 1, "never a second job while the first may run");
    }

    #[test]
    fn a_failed_bootstrap_or_write_is_a_launch_error_with_launchds_words() {
        let fake = Launchd::new();
        fake.sys.answer(
            "/bin/launchctl bootstrap",
            paddock_service::port::Output {
                code: Some(5),
                stderr: "Bootstrap failed: 5: Input/output error".into(),
                ..Default::default()
            },
        );
        let l = fake.launcher(agent());
        let e = l.launch().unwrap_err();
        assert!(e.contains("Bootstrap failed: 5"), "{e}");
        assert_eq!(l.child(), Child::None);
        let fake = Launchd::new();
        fake.sys.fail_writes(Some(std::io::ErrorKind::PermissionDenied));
        let e = fake.launcher(agent()).launch().unwrap_err();
        assert!(e.contains("could not be written"), "{e}");
        assert_eq!(fake.bootstraps(), 0);
    }

    /// The quit after `Shutdown`: an agent that exits within the grace leaves its code and no record in launchd; one that does not
    /// is ended by launchd (bootout), never by a signal to a pid.
    #[test]
    fn finish_waits_for_the_exit_then_drops_the_job_and_ends_one_that_stays() {
        let fake = Launchd::new();
        let l = fake.launcher(agent());
        l.launch().unwrap();
        fake.exit(Some(0));
        l.finish(Duration::from_secs(3));
        assert_eq!(l.child(), Child::Exited { code: Some(0) });
        assert_eq!(fake.bootouts(), 1);
        assert!(!lock(&fake.job).loaded, "no record left behind");

        let fake = Launchd::new();
        lock(&fake.job).stubborn = true;
        let l = fake.launcher(agent());
        l.launch().unwrap();
        let before = fake.sys.now();
        l.finish(Duration::from_secs(3));
        assert!(fake.sys.now() - before >= Duration::from_secs(3), "it got its grace first");
        assert_eq!(fake.bootouts(), 1);
        assert_eq!(l.child(), Child::Exited { code: None });
        assert!(
            !fake.sys.commands().iter().any(|c| c.contains("kill")),
            "ended through launchd, never a kill: {:?}",
            fake.sys.commands()
        );
        // Nothing loaded: nothing to end.
        let fake = Launchd::new();
        fake.launcher(agent()).finish(Duration::from_secs(3));
        assert!(fake.sys.commands().is_empty());
    }
}
