//! Dyno's helper as a system service (ADR 0027 §1 and §4, WP-2). Two sides use it:
//!
//! - The **agent** (unprivileged) turns the helper on, updates it and turns it off: [`turn_on`], [`turn_off`] run the
//!   bundled helper's own `install` and `uninstall` verbs through the OS's one prompt ([`prompt_notice`] says first what
//!   the prompt will look like), and [`assess`] decides from the helper's answer whether to offer anything (D5: only a
//!   protocol or minimum-version change asks).
//! - The **helper** (elevated by that prompt) runs [`install_in_place`] and [`uninstall_in_place`]: the protected copy,
//!   the registration and their checks, all in `paddock-service`.
//!
//! The agent reaches the running helper through [`client::connect`] (the trust check of `paddock-ipc`'s system endpoint).
//!
//! Registration goes through `paddock-service` (rule 55): this crate only says what Dyno's helper is ([`spec`]) and
//! where Dyno keeps it ([`windows_roots`], [`install_roots`]).

use dyno_core::helper_paths::WindowsRoots;
use dyno_core::helper_wire::{HelperUser, HELPER_API};
use paddock_env::Os;
use paddock_service::hardening::{Hardening, Sandbox};
use paddock_service::privileged::trust::TrustPort;
use paddock_service::privileged::{
    assess as assess_installed, backend_for, layout, run_elevated, Assessment, BundledHelper, ElevationError, InstallError,
    InstalledHelper, Installer, KnownFolders, Layout, ProtectedFiles, Report, Roots, SignatureCheck, SourceVerifier,
};
use paddock_service::spec::{Kind, Restart, ServiceSpec, WindowsStart};
use paddock_service::{Elevator, ServiceError, System};
use std::path::{Path, PathBuf};

/// The launchd label (also the protected file's name on macOS).
pub const LABEL: &str = "net.decryptology.dyno.helper";
/// The Windows service name.
pub const SERVICE_NAME: &str = "DynoHelper";
/// The program's file name without `.exe`, and the systemd unit's name.
pub const PROGRAM: &str = "dyno-helper";
/// The folder under Program Files (Windows).
pub const WINDOWS_FOLDER: &str = "Dyno Helper";
/// The oldest installed helper this Dyno accepts (ADR 0027 §4: raised only when a helper fix must reach users). WP-3
/// moves it next to `HELPER_API` in `dyno_core::helper_wire`.
pub const MIN_HELPER_VERSION: &str = "1.1.0";
/// The verbs of the helper program that this crate runs elevated.
pub const INSTALL_VERB: &str = "install";
/// The flag that carries the owner to `install`.
pub const OWNER_FLAG: &str = "--for";
/// See [`INSTALL_VERB`].
pub const UNINSTALL_VERB: &str = "uninstall";

/// What the helper is: a system service that starts with the OS, restarts after a failure, runs with the privileges the
/// ADR measured (Windows: `SeChangeNotifyPrivilege` only, and interactive users may start it) and the systemd sandbox of
/// ADR 0027 §1 (no network, only `/proc`, root-only files, SMART pass-through and `nft`).
///
/// # Panics
/// Never: the constants above are valid (a test builds it).
pub fn spec() -> ServiceSpec {
    let hardening = Hardening::new()
        .windows_privileges(["SeChangeNotifyPrivilege"])
        .users_may_start()
        .sandbox(
            Sandbox::new()
                .capabilities([
                    "CAP_SYS_PTRACE",
                    "CAP_DAC_READ_SEARCH",
                    "CAP_SYS_RAWIO",
                    "CAP_SYS_ADMIN",
                    "CAP_NET_ADMIN",
                ])
                .address_families(["AF_UNIX", "AF_NETLINK"])
                .state_dir("dyno-helper")
                .runtime_dir("dyno-helper"),
        );
    ServiceSpec::builder(LABEL, SERVICE_NAME, PROGRAM)
        .unit(PROGRAM)
        .display_name("Dyno helper")
        .description("Answers Dyno's read-only questions that need administrator rights. It changes nothing on this computer.")
        .args(["serve"])
        .kind(Kind::SystemService)
        .restart(Restart::OnFailure)
        .docs_url("https://dev.decryptology.net/decryp7/dyno")
        .hardening(hardening)
        // Not delayed: a delayed start leaves the helper down for a minute or two after every boot (the card said Repair).
        .windows_start(WindowsStart::Auto)
        .build()
        .unwrap_or_else(|e| unreachable!("the helper's constant spec is valid: {e}"))
}

/// The folders Windows names for the helper's places: Program Files, ProgramData and the Windows folder, as the OS reports
/// them (`SHGetKnownFolderPath`, `GetSystemDirectoryW`), never from the environment: the elevated install may inherit a
/// user-writable `HKCU\Environment` (the `windir` UAC bypass). Where the OS gives no answer the usual folder is used and the
/// failure is logged; the install itself uses [`install_roots`], which refuses instead.
pub fn windows_roots(folders: &dyn KnownFolders) -> WindowsRoots {
    let or_default = |what: &str, found: std::io::Result<PathBuf>, default: &str| {
        found.unwrap_or_else(|e| {
            log::warn!(folder = what, error:% = e; "the OS did not name the folder, using the usual one");
            PathBuf::from(default)
        })
    };
    let system = or_default("system directory", folders.system_directory(), r"C:\Windows\System32");
    WindowsRoots {
        program_files: or_default("Program Files", folders.program_files(), r"C:\Program Files"),
        program_data: or_default("ProgramData", folders.program_data(), r"C:\ProgramData"),
        // Split as text: a Windows path is not split by a Unix host's separators.
        system_root: system
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .rsplit_once(['\\', '/'])
            .map_or_else(|| PathBuf::from(r"C:\Windows"), |(root, _)| PathBuf::from(root)),
    }
}

/// The roots the protected install builds its folder under.
///
/// # Errors
/// [`ServiceError::Unavailable`] when the OS does not name Program Files.
pub fn install_roots(folders: &dyn KnownFolders) -> Result<Roots, ServiceError> {
    Roots::from_known(folders)
}

/// Where Dyno's helper lives on `os`.
///
/// # Errors
/// Never for the constant spec; the error type is the layout's.
pub fn helper_layout(os: Os, roots: &Roots) -> Result<Layout, ServiceError> {
    layout(os, &spec(), WINDOWS_FOLDER, roots)
}

/// What to tell the user before the OS prompt (ADR 0027 R1: the prompt does not always name Dyno).
pub fn prompt_notice(os: Os) -> &'static str {
    match os {
        Os::MacOs => "macOS will ask for your password for osascript, on behalf of Dyno. It installs Dyno's helper once.",
        Os::Windows => {
            "Windows will ask you to allow dyno-helper.exe (publisher: Unknown) to make changes. It installs Dyno's helper once."
        }
        Os::Linux => "Your desktop will ask you to authenticate to run /usr/bin/dyno-helper. It starts Dyno's helper once.",
    }
}

/// The dialog text of the macOS password prompt itself (turn on).
pub const MACOS_PROMPT: &str = "Dyno wants to install its helper, which shows the readings that need administrator rights.";

/// The dialog text of the macOS password prompt for `action`: it says what this change does, so a repair does not look like an install.
pub fn macos_prompt(action: dyno_core::helper_wire::HelperAction) -> &'static str {
    use dyno_core::helper_wire::HelperAction as A;
    match action {
        A::TurnOn => MACOS_PROMPT,
        A::Repair => "Dyno wants to repair its helper, which shows the readings that need administrator rights.",
        A::Update => "Dyno wants to update its helper, which shows the readings that need administrator rights.",
        A::TurnOff => "Dyno wants to remove its helper from this computer.",
    }
}

/// Turns the helper on or updates it: runs `<bundled helper> install` through the OS's one prompt and waits. `bundled`
/// is the helper next to the agent (macOS and Windows: the program that is copied; Linux: `/usr/bin/dyno-helper`).
///
/// # Errors
/// What the user did or what went wrong.
pub fn turn_on(
    os: Os,
    bundled: &Path,
    owner: &HelperUser,
    sys: &mut dyn System,
    elevator: &mut dyn Elevator,
) -> Result<(), ElevationError> {
    turn_on_with(os, bundled, owner, sys, elevator, MACOS_PROMPT)
}

/// [`turn_on`] with the macOS dialog's text given ([`macos_prompt`]).
///
/// # Errors
/// What the user did or what went wrong.
pub fn turn_on_with(
    os: Os,
    bundled: &Path,
    owner: &HelperUser,
    sys: &mut dyn System,
    elevator: &mut dyn Elevator,
    prompt: &str,
) -> Result<(), ElevationError> {
    log::info!(os:? = os; "turning the helper on");
    let owner = owner.to_arg();
    run_elevated(os, sys, elevator, bundled, &[INSTALL_VERB, OWNER_FLAG, &owner], prompt)
}

/// Turns the helper off when it does not answer the authenticated `Uninstall` request: runs the installed helper's
/// `uninstall` through the one prompt.
///
/// # Errors
/// What the user did or what went wrong.
pub fn turn_off(os: Os, installed: &Path, sys: &mut dyn System, elevator: &mut dyn Elevator) -> Result<(), ElevationError> {
    turn_off_with(
        os,
        installed,
        sys,
        elevator,
        macos_prompt(dyno_core::helper_wire::HelperAction::TurnOff),
    )
}

/// [`turn_off`] with the macOS dialog's text given.
///
/// # Errors
/// What the user did or what went wrong.
pub fn turn_off_with(
    os: Os,
    installed: &Path,
    sys: &mut dyn System,
    elevator: &mut dyn Elevator,
    prompt: &str,
) -> Result<(), ElevationError> {
    log::info!(os:? = os; "turning the helper off");
    run_elevated(os, sys, elevator, installed, &[UNINSTALL_VERB], prompt)
}

/// What the agent does about the helper that answered `answer` (`(version, protocol)`), or about none.
pub fn assess(answer: Option<(&str, u32)>) -> Assessment {
    let installed = answer.map(|(version, protocol)| InstalledHelper {
        version: version.to_string(),
        protocol,
    });
    let assessment = assess_installed(
        installed.as_ref(),
        &BundledHelper {
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol: HELPER_API,
            min_version: MIN_HELPER_VERSION.to_string(),
        },
    );
    log::info!(assessment:? = assessment; "the helper was assessed");
    assessment
}

/// The elevated side of "turn on" and "update": copies `source` (the running helper) into the protected place and
/// registers it. Idempotent: run again it takes over an older registration (a 0.x helper under the same label).
/// `make_system` builds the OS (one for the registration, one for `icacls`).
///
/// # Errors
/// An [`InstallError`] (also when the layout cannot be built).
///
/// Windows: `trust` judges Program Files, the helper's folder and the copy, and the source must sit in a folder only
/// administrators can change. macOS: `verifier` must accept the bytes of `source` against the signature next to it
/// ([`source_check::signature_locations`]). Both fail closed (security review F1, F2).
pub fn install_in_place(
    os: Os,
    roots: &Roots,
    source: &Path,
    files: &mut dyn ProtectedFiles,
    trust: &mut dyn TrustPort,
    verifier: &dyn SourceVerifier,
    make_system: &dyn Fn() -> Box<dyn System>,
) -> Result<Report, InstallError> {
    let spec = spec();
    let layout = helper_layout(os, roots).map_err(|e| InstallError::Refused(e.to_string()))?;
    let mut backend = backend_for(&layout, &spec, make_system()).map_err(|e| InstallError::Refused(e.to_string()))?;
    let mut sys = make_system();
    let mut installer = Installer::new(&layout, files, backend.as_mut(), sys.as_mut()).with_trust(trust);
    if os == Os::MacOs {
        installer = installer.with_signature(SignatureCheck {
            verifier,
            locations: source_check::signature_locations(source),
        });
    }
    let report = installer.install(source)?;
    log::info!(
        os:? = os,
        sha256 = report.sha256.as_deref().unwrap_or(""),
        signature = report.signature.as_deref().unwrap_or("not required"),
        replaced = report.replaced_existing;
        "the helper was installed"
    );
    Ok(report)
}

/// The elevated side of "turn off": stops and removes the registration and the protected copy.
///
/// # Errors
/// An [`InstallError`].
pub fn uninstall_in_place(
    os: Os,
    roots: &Roots,
    files: &mut dyn ProtectedFiles,
    make_system: &dyn Fn() -> Box<dyn System>,
) -> Result<Report, InstallError> {
    let spec = spec();
    let layout = helper_layout(os, roots).map_err(|e| InstallError::Refused(e.to_string()))?;
    let mut backend = backend_for(&layout, &spec, make_system()).map_err(|e| InstallError::Refused(e.to_string()))?;
    let mut sys = make_system();
    let report = Installer::new(&layout, files, backend.as_mut(), sys.as_mut()).uninstall()?;
    log::info!(os:? = os; "the helper was removed");
    Ok(report)
}

/// The service removing itself (its own `Uninstall` request): files first, the stop last (the process ends with it).
///
/// # Errors
/// An [`InstallError`].
pub fn uninstall_self_in_place(
    os: Os,
    roots: &Roots,
    files: &mut dyn ProtectedFiles,
    make_system: &dyn Fn() -> Box<dyn System>,
) -> Result<Report, InstallError> {
    let spec = spec();
    let layout = helper_layout(os, roots).map_err(|e| InstallError::Refused(e.to_string()))?;
    let mut backend = backend_for(&layout, &spec, make_system()).map_err(|e| InstallError::Refused(e.to_string()))?;
    let mut sys = make_system();
    Installer::new(&layout, files, backend.as_mut(), sys.as_mut()).uninstall_self()
}

pub mod client;
pub mod source_check;

#[cfg(test)]
mod tests;
