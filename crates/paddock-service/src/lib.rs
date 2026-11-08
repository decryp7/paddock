//! Background services and login items for the Paddock apps (plan X4, decision D4, rule 24, catalogue R03/R04/R05).
//!
//! One [`ServiceSpec`] declares the app's background program; everything else follows from it:
//!
//! - [`render`]: the OS files, pure and golden-tested: the launchd plist (the bundled one an app ships for
//!   `SMAppService`, or a file), the systemd unit, the Windows `Run` value and service (`sc.exe` calls) and the NSIS
//!   installer hooks. Apps generate their `src-tauri/macos/<label>.plist` and `src-tauri/windows/installer-hooks.nsh`
//!   from the spec (check them in a test with `paddock_commands::testing::sync_generated`) instead of writing them by
//!   hand.
//! - [`backend`]: one [`Backend`] per mechanism (`SMAppService`, plist file + `launchctl`, systemd user or system unit,
//!   `Run` value, Service Control Manager directly or through Windows' prompt), all over the [`System`] port, so
//!   nothing in the tests touches the real launchd, Login Items, systemd, registry or services (rule 7): the
//!   [`testing`] fakes record every command and file write.
//! - [`plan`] and [`repair`]: rule 24 as a pure decision and one bounded sequence: unregister → `launchctl bootout` →
//!   wait until the job is gone → register → check, retried a bounded number of times; once per version change where
//!   the OS pins the build, and automatically when a stale registration keeps the service down; the user's switch is
//!   never overridden; two installed copies never take the registration from each other.
//! - [`Manager`]: what an app holds: [`Manager::status`], [`Manager::on_start`] (rule 24), [`Manager::repair`],
//!   [`Manager::install`], [`Manager::uninstall`], the registered version in the data folder ([`record`]) and the
//!   phase-labelled progress ([`RepairTracker`]). [`ServiceStatus::health`] is the entry for `paddock-health`, whose
//!   Repair never depends on the service (rule 19).
//! - [`login`]: the launch-at-login settings section and switch (the app's login item and the service together).
//! - [`watch`] and [`supervised`]: the program's side: restart itself when its executable changed and it is idle, and
//!   take the login shell's `PATH` (`paddock_proc::login_path`) when the OS started it.
//! - [`scm_host`]: the Windows service host (`--service`): runs the program in the logged-on user's session, restarts
//!   it, moves it with the session and stops it gracefully ([`service_main`] is the one entry point and the Windows dispatcher; `native::host_program` runs this host under it).
//! - [`verbs`]: the words every background program understands (`install`, `uninstall`, `status`, `stop`,
//!   `print-unit`, `print-plist`, `--service`).
//!
//! ```
//! use paddock_service::{ServiceSpec, ProcessType, render};
//!
//! let spec = ServiceSpec::builder("net.decryptology.pitwall.service", "Pitwall", "pitwall-server")
//!     .bundle_id("net.decryptology.pitwall")
//!     .display_name("Pitwall local LLM server")
//!     .supervised_env("PITWALL_SERVICE")
//!     .process_type(ProcessType::Interactive)
//!     .build()
//!     .unwrap();
//! let plist = render::launchd_plist(&spec, render::LaunchdProgram::Bundled);
//! assert!(plist.contains("<string>Contents/MacOS/pitwall-server</string>"));
//! ```
//!
//! In the app (on a background thread, rule 10):
//!
//! ```no_run
//! # use paddock_service::*;
//! # fn spec() -> ServiceSpec { unimplemented!() }
//! # let (program, data_dir) = (std::path::PathBuf::new(), std::path::PathBuf::new());
//! let backend = native::host_backend(&spec(), &program, backend::Side::App, &data_dir)?;
//! let mut service = Manager::new("Pitwall", backend, &data_dir, spec().label(), env!("CARGO_PKG_VERSION"), || {
//!     /* ask the service, bounded */ true
//! });
//! let started = service.on_start(&std::sync::atomic::AtomicBool::new(false));
//! # Ok::<(), ServiceError>(())
//! ```

#[cfg(target_os = "macos")]
pub mod apple_event;
pub mod backend;
pub mod control;
pub mod elevated;
mod error;
pub mod fake_systemd;
pub mod hardening;
pub mod launch;
pub mod launchctl;
pub mod login;
pub mod manager;
pub mod native;
pub mod os_log;
pub mod parse;
pub mod paths;
pub mod per_machine;
pub mod plan;
pub mod port;
pub mod privileged;
pub mod record;
pub mod render;
pub mod repair;
pub mod scm_host;
pub mod service_main;
pub mod spec;
pub mod start;
pub mod start_wait;
pub mod status;
pub mod stop;
pub mod supervised;
pub mod system;
pub mod testing;
pub mod tracker;
pub mod verbs;
pub mod watch;

/// The event every service update is sent on, to every window (a [`ServiceUpdate`]).
pub const EVENT_NAME: &str = "paddock://service";

paddock_commands::command_table! {
    /// The commands of the service adapter (`paddock_service::tauri::invoke_handler` serves them; the shell plugin folds
    /// them into its table). Every window may look; main windows and the Settings dialog change it.
    pub static COMMANDS;
    /// The background service now.
    Query [WINDOW] => tauri::service_status,
    /// Repairs the registration (returns at once; the outcome is an event).
    Mutation [MAIN | DIALOG] => tauri::service_repair,
    /// Registers the service with the user's consent (returns at once; the outcome is an event).
    Mutation [MAIN | DIALOG] => tauri::service_install,
    /// Opens macOS Login Items, where the user switches the background item.
    Mutation [MAIN | DIALOG] => tauri::open_login_items,
}

#[cfg(feature = "tauri")]
pub mod tauri;

#[cfg(test)]
mod generated;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod readback_tests;
#[cfg(test)]
mod recorded_tests;
#[cfg(test)]
mod resume_tests;
#[cfg(test)]
mod scenarios;

pub use backend::{
    Backend, Capabilities, DisabledMark, Inspection, Job, LastExit, LoginItemFacts, Mechanism,
    Registration, StartFacts, Target, UnitFile, UnitFileState, UnitOrigin, UnregisterError,
};
pub use control::{Request, ServiceCommandError, ServiceControl, ServiceErrorCode, ServiceUpdate};
pub use error::{ServiceError, SpecError};
pub use manager::{Manager, Started};
pub use os_log::OsLog;
pub use plan::{Hold, Plan, Reason, Refusal, ResumePath};
pub use port::{AppService, AppServiceStatus, Elevator, Output, RegisterError, RunError, System};
pub use repair::{RepairOutcome, Step, Timings};
pub use service_main::{Context, Hosting, Outcome, ServiceExit, ServiceMainError, service_main};
pub use spec::{KillMode, Kind, ProcessType, Restart, ServiceSpec, SpecBuilder, WindowsStart};
pub use start_wait::{StartObservation, StartVerdict, StartWait, StartWatch};
pub use status::{Condition, HealthReport, LOGIN_SECTION, ServiceStatus};
pub use system::RealSystem;
pub use tracker::{RepairNotice, RepairState, RepairTracker};
pub use verbs::Verb;
