//! Who may call the helper (ADR 0016 section 3). Pure over the facts the OS gave about the peer; the real facts come
//! from `dyno-ipc` (the kernel's user id and process id) and [`crate::sys::process_image`].

use dyno_core::helper_wire::HelperUser;
use std::path::{Path, PathBuf};

/// The rules the daemon was installed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// The only user served.
    pub user: HelperUser,
    /// The only program that may call.
    pub agent_exe: PathBuf,
}

/// Why a caller was refused (recorded in the audit log, never shown with details to the caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The kernel did not say who connected, or it is another user.
    WrongUser(String),
    /// The caller's program could not be determined or is not the recorded agent.
    WrongProgram(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::WrongUser(m) | Refusal::WrongProgram(m) => f.write_str(m),
        }
    }
}

/// Looks up the program file of a process.
pub trait ImageOf: Send + Sync {
    /// The program file of `pid`, when known.
    fn image(&self, pid: u32) -> Option<PathBuf>;
}

/// The real lookup.
pub struct OsImages;

impl ImageOf for OsImages {
    fn image(&self, pid: u32) -> Option<PathBuf> {
        crate::sys::process_image(pid)
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (a, b) = (canon(a), canon(b));
    if cfg!(windows) {
        a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

impl Policy {
    /// Decides whether a peer may use the helper. `uid` is the kernel's answer on Unix (`None` on Windows, where the
    /// pipe's access list already admitted only SYSTEM and the authorised account); `pid` the peer's process id.
    pub fn check(&self, uid: Option<u32>, pid: Option<u32>, images: &dyn ImageOf) -> Result<(), Refusal> {
        match (&self.user, uid) {
            (HelperUser::Uid(allowed), Some(got)) if *allowed == got => {}
            (HelperUser::Uid(_), Some(got)) => return Err(Refusal::WrongUser(format!("user {got} is not the user the helper serves"))),
            (HelperUser::Uid(_), None) => return Err(Refusal::WrongUser("the system did not say who connected".into())),
            // The pipe's access list is the check on Windows.
            (HelperUser::Sid(_), _) => {}
        }
        let pid = pid.ok_or_else(|| Refusal::WrongProgram("the system did not say which process connected".into()))?;
        let image = images
            .image(pid)
            .ok_or_else(|| Refusal::WrongProgram(format!("process {pid} could not be identified")))?;
        if same_file(&image, &self.agent_exe) {
            Ok(())
        } else {
            Err(Refusal::WrongProgram(format!(
                "process {pid} is {}, not the Dyno agent",
                image.display()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Images(Option<PathBuf>);
    impl ImageOf for Images {
        fn image(&self, _: u32) -> Option<PathBuf> {
            self.0.clone()
        }
    }

    fn policy(user: HelperUser) -> Policy {
        Policy {
            user,
            agent_exe: PathBuf::from("/opt/dyno/dyno-agent"),
        }
    }

    #[test]
    fn the_authorised_user_with_the_recorded_agent_is_let_in() {
        let p = policy(HelperUser::Uid(501));
        assert_eq!(p.check(Some(501), Some(9), &Images(Some("/opt/dyno/dyno-agent".into()))), Ok(()));
    }

    #[test]
    fn another_user_an_unknown_user_or_an_unknown_process_is_refused() {
        let p = policy(HelperUser::Uid(501));
        let agent = Images(Some("/opt/dyno/dyno-agent".into()));
        assert!(matches!(p.check(Some(502), Some(9), &agent), Err(Refusal::WrongUser(m)) if m.contains("502")));
        assert!(matches!(p.check(None, Some(9), &agent), Err(Refusal::WrongUser(_))));
        assert!(matches!(p.check(Some(501), None, &agent), Err(Refusal::WrongProgram(_))));
        assert!(matches!(p.check(Some(501), Some(9), &Images(None)), Err(Refusal::WrongProgram(m)) if m.contains("identified")));
    }

    #[test]
    fn another_program_of_the_same_user_is_refused() {
        let p = policy(HelperUser::Uid(501));
        let other = Images(Some("/tmp/evil/dyno-agent".into()));
        let r = p.check(Some(501), Some(9), &other).unwrap_err();
        assert!(matches!(&r, Refusal::WrongProgram(m) if m.contains("/tmp/evil/dyno-agent") && m.contains("not the Dyno agent")));
        assert!(r.to_string().contains("not the Dyno agent"));
    }

    #[test]
    fn on_windows_the_pipe_list_is_the_user_check_and_the_program_is_still_checked() {
        let p = policy(HelperUser::Sid("S-1-5-21-1-2-3-1001".into()));
        assert_eq!(p.check(None, Some(9), &Images(Some("/opt/dyno/dyno-agent".into()))), Ok(()));
        assert!(p.check(None, Some(9), &Images(Some("/x".into()))).is_err());
    }

    #[test]
    fn a_link_to_the_agent_counts_as_the_agent() {
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            let real = dir.path().join("dyno-agent");
            std::fs::write(&real, b"x").unwrap();
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            let p = Policy {
                user: HelperUser::Uid(1),
                agent_exe: real,
            };
            assert_eq!(p.check(Some(1), Some(2), &Images(Some(link))), Ok(()));
        }
    }

    #[test]
    fn the_real_lookup_knows_this_process() {
        let me = OsImages.image(std::process::id());
        if cfg!(any(windows, target_os = "macos", target_os = "linux")) {
            assert_eq!(
                me.map(|p| std::fs::canonicalize(p).unwrap()),
                Some(std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap())
            );
        }
    }
}
