//! A fake systemd user manager for tests (this crate's model and the apps' wiring tests): the verbs `Systemd` uses
//! (`daemon-reload`, `enable`, `disable`, `stop`, `start`, `is-active`, `is-enabled`, `show`) answer from one small
//! state, and what the user or the OS does to a unit (`systemctl --user disable`, a stop, a crash, a restart loop, a
//! logout with or without lingering) is a method. It sits on [`FakeSystem`], so one log shows every command and nothing
//! touches the real systemd (ER-14).

use crate::port::Output;
use crate::testing::FakeSystem;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// What `systemctl is-active` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Active {
    /// Running.
    Active,
    /// Not running, not wanted to be.
    Inactive,
    /// Waiting for the automatic restart (`RestartSec`), or starting.
    Activating,
    /// Gave up (start limit) or failed without a restart.
    Failed,
}

impl Active {
    fn word(self) -> &'static str {
        match self {
            Active::Active => "active",
            Active::Inactive => "inactive",
            Active::Activating => "activating",
            Active::Failed => "failed",
        }
    }
}

#[derive(Debug)]
struct World {
    unit: String,
    fragment: PathBuf,
    /// The unit has been enabled or started once (systemd knows it).
    known: bool,
    enabled: bool,
    active: Active,
    restarts: u32,
    /// The unit's last main process ended with this exit status (code 1) and `Result=exit-code`.
    last_failure: Option<i32>,
    session: bool,
    linger: bool,
    /// The unit never starts (`enable --now` enables it, the start fails and it is restarted).
    start_loops: bool,
}

impl World {
    fn bus_up(&self) -> bool {
        self.session || self.linger
    }
}

/// A systemd user manager answering `systemctl --user ...` for one unit.
#[derive(Clone)]
pub struct FakeSystemd {
    w: Arc<Mutex<World>>,
}

const NO_BUS: &str = "Failed to connect to bus: No medium found\n";

impl FakeSystemd {
    /// The manager of `unit` (`dyno-agent.service`) whose file is `fragment`, answering on `sys`. A user is logged in.
    pub fn new(sys: &FakeSystem, unit: &str, fragment: &Path) -> Self {
        let this = FakeSystemd {
            w: Arc::new(Mutex::new(World {
                unit: unit.to_string(),
                fragment: fragment.to_path_buf(),
                known: false,
                enabled: false,
                active: Active::Inactive,
                restarts: 0,
                last_failure: None,
                session: true,
                linger: false,
                                start_loops: false,
            })),
        };
        let handle = this.clone();
        sys.on("systemctl", move |words, _| Ok(handle.answer(words)));
        this
    }

    fn lock(&self) -> MutexGuard<'_, World> {
        self.w.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn answer(&self, words: &[String]) -> Output {
        let mut w = self.lock();
        let args: Vec<&str> = words.iter().skip(1).map(String::as_str).collect();
        if !w.bus_up() {
            return Output {
                code: Some(1),
                stdout: String::new(),
                stderr: NO_BUS.to_string(),
            };
        }
        let Some((&"--user", rest)) = args.split_first() else {
            return Output::code(1);
        };
        let verb = rest.first().copied().unwrap_or("");
        let now = rest.contains(&"--now");
        let names_unit = rest.iter().any(|a| *a == w.unit);
        match verb {
            "daemon-reload" => Output::code(0),
            "enable" => {
                if !names_unit {
                    return Output::code(1);
                }
                w.known = true;
                w.enabled = true;
                if now {
                    Self::start(&mut w);
                }
                Output::code(0)
            }
            "start" => {
                w.known = true;
                Self::start(&mut w);
                Output::code(0)
            }
            "disable" => {
                if !w.known {
                    return Output {
                        code: Some(1),
                        stdout: String::new(),
                        stderr: format!(
                            "Failed to disable unit: Unit file {} does not exist.\n",
                            w.unit
                        ),
                    };
                }
                w.enabled = false;
                if now {
                    w.active = Active::Inactive;
                }
                Output::code(0)
            }
            "stop" => {
                if w.known {
                    w.active = Active::Inactive;
                    Output::code(0)
                } else {
                    Output {
                        code: Some(5),
                        stdout: String::new(),
                        stderr: format!("Failed to stop {}: Unit {} not loaded.\n", w.unit, w.unit),
                    }
                }
            }
            "is-active" => Output {
                code: Some(if w.active == Active::Active { 0 } else { 3 }),
                stdout: format!("{}\n", if w.known { w.active.word() } else { "inactive" }),
                stderr: String::new(),
            },
            "is-enabled" => {
                let (code, word) = match (w.known, w.enabled) {
                    (true, true) => (0, "enabled"),
                    (true, false) => (1, "disabled"),
                    (false, _) => (1, "disabled"),
                };
                Output {
                    code: Some(code),
                    stdout: format!("{word}\n"),
                    stderr: String::new(),
                }
            }
            "show" => {
                let mut text = String::new();
                if w.known {
                    text += &format!("FragmentPath={}\n", w.fragment.display());
                    text += &format!(
                        "UnitFileState={}\n",
                        if w.enabled { "enabled" } else { "disabled" }
                    );
                }
                text += &format!("NRestarts={}\n", w.restarts);
                match w.last_failure {
                    Some(status) => {
                        text +=
                            &format!("Result=exit-code\nExecMainStatus={status}\nExecMainCode=1\n");
                    }
                    None => text += "Result=success\nExecMainStatus=0\nExecMainCode=0\n",
                }
                Output::ok(&text)
            }
            _ => Output::code(1),
        }
    }

    fn start(w: &mut World) {
        if w.start_loops {
            w.active = Active::Activating;
            w.last_failure = Some(1);
        } else {
            w.active = Active::Active;
        }
    }

    /// The unit's state now.
    pub fn active(&self) -> Active {
        self.lock().active
    }

    /// Whether the unit is enabled (it starts with the user's manager).
    pub fn enabled(&self) -> bool {
        self.lock().enabled
    }

    /// How often the unit was restarted after it exited.
    pub fn restarts(&self) -> u32 {
        self.lock().restarts
    }

    /// The user ran `systemctl --user disable` (no `--now`): the unit keeps running and will not start at the next login.
    pub fn user_disables(&self) {
        self.lock().enabled = false;
    }

    /// The user ran `systemctl --user enable` (no `--now`) on a unit systemd knows.
    pub fn user_enables(&self) {
        let mut w = self.lock();
        if w.known {
            w.enabled = true;
        }
    }

    /// Whether `systemctl --user` can reach a manager (somebody is logged in, or lingering keeps one running).
    pub fn bus_up(&self) -> bool {
        self.lock().bus_up()
    }

    /// The user ran `systemctl --user stop`: the unit stays enabled.
    pub fn user_stops(&self) {
        self.lock().active = Active::Inactive;
    }

    /// The main process exited with status 1 and systemd will restart it after `RestartSec`.
    pub fn crashes(&self) {
        let mut w = self.lock();
        w.active = Active::Activating;
        w.restarts += 1;
        w.last_failure = Some(1);
    }

    /// The automatic restart came and the process runs again.
    pub fn restarted(&self) {
        let mut w = self.lock();
        if w.active == Active::Activating {
            w.active = Active::Active;
        }
    }

    /// A restart loop: the process exits at once every time, so the unit is never `active` for long; `rounds` restarts
    /// are counted, and `gave_up` says systemd stopped trying (start limit: `failed`) instead of waiting for the next one.
    pub fn restart_loop(&self, rounds: u32, gave_up: bool) {
        let mut w = self.lock();
        w.restarts += rounds;
        w.last_failure = Some(1);
        w.active = if gave_up {
            Active::Failed
        } else {
            Active::Activating
        };
    }

    /// From now on `start` and `enable --now` find a process that exits at once (a restart loop from the first second).
    pub fn starts_into_a_loop(&self, on: bool) {
        self.lock().start_loops = on;
    }

    /// Whether lingering is on (`loginctl enable-linger`): the user's manager keeps running after the last logout.
    pub fn set_linger(&self, on: bool) {
        self.lock().linger = on;
    }

    /// The user logged out: without lingering the user's manager exits, every unit of it stops and `systemctl --user` has
    /// no bus to talk to; the unit file and the enabled state stay.
    pub fn logout(&self) {
        let mut w = self.lock();
        w.session = false;
        if !w.linger {
            w.active = Active::Inactive;
        }
    }

    /// The user logged in: the manager starts the units that are enabled.
    pub fn login(&self) {
        let mut w = self.lock();
        w.session = true;
        if w.known && w.enabled && w.active == Active::Inactive {
            Self::start(&mut w);
        }
    }

    /// The unit's file was removed behind systemd's back (`systemctl` still knows the unit until the next reload).
    pub fn forgets(&self) {
        let mut w = self.lock();
        w.known = false;
        w.enabled = false;
        w.active = Active::Inactive;
    }
}
