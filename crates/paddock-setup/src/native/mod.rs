//! The Windows glue: reads the machine, calls the pure core, prints, logs and returns the exit code. No decisions of its
//! own (they are in the library modules, unit-tested over fakes); this directory is excluded from the coverage gate and is
//! tested on a Windows machine (`tests/it/windows.rs`).

mod apply;
mod facts;
mod folders;
mod handle;
mod hold;
mod logfs;
mod registry;
mod wide;

use crate::args::{self, Command, RunScope};
use crate::checkdir::{self, CheckedDir};
use crate::exit::Exit;
use crate::log::{Clock, NoLog, SetupLog, Sink};
use crate::{BAKED, guard, runvalues, samefolder, selftest, staging, stop};
use facts::{Machine, RealDirs};
use logfs::{FileLog, SystemClock};
use registry::{Root, View};
use std::path::PathBuf;

type FileSetupLog = SetupLog<FileLog, SystemClock>;

fn opened_log(proof: &CheckedDir) -> FileSetupLog {
    SetupLog::open(proof, FileLog::new(PathBuf::from(proof.path()).join("updates").join("setup.log")), SystemClock)
}

/// `check-dir`, then `updates\` is made, then the log is opened. `None` log when the check refuses.
fn checked(dir: &str) -> Result<(CheckedDir, FileSetupLog), checkdir::DirRefusal> {
    let proof = checkdir::check(&RealDirs, dir)?;
    // Folder trouble does not fail the check; it only means there is no log.
    let _ = folders::ensure(&PathBuf::from(proof.path()).join("updates").to_string_lossy());
    let log = opened_log(&proof);
    Ok((proof, log))
}

fn say(text: &str) {
    println!("{text}");
}

/// Runs a command line; returns the process exit code.
pub fn main(argv: &[String]) -> i32 {
    let command = match args::parse(argv) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("paddock-setup: {e}");
            return Exit::Usage.code();
        }
    };
    let exit = run(command);
    exit.code()
}

fn run(command: Command) -> Exit {
    match command {
        Command::SelfTest => {
            for line in selftest::report(&BAKED) {
                say(&line);
            }
            Exit::Ok
        }
        Command::SameFolder { a, b } => {
            let name = |p: &str| handle::open_for_query(p).ok().and_then(|h| handle::final_path(&h));
            if samefolder::same(name(&a).as_deref(), name(&b).as_deref()) { Exit::Ok } else { Exit::NotSame }
        }
        Command::CheckDir { dir } => match checked(&dir) {
            Ok((proof, mut log)) => {
                log.line("check-dir", "folder accepted");
                say(&format!("ok {}", proof.path()));
                Exit::Ok
            }
            Err(refusal) => {
                // Nothing is written for a refusal: the folder is the thing refused.
                let app = BAKED.product.unwrap_or("the app");
                say(&refusal.message(app, &dir));
                Exit::BadDir
            }
        },
        Command::Guard { dir, product, main, update, restart } => {
            let mut file_log = dir.as_deref().and_then(|d| checked(d).ok()).map(|(_, l)| l);
            let mut none = NoLog;
            let log: &mut dyn Sink = match file_log.as_mut() {
                Some(l) => l,
                None => &mut none,
            };
            let machine = Machine { product: product.clone() };
            match guard::decide(&machine, &product, &main, update, restart, log) {
                guard::Decision::NotFresh | guard::Decision::Proceed => Exit::Ok,
                guard::Decision::Blocked { message, restart } => {
                    say(&message);
                    if let Some(program) = restart {
                        let started = facts::desktop_token().is_some_and(|t| spawn_as_desktop_user(&t, &program));
                        log.line("guard", if started { "the older copy was started again" } else { "the older copy could not be started again" });
                    }
                    Exit::Blocked
                }
            }
        }
        Command::StopAll { dir, images } => {
            let Ok((_proof, mut log)) = checked(&dir) else {
                say("The install folder is not allowed.");
                return Exit::BadDir;
            };
            let t = log.begin("stop-all");
            let mut port = StopAdapter;
            let exit = stop::run(&mut port, &dir, &images, &mut log);
            log.end("stop-all", t, exit.sentence());
            log.exit(exit.code(), exit.sentence());
            exit
        }
        Command::CleanRun { dir, scope } => clean_run(&dir, scope),
        Command::CleanStaging { dir } => {
            let Ok((_proof, mut log)) = checked(&dir) else { return Exit::BadDir };
            let updates = PathBuf::from(&dir).join("updates");
            let entries: Vec<staging::Entry> = std::fs::read_dir(&updates)
                .map(|d| {
                    d.filter_map(|e| e.ok())
                        .map(|e| staging::Entry { name: e.file_name().to_string_lossy().into_owned(), is_dir: e.file_type().map(|t| t.is_dir()).unwrap_or(false) })
                        .collect()
                })
                .unwrap_or_default();
            let mut failed = false;
            for name in staging::removals(&entries) {
                let p = updates.join(&name);
                let is_dir = entries.iter().any(|e| e.name == name && e.is_dir);
                if (if is_dir { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) }).is_err() {
                    failed = true;
                }
            }
            log.line("clean-staging", if failed { "some staged files could not be removed" } else { "staging removed" });
            if failed { Exit::Failed } else { Exit::Ok }
        }
        Command::ApplyUpdate { file, signature, version, size } => apply_update(file, signature, version, size),
        Command::HoldInstalling => match BAKED.identifier {
            None => Exit::Failed,
            Some(id) => match hold::hold(id) {
                Ok(()) => Exit::Ok,
                Err(e) => {
                    eprintln!("paddock-setup: {e}");
                    Exit::Failed
                }
            },
        },
    }
}

fn apply_update(file: String, signature: String, version: semver::Version, size: Option<u64>) -> Exit {
    let Ok(exe) = std::env::current_exe() else { return Exit::Failed };
    let Some(instdir) = exe.parent().map(|p| p.to_string_lossy().into_owned()) else { return Exit::Failed };
    let layout = crate::apply::Layout::new(&instdir);
    // The log exists only after check-dir passed (a refusal returns before any write).
    let Ok((_proof, mut log)) = checked(&instdir) else {
        return Exit::Refused(crate::apply::Refusal::InstallDir);
    };
    let key = BAKED.update_key.and_then(|k| paddock_updater::signature::UpdateKey::resolve([Some(k)]));
    let (Some(main), Some(identifier)) = (BAKED.main, BAKED.identifier) else {
        log.line("apply-update", "this build has no identifier or main binary name");
        return Exit::Refused(crate::apply::Refusal::NoKey);
    };
    let started = SystemClock.unix_ms();
    let request = crate::apply::Request { file, signature, version, size };
    let cell = std::cell::RefCell::new(log);
    let result = {
        let mut port = apply::Real::new(layout.clone(), main.to_string(), identifier.to_string(), key, Shared(&cell));
        crate::apply::run(&mut port, &layout, &request, &mut Shared(&cell))
    };
    let mut log = cell.into_inner();
    log.line("apply-update", &format!("finished in {} ms", SystemClock.unix_ms().saturating_sub(started)));
    match result {
        Ok(()) => Exit::Ok,
        Err(r) => {
            log.exit(r.code(), r.sentence());
            Exit::Refused(r)
        }
    }
}

/// One log shared by the sequence and the port that it drives.
pub struct Shared<'a>(&'a std::cell::RefCell<FileSetupLog>);

impl Sink for Shared<'_> {
    fn line(&mut self, phase: &str, message: &str) {
        self.0.borrow_mut().line(phase, message);
    }
}

fn clean_run(dir: &str, scope: RunScope) -> Exit {
    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    let Ok((_proof, mut log)) = checked(dir) else { return Exit::BadDir };
    let mut failed = false;
    let mut removed = 0u32;
    for view in [View::Wide, View::Narrow] {
        if let Some(key) = registry::open_write(Root::Machine, RUN, view) {
            let values: Vec<runvalues::RunValue> = key.string_values().into_iter().map(|(name, command)| runvalues::RunValue { name, command }).collect();
            for name in runvalues::machine_removals(&values, dir) {
                if key.delete_value(&name) { removed += 1 } else { failed = true }
            }
        }
    }
    if scope == RunScope::MachineAndUsers {
        let product = BAKED.product.unwrap_or_default();
        let hives = registry::open_read(Root::Users, "", View::Wide).map(|k| k.subkeys()).unwrap_or_default();
        for sid in guard::scan_accounts(&hives) {
            if let Some(key) = registry::open_write(Root::Users, &format!(r"{sid}\{RUN}"), View::Wide) {
                let values: Vec<runvalues::RunValue> = key.string_values().into_iter().map(|(name, command)| runvalues::RunValue { name, command }).collect();
                for name in runvalues::user_removals(&values, dir, product) {
                    if key.delete_value(&name) { removed += 1 } else { failed = true }
                }
            }
        }
    }
    log.line("clean-run", &format!("removed {removed} value(s){}", if failed { ", some could not be removed" } else { "" }));
    if failed { Exit::Failed } else { Exit::Ok }
}

/// `stop-all` over `paddock_proc::installed::stop_all` (plan S1). Without the `proc-stop` feature the command answers
/// "not built" instead of guessing.
struct StopAdapter;

#[cfg(not(feature = "proc-stop"))]
impl stop::StopPort for StopAdapter {
    fn stop_all(&mut self, _dir: &str, _images: &[String]) -> Result<stop::StopReport, String> {
        Err("this build of paddock-setup has no process stopper (feature proc-stop)".into())
    }
}

#[cfg(feature = "proc-stop")]
impl stop::StopPort for StopAdapter {
    fn stop_all(&mut self, dir: &str, images: &[String]) -> Result<stop::StopReport, String> {
        let _ = (dir, images);
        // Wired when `paddock_proc::installed::stop_all(dir, images, bounds) -> StopReport` (ADR 0053 section 3) lands.
        Err("paddock_proc::installed is not available yet".into())
    }
}

/// Starts `program` with the desktop user's token (`CreateProcessWithTokenW`, unelevated).
fn spawn_as_desktop_user(token: &handle::Owned, program: &str) -> bool {
    use windows_sys::Win32::System::Threading::{CreateProcessWithTokenW, PROCESS_INFORMATION, STARTUPINFOW};
    let wide = wide::to_wide(program);
    let mut cmd = wide::to_wide(&format!("\"{program}\""));
    let dir = std::path::Path::new(program).parent().map(|p| wide::to_wide(&p.to_string_lossy()));
    // SAFETY: zeroed structures of the right type; the strings live through the call.
    unsafe {
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessWithTokenW(
            token.0,
            0,
            wide.as_ptr(),
            cmd.as_mut_ptr(),
            0,
            std::ptr::null(),
            dir.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
            &si,
            &mut pi,
        );
        if ok == 0 {
            return false;
        }
        drop(handle::Owned(pi.hProcess));
        drop(handle::Owned(pi.hThread));
        true
    }
}
