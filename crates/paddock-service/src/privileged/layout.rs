//! Where a privileged service lives on each OS (ADR 0027 §1), as pure functions of the target OS, the spec and the
//! folders the OS names, so every layout is tested on every OS.

use crate::backend::{Backend, LaunchdFile, Systemd, Target, WindowsService};
use crate::error::ServiceError;
use crate::paths::launchd_plist_path;
use crate::port::System;
use crate::spec::{Kind, ServiceSpec};
use paddock_env::{MapVars, Os};
use std::path::PathBuf;

/// The macOS folder of protected helper programs (root:wheel, created by the OS).
pub const MACOS_HELPER_FOLDER: &str = "/Library/PrivilegedHelperTools";
/// The Linux folder a package installs the program into.
pub const LINUX_PROGRAM_FOLDER: &str = "/usr/bin";
/// The Linux folder a package installs the system unit into (shipped disabled).
pub const LINUX_UNIT_FOLDER: &str = "/usr/lib/systemd/system";

/// The folders Windows names (as the OS reports them, never the environment: an elevated process may inherit a user-writable
/// one; tests pass their own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roots {
    /// Program Files (`FOLDERID_ProgramFiles`).
    pub program_files: PathBuf,
}

impl Roots {
    /// The roots `folders` names.
    ///
    /// # Errors
    /// [`ServiceError::Unavailable`] when the OS does not name Program Files: nothing is installed into a guessed place.
    pub fn from_known(folders: &dyn super::KnownFolders) -> Result<Self, ServiceError> {
        let program_files = folders.program_files().map_err(|e| {
            ServiceError::Unavailable(format!("Windows did not name the Program Files folder: {e}"))
        })?;
        log::info!(program_files = program_files.display().to_string().as_str(); "install roots from the OS");
        Ok(Self { program_files })
    }
}

/// The protected copy of the program: a folder only an administrator can change, and the file inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyTarget {
    /// The folder.
    pub folder: PathBuf,
    /// The mode the folder gets when it has to be created (Unix; the folder is `root`'s and not writable by others).
    pub folder_mode: u32,
    /// Whether the folder belongs to the OS (macOS `/Library/PrivilegedHelperTools`): never created with a guessed mode
    /// and never removed.
    pub folder_is_system: bool,
    /// The program.
    pub file: PathBuf,
    /// The mode of the program (Unix).
    pub file_mode: u32,
}

/// Where everything of one privileged service goes on one OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The OS the layout is for.
    pub os: Os,
    /// The program the service manager runs (the protected copy, or the packaged program on Linux).
    pub program: PathBuf,
    /// The protected copy this installer makes (`None` on Linux: the package owns `/usr/bin/<program>`).
    pub copy: Option<CopyTarget>,
    /// The registration file: the launchd plist, or the packaged systemd unit (none on Windows: the SCM holds it).
    pub registration: Option<PathBuf>,
}

fn join_windows(base: &std::path::Path, tail: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}\\{tail}",
        base.to_string_lossy().trim_end_matches(['\\', '/'])
    ))
}

/// The layout of `spec` on `os`; `windows_folder` is the folder under Program Files (`Dyno Helper`).
///
/// # Errors
/// [`ServiceError::Unavailable`] when `spec` is not a [`Kind::SystemService`].
pub fn layout(
    os: Os,
    spec: &ServiceSpec,
    windows_folder: &str,
    roots: &Roots,
) -> Result<Layout, ServiceError> {
    if spec.kind() != Kind::SystemService {
        return Err(ServiceError::Unavailable(
            "only a system service has a protected install".into(),
        ));
    }
    Ok(match os {
        Os::MacOs => {
            let file = PathBuf::from(MACOS_HELPER_FOLDER).join(spec.label());
            Layout {
                os,
                program: file.clone(),
                copy: Some(CopyTarget {
                    folder: PathBuf::from(MACOS_HELPER_FOLDER),
                    folder_mode: 0o1755,
                    folder_is_system: true,
                    file,
                    file_mode: 0o755,
                }),
                registration: Some(launchd_plist_path(spec, &MapVars::new())?),
            }
        }
        Os::Windows => {
            let folder = join_windows(&roots.program_files, windows_folder);
            let file = join_windows(&folder, &spec.program_file(true));
            Layout {
                os,
                program: file.clone(),
                copy: Some(CopyTarget {
                    folder,
                    folder_mode: 0,
                    folder_is_system: false,
                    file,
                    file_mode: 0,
                }),
                registration: None,
            }
        }
        Os::Linux => Layout {
            os,
            program: PathBuf::from(LINUX_PROGRAM_FOLDER).join(spec.program()),
            copy: None,
            registration: Some(PathBuf::from(LINUX_UNIT_FOLDER).join(spec.unit_file_name())),
        },
    })
}

/// The backend that registers `spec` as `layout` says: a launchd daemon plist, the Windows service (with its
/// privileges and start DACL), or the packaged systemd unit enabled and started. `sys` is the OS.
///
/// # Errors
/// [`ServiceError::Unavailable`] when `spec` is not a system service or the layout lacks its registration file.
pub fn backend_for(
    layout: &Layout,
    spec: &ServiceSpec,
    sys: Box<dyn System>,
) -> Result<Box<dyn Backend>, ServiceError> {
    let target = Target::exe(layout.program.clone());
    let missing = || ServiceError::Unavailable("the layout has no registration file".into());
    Ok(match layout.os {
        Os::MacOs => Box::new(LaunchdFile::new(
            spec.clone(),
            0,
            layout.registration.clone().ok_or_else(missing)?,
            target,
            sys,
        )),
        Os::Windows => Box::new(WindowsService::new(spec.clone(), target, sys)),
        Os::Linux => Box::new(
            Systemd::new(
                spec.clone(),
                layout.registration.clone().ok_or_else(missing)?,
                target,
                sys,
            )
            .packaged(),
        ),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn helper_spec() -> ServiceSpec {
        use crate::hardening::{Hardening, Sandbox};
        ServiceSpec::builder("net.decryptology.dyno.helper", "DynoHelper", "dyno-helper")
            .unit("dyno-helper")
            .display_name("Dyno helper")
            .args(["serve"])
            .kind(Kind::SystemService)
            .restart(crate::spec::Restart::OnFailure)
            .hardening(
                Hardening::new()
                    .windows_privileges(["SeChangeNotifyPrivilege"])
                    .users_may_start()
                    .sandbox(
                        Sandbox::new()
                            .capabilities(["CAP_SYS_PTRACE"])
                            .address_families(["AF_UNIX"]),
                    ),
            )
            .build()
            .unwrap()
    }

    fn roots() -> Roots {
        Roots {
            program_files: PathBuf::from("C:\\Program Files"),
        }
    }

    #[test]
    fn every_os_has_its_protected_places() {
        let spec = helper_spec();
        let mac = layout(Os::MacOs, &spec, "Dyno Helper", &roots()).unwrap();
        assert_eq!(
            mac.program,
            PathBuf::from("/Library/PrivilegedHelperTools/net.decryptology.dyno.helper")
        );
        let copy = mac.copy.unwrap();
        assert_eq!(
            (copy.folder_mode, copy.file_mode, copy.folder_is_system),
            (0o1755, 0o755, true)
        );
        assert_eq!(
            mac.registration.unwrap(),
            PathBuf::from("/Library/LaunchDaemons/net.decryptology.dyno.helper.plist")
        );
        let win = layout(Os::Windows, &spec, "Dyno Helper", &roots()).unwrap();
        assert_eq!(
            win.program,
            PathBuf::from("C:\\Program Files\\Dyno Helper\\dyno-helper.exe")
        );
        let copy = win.copy.unwrap();
        assert_eq!(copy.folder, PathBuf::from("C:\\Program Files\\Dyno Helper"));
        assert!(!copy.folder_is_system && win.registration.is_none());
        let lin = layout(Os::Linux, &spec, "Dyno Helper", &roots()).unwrap();
        assert_eq!(lin.program, PathBuf::from("/usr/bin/dyno-helper"));
        assert!(lin.copy.is_none());
        assert_eq!(
            lin.registration.unwrap(),
            PathBuf::from("/usr/lib/systemd/system/dyno-helper.service")
        );
    }

    #[test]
    fn a_user_agent_has_no_protected_install() {
        let agent = ServiceSpec::builder("net.decryptology.dyno.agent", "Dyno", "dyno-agent")
            .build()
            .unwrap();
        assert!(layout(Os::Linux, &agent, "x", &roots()).is_err());
    }

    #[test]
    fn the_backend_follows_the_os() {
        use crate::backend::Mechanism;
        use crate::testing::FakeSystem;
        let spec = helper_spec();
        for (os, mechanism) in [
            (Os::MacOs, Mechanism::LaunchdFile),
            (Os::Windows, Mechanism::WindowsService),
            (Os::Linux, Mechanism::SystemdSystem),
        ] {
            let l = layout(os, &spec, "Dyno Helper", &roots()).unwrap();
            let b = backend_for(&l, &spec, Box::new(FakeSystem::new())).unwrap();
            assert_eq!(b.mechanism(), mechanism);
        }
        let mut broken = layout(Os::MacOs, &spec, "x", &roots()).unwrap();
        broken.registration = None;
        assert!(backend_for(&broken, &spec, Box::new(FakeSystem::new())).is_err());
        broken.os = Os::Linux;
        assert!(backend_for(&broken, &spec, Box::new(FakeSystem::new())).is_err());
    }
}
