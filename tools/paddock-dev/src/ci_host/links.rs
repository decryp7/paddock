//! The link a host step leaves: a symlink on Unix, a junction on Windows (Git bash's `ln -s` copies there, so the shell called
//! `cmd //c mklink //J`). The OS call sits behind [`Links`] so the Windows spelling is unit-tested on any OS with a recording
//! fake (the call cannot run here), and the real one type-checks for Windows (`cargo xwin check`).

use crate::ci_isolation::home;
use crate::ci_tool::ToolCall;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// Makes `link` a link to the folder `target`.
pub trait Links {
    /// Creates the link; `Err` is the OS's or the tool's message.
    fn link_dir(&self, target: &Path, link: &Path) -> Result<(), String>;
}

/// The call that makes a junction: `cmd /c mklink /J <link> <target>`, both as Windows paths.
pub fn junction_call(target: &str, link: &str) -> ToolCall {
    ToolCall::new("cmd")
        .args(["/c", "mklink", "/J", &home::native(link), &home::native(target)])
        .timeout(Duration::from_secs(60))
}

/// The real thing: a junction through `cmd` on Windows, a symlink elsewhere.
pub struct OsLinks<'a> {
    /// The job's environment, for the program's `PATH`.
    pub env: &'a HashMap<String, String>,
    /// Make junctions (Windows semantics), whatever the host.
    pub windows: bool,
}

impl Links for OsLinks<'_> {
    fn link_dir(&self, target: &Path, link: &Path) -> Result<(), String> {
        if self.windows {
            let call = junction_call(
                &paddock_env::winpath::lexical_text(target, paddock_env::Os::current()),
                &paddock_env::winpath::lexical_text(link, paddock_env::Os::current()),
            );
            let ran = call.run(self.env, &mut std::io::sink())?;
            return if ran.success {
                Ok(())
            } else {
                Err(format!("mklink /J failed ({}): {}", ran.how, ran.stderr.trim()))
            };
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).map_err(|e| e.to_string())
        }
        #[cfg(not(unix))]
        {
            Err("symlinks are not made on this OS".to_string())
        }
    }
}

/// A recording link maker for the unit tests of the steps that link.
#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::Mutex;

    /// Records every link asked for; fails when `fail` is set.
    #[derive(Default)]
    pub struct RecordingLinks {
        /// The (target, link) pairs made so far.
        pub made: Mutex<Vec<(String, String)>>,
        /// Refuse every link.
        pub fail: bool,
    }

    impl Links for RecordingLinks {
        fn link_dir(&self, target: &Path, link: &Path) -> Result<(), String> {
            if self.fail {
                return Err("refused".into());
            }
            self.made
                .lock()
                .unwrap()
                .push((target.display().to_string(), link.display().to_string()));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_junction_is_mklink_j_with_the_link_first_and_windows_paths() {
        let c = junction_call("C:/ci/src/x-s1", "C:/work/abc/hostexecutor");
        let shown = c.display();
        #[cfg(windows)]
        assert_eq!(shown, r"cmd /c mklink /J C:\work\abc\hostexecutor C:\ci\src\x-s1");
        #[cfg(not(windows))]
        assert_eq!(shown, "cmd /c mklink /J C:/work/abc/hostexecutor C:/ci/src/x-s1");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_made_on_unix() {
        let d = crate::ci_host::canonical_tempdir();
        let target = d.path().join("t");
        std::fs::create_dir(&target).unwrap();
        let env = HashMap::new();
        OsLinks { env: &env, windows: false }
            .link_dir(&target, &d.path().join("l"))
            .unwrap();
        assert!(std::fs::symlink_metadata(d.path().join("l")).unwrap().file_type().is_symlink());
        assert!(
            OsLinks { env: &env, windows: false }
                .link_dir(&target, &d.path().join("l"))
                .is_err(),
            "exists"
        );
    }
}
