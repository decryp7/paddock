//! Installing, updating and removing a privileged system service (ADR 0027 §1 and §4): the helper of an app that needs
//! rights the user's session lacks. Everything is a pure plan plus a thin adapter over ports, so nothing in the tests
//! installs a service (ER-14):
//!
//! - [`layout`]: where each OS keeps the protected copy, the registration and the folder (pure).
//! - [`elevate`]: the one OS prompt per OS (`osascript` "with administrator privileges", Windows' `runas`, `pkexec`),
//!   its command lines and what its exit codes mean (pure; run through [`System`](crate::System) and
//!   [`Elevator`](crate::Elevator)).
//! - [`files`]: the [`ProtectedFiles`] port (facts, create, copy by one handle with a hash, remove), its real adapter
//!   and the ownership and mode rules ([`files::verify_protected`]).
//! - [`install`]: the plan ([`install::plan_install`], [`install::plan_uninstall`]) and [`Installer`], which runs it over
//!   the files and a [`Backend`](crate::Backend), logging every phase with its duration (ER-27).
//! - [`assess`]: what the app does about an installed helper (D5: ask only when the protocol or the minimum version
//!   changed).
//! - [`windows_acl`]: the `icacls` calls that protect the Program Files folder and the check of their result.

pub mod assess;
pub mod elevate;
pub mod files;
pub mod install;
pub mod layout;
pub mod windows_acl;

pub use assess::{Assessment, BundledHelper, InstalledHelper, UpdateReason, assess};
pub use elevate::{ElevatedCommand, ElevationError, run_elevated};
pub use files::{FileFacts, FileKind, ProtectedFiles, RealFiles, Violation, verify_protected};
pub use install::{
    InstallError, Installer, Phase, PhaseRecord, Report, Step, Waits, plan_install, plan_uninstall,
    plan_uninstall_self,
};
pub use layout::{CopyTarget, Layout, Roots, backend_for, layout};
