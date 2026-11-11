//! macOS: start the runner as its own *responsible process* (ADR 0020).
//!
//! macOS attributes a process's access to protected folders (Documents, Desktop, Downloads, removable volumes...) to its
//! *responsible process*, by default the app that started it. The runner and every CI job are started by Pitcrew's
//! service from the Pitcrew.app bundle, so a job that reads `~/Documents` made macOS ask "Pitcrew.app would like to
//! access files in your Documents folder", although Pitcrew itself never touches it. Terminal, iTerm2 and VS Code avoid
//! this with `responsibility_spawnattrs_setdisclaim(attr, 1)` (a private but long-stable libSystem function) on the
//! `posix_spawn` attributes: the child, and so its descendants, are their own responsible processes.
//!
//! `std::process::Command` offers no hook into the spawn attributes, so this module spawns with `posix_spawn` itself,
//! keeping what the std path gave: a pipe each for standard output and error, no standard input, its own process group
//! (graceful stop and tree kill use the group), the working folder, the environment, `SIGPIPE` back to its default and
//! no inherited descriptors. The function is looked up with `dlsym`; if it is missing the runner is started the old way
//! (still without the disclaim) and the supervisor logs that once.
//!
//! This is the one place on macOS with `unsafe` (FFI to libSystem); every block says why it is sound.
#![allow(unsafe_code)]

use std::ffi::{c_int, CString};
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::path::Path;

/// `int responsibility_spawnattrs_setdisclaim(posix_spawnattr_t *attr, int disclaim)`.
pub type DisclaimFn = unsafe extern "C" fn(*mut libc::posix_spawnattr_t, c_int) -> c_int;

/// The disclaim function of libSystem, when this macOS has it.
pub fn find_disclaim() -> Option<DisclaimFn> {
    // SAFETY: `dlsym` with RTLD_DEFAULT and a NUL-terminated name only reads the symbol table. The symbol, when present,
    // has the C signature above (it is what Terminal and VS Code call); a null result is mapped to `None`.
    unsafe {
        let p = libc::dlsym(libc::RTLD_DEFAULT, c"responsibility_spawnattrs_setdisclaim".as_ptr());
        if p.is_null() {
            None
        } else {
            Some(std::mem::transmute::<*mut libc::c_void, DisclaimFn>(p))
        }
    }
}

/// Whether this macOS lets the runner be started as its own responsible process.
pub fn disclaim_supported() -> bool {
    find_disclaim().is_some()
}

/// A process started by [`spawn`].
pub struct Spawned {
    /// Its process id (also its process group).
    pub pid: i32,
    /// Its standard output.
    pub stdout: File,
    /// Its standard error.
    pub stderr: File,
    /// The disclaim was applied to the spawn attributes.
    pub disclaimed: bool,
}

fn cstring(s: &std::ffi::OsStr) -> Result<CString, String> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(s.as_bytes()).map_err(|_| "a program, argument or variable contains a NUL character".to_string())
}

fn check(rc: c_int, what: &str) -> Result<(), String> {
    if rc == 0 {
        Ok(())
    } else {
        Err(format!("{what} failed: {}", std::io::Error::from_raw_os_error(rc)))
    }
}

fn pipe() -> Result<(OwnedFd, OwnedFd), String> {
    nix::unistd::pipe().map_err(|e| format!("a pipe could not be made: {e}"))
}

/// Starts `program args` in `dir` with `env` over the service's own environment. `disclaim` is the function to apply
/// (`find_disclaim()` in production, a recorder in tests, `None` for the old behaviour).
pub fn spawn(
    program: &Path,
    args: &[String],
    dir: &Path,
    env: &[(String, String)],
    disclaim: Option<DisclaimFn>,
) -> Result<Spawned, String> {
    use std::os::unix::ffi::OsStrExt;
    let prog = cstring(program.as_os_str())?;
    let mut argv_c = vec![prog.clone()];
    for a in args {
        argv_c.push(cstring(std::ffi::OsStr::new(a))?);
    }
    let mut vars: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString> = std::env::vars_os().collect();
    for (k, v) in env {
        vars.insert(k.into(), v.into());
    }
    let mut envp_c = Vec::with_capacity(vars.len());
    for (k, v) in &vars {
        let mut kv = k.as_bytes().to_vec();
        kv.push(b'=');
        kv.extend_from_slice(v.as_bytes());
        envp_c.push(CString::new(kv).map_err(|_| "a variable contains a NUL character".to_string())?);
    }
    let dir_c = cstring(dir.as_os_str())?;

    let (out_r, out_w) = pipe()?;
    let (err_r, err_w) = pipe()?;

    // SAFETY: attr and actions are initialised before use and destroyed on every path; all pointers passed to
    // posix_spawn (argv, envp, strings) outlive the call; the descriptors are valid until after it returns.
    unsafe {
        let mut attr: libc::posix_spawnattr_t = std::ptr::null_mut();
        check(libc::posix_spawnattr_init(&mut attr), "posix_spawnattr_init")?;
        let mut actions: libc::posix_spawn_file_actions_t = std::ptr::null_mut();
        if let Err(e) = check(libc::posix_spawn_file_actions_init(&mut actions), "posix_spawn_file_actions_init") {
            libc::posix_spawnattr_destroy(&mut attr);
            return Err(e);
        }
        let result = (|| -> Result<(i32, bool), String> {
            // Own process group; SIGPIPE back to the default (the service ignores it, and "ignored" is inherited); an
            // empty signal mask; no descriptors beyond the ones set up below.
            let mut default_sigs: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut default_sigs);
            libc::sigaddset(&mut default_sigs, libc::SIGPIPE);
            let mut mask: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut mask);
            check(
                libc::posix_spawnattr_setsigdefault(&mut attr, &default_sigs),
                "posix_spawnattr_setsigdefault",
            )?;
            check(libc::posix_spawnattr_setsigmask(&mut attr, &mask), "posix_spawnattr_setsigmask")?;
            check(libc::posix_spawnattr_setpgroup(&mut attr, 0), "posix_spawnattr_setpgroup")?;
            let flags = libc::POSIX_SPAWN_SETPGROUP
                | libc::POSIX_SPAWN_SETSIGDEF
                | libc::POSIX_SPAWN_SETSIGMASK
                | libc::POSIX_SPAWN_CLOEXEC_DEFAULT;
            check(
                libc::posix_spawnattr_setflags(&mut attr, flags as libc::c_short),
                "posix_spawnattr_setflags",
            )?;
            let mut disclaimed = false;
            if let Some(f) = disclaim {
                // The runner and its jobs become their own responsible processes (see the module comment).
                check(f(&mut attr, 1), "responsibility_spawnattrs_setdisclaim")?;
                disclaimed = true;
            }
            check(
                libc::posix_spawn_file_actions_addopen(&mut actions, 0, c"/dev/null".as_ptr(), libc::O_RDONLY, 0),
                "stdin",
            )?;
            check(libc::posix_spawn_file_actions_adddup2(&mut actions, out_w.as_raw_fd(), 1), "stdout")?;
            check(libc::posix_spawn_file_actions_adddup2(&mut actions, err_w.as_raw_fd(), 2), "stderr")?;
            check(
                libc::posix_spawn_file_actions_addchdir_np(&mut actions, dir_c.as_ptr()),
                "the working folder",
            )?;
            let mut argv: Vec<*mut libc::c_char> = argv_c.iter().map(|c| c.as_ptr().cast_mut()).collect();
            argv.push(std::ptr::null_mut());
            let mut envp: Vec<*mut libc::c_char> = envp_c.iter().map(|c| c.as_ptr().cast_mut()).collect();
            envp.push(std::ptr::null_mut());
            let mut pid: libc::pid_t = 0;
            check(
                libc::posix_spawn(&mut pid, prog.as_ptr(), &actions, &attr, argv.as_ptr(), envp.as_ptr()),
                "posix_spawn",
            )?;
            Ok((pid, disclaimed))
        })();
        libc::posix_spawn_file_actions_destroy(&mut actions);
        libc::posix_spawnattr_destroy(&mut attr);
        let (pid, disclaimed) = result.map_err(|e| format!("{} could not be started: {e}", program.display()))?;
        drop((out_w, err_w));
        // SAFETY: the read ends are valid descriptors we own; `File` takes over closing them.
        let stdout = File::from_raw_fd(out_r.into_raw_fd());
        let stderr = File::from_raw_fd(err_r.into_raw_fd());
        Ok(Spawned {
            pid,
            stdout,
            stderr,
            disclaimed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::atomic::{AtomicI32, Ordering};

    static SEEN: AtomicI32 = AtomicI32::new(-1);

    unsafe extern "C" fn recorder(_attr: *mut libc::posix_spawnattr_t, disclaim: c_int) -> c_int {
        SEEN.store(disclaim, Ordering::SeqCst);
        0
    }

    fn run(f: Option<DisclaimFn>, script: &str, env: &[(String, String)], dir: &Path) -> (String, String, i32, bool) {
        let mut s = spawn(Path::new("/bin/sh"), &["-c".into(), script.into()], dir, env, f).unwrap();
        let (mut out, mut err) = (String::new(), String::new());
        s.stdout.read_to_string(&mut out).unwrap();
        s.stderr.read_to_string(&mut err).unwrap();
        let st = nix::sys::wait::waitpid(nix::unistd::Pid::from_raw(s.pid), None).unwrap();
        let code = match st {
            nix::sys::wait::WaitStatus::Exited(_, c) => c,
            _ => -1,
        };
        (out, err, code, s.disclaimed)
    }

    #[test]
    fn the_disclaim_attribute_is_applied_when_the_function_exists_and_the_child_runs_as_configured() {
        let dir = paddock_testkit::boot_volume::boot_volume_temp();
        let script = "echo out:$PWD; echo err >&2; echo var:$PITCREW_TEST_VAR; read x < /dev/stdin; echo stdin:[$x]; ps -o pgid= -p $$ | tr -d ' ' ; exit 7";
        SEEN.store(-1, Ordering::SeqCst);
        let (out, err, code, disclaimed) = run(Some(recorder), script, &[("PITCREW_TEST_VAR".into(), "yes".into())], &dir);
        assert_eq!(SEEN.load(Ordering::SeqCst), 1, "disclaim=1 was set on the attributes");
        assert!(disclaimed);
        assert_eq!(code, 7);
        assert_eq!(err.trim(), "err");
        assert!(out.contains("var:yes") && out.contains("stdin:[]"), "{out}");
        let canon = dir.canonicalize().unwrap();
        assert!(
            out.contains(&format!("out:{}", canon.display())) || out.contains(&format!("out:{}", dir.display())),
            "{out}"
        );
    }

    #[test]
    fn without_the_function_the_runner_still_starts_just_not_disclaimed() {
        let (out, _, code, disclaimed) = run(None, "echo hi", &[], &paddock_testkit::boot_volume::boot_volume_temp());
        assert_eq!((out.trim(), code, disclaimed), ("hi", 0, false));
    }

    #[test]
    fn the_real_function_exists_on_this_macos_and_works() {
        assert!(disclaim_supported(), "libSystem has responsibility_spawnattrs_setdisclaim");
        let (out, _, code, disclaimed) = run(find_disclaim(), "echo real", &[], &paddock_testkit::boot_volume::boot_volume_temp());
        assert_eq!((out.trim(), code, disclaimed), ("real", 0, true));
    }

    #[test]
    #[cfg_attr(
        unix,
        expect(
            clippy::disallowed_methods,
            reason = "ADR 0033 WP-6 (test): killpg on a recorded pid; use paddock_proc"
        )
    )]
    fn the_child_leads_its_own_process_group_and_a_bad_program_is_an_error() {
        let mut s = spawn(
            Path::new("/bin/sh"),
            &["-c".into(), "sleep 30".into()],
            &paddock_testkit::boot_volume::boot_volume_temp(),
            &[],
            find_disclaim(),
        )
        .unwrap();
        assert_eq!(
            nix::unistd::getpgid(Some(nix::unistd::Pid::from_raw(s.pid))).unwrap().as_raw(),
            s.pid
        );
        nix::sys::signal::killpg(nix::unistd::Pid::from_raw(s.pid), nix::sys::signal::Signal::SIGKILL).unwrap();
        let _ = nix::sys::wait::waitpid(nix::unistd::Pid::from_raw(s.pid), None);
        let mut sink = String::new();
        let _ = s.stdout.read_to_string(&mut sink);
        let err = spawn(
            Path::new("/nonexistent/program"),
            &[],
            &paddock_testkit::boot_volume::boot_volume_temp(),
            &[],
            None,
        )
        .err()
        .unwrap();
        assert!(err.contains("could not be started"), "{err}");
        let nul = spawn(
            Path::new("/bin/sh"),
            &["a\0b".into()],
            &paddock_testkit::boot_volume::boot_volume_temp(),
            &[],
            None,
        )
        .err()
        .unwrap();
        assert!(nul.contains("NUL"));
    }
}
