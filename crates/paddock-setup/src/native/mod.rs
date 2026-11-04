//! The Windows glue: reads the machine, calls the pure core, prints, logs and returns the exit code. No decisions of its
//! own (they are in the library modules, unit-tested over fakes); this directory is excluded from the coverage gate and is
//! tested on a Windows machine (`tests/vm`, opt-in, ER-14).

mod apply;
mod facts;
mod folders;
mod handle;
mod hold;
mod logfs;
pub mod mutex;
mod registry;
mod staging_fs;
mod wide;

pub use mutex::{installer_probe, wait_for_installer};

use crate::args::{self, Command};
use crate::checkdir::{self, CheckedDir};
use crate::exit::Exit;
use crate::log::{Clock, NoLog, SetupLog, Sink};
use crate::{BAKED, guard, runvalues, samefolder, selftest, staging, stop};
use facts::{Machine, RealDirs};
use logfs::{FileLog, SystemClock};
use registry::{Root, View};
use std::path::PathBuf;

type FileSetupLog = SetupLog<FileLog, SystemClock>;

/// The log of a folder that passed `check-dir`: `<canonical folder>\updates\setup.log`.
fn opened_log(proof: &CheckedDir) -> FileSetupLog {
    SetupLog::open(
        proof,
        FileLog::new(
            PathBuf::from(proof.path())
                .join("updates")
                .join("setup.log"),
        ),
        SystemClock,
    )
}

/// `check-dir`, then `updates\` is made, then the log is opened. Everything below uses `proof.path()`, the canonical folder
/// that was checked, never the text that was passed in.
fn checked(dir: &str) -> Result<(CheckedDir, FileSetupLog), checkdir::DirRefusal> {
    let proof = checkdir::check(&RealDirs, dir)?;
    // Folder trouble does not fail the check; it only means there is no log.
    let _ = folders::ensure_updates(
        &PathBuf::from(proof.path())
            .join("updates")
            .to_string_lossy(),
    );
    let log = opened_log(&proof);
    Ok((proof, log))
}

fn say(text: &str) {
    println!("{text}");
}

/// One log shared by the sequence and the port that it drives.
#[derive(Clone, Copy)]
pub struct Shared<'a>(&'a std::cell::RefCell<FileSetupLog>);

impl Sink for Shared<'_> {
    fn line(&mut self, phase: &str, message: &str) {
        self.0.borrow_mut().line(phase, message);
    }
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
    run(command).code()
}

/// Ends a command that has a log: the exit code and its meaning go to `setup.log`.
fn finish(log: &mut FileSetupLog, exit: Exit) -> Exit {
    log.exit(exit.code(), exit.sentence());
    exit
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
            let name = |p: &str| {
                handle::open_for_query(p)
                    .ok()
                    .and_then(|h| handle::final_path(&h))
            };
            let equal = |x: &str, y: &str| wide::compare_ordinal_ignore_case(x, y);
            let same = samefolder::same_with(name(&a).as_deref(), name(&b).as_deref(), &equal);
            if let Ok((_proof, mut log)) = checked(&b) {
                log.line(
                    "same-folder",
                    if same {
                        "the installer runs from the install folder: a reinstall"
                    } else {
                        "not the same folder"
                    },
                );
            }
            if same { Exit::Ok } else { Exit::NotSame }
        }
        Command::CheckDir { dir } => match checked(&dir) {
            Ok((proof, mut log)) => {
                log.line("check-dir", "folder accepted");
                say(&format!("ok {}", proof.path()));
                finish(&mut log, Exit::Ok)
            }
            Err(refusal) => {
                // Nothing is written for a refusal: the folder is the thing refused.
                let app = BAKED.product.unwrap_or("the app");
                say(&refusal.message(app, &dir));
                Exit::BadDir
            }
        },
        Command::Guard {
            dir,
            product,
            main,
            update,
            restart,
        } => {
            // The names of the app are compiled in; the options are only the fallback of a build without them.
            let (Some(product), Some(main)) = (
                BAKED.product.map(String::from).or(product),
                BAKED.main.map(String::from).or(main),
            ) else {
                eprintln!(
                    "paddock-setup: this build does not know the product and main binary names"
                );
                return Exit::Usage;
            };
            let mut file_log = dir.as_deref().and_then(|d| checked(d).ok()).map(|(_, l)| l);
            let mut none = NoLog;
            let log: &mut dyn Sink = match file_log.as_mut() {
                Some(l) => l,
                None => &mut none,
            };
            let decision = guard::decide(
                &Machine {
                    product: product.clone(),
                },
                &product,
                &main,
                update,
                restart,
                log,
            );
            // The include reads stdout: the message on the first line and, when the older copy is to be started again as the
            // desktop user (its PADDOCK_ABORT does that), a last `RESTART:<path>` line.
            let (exit, lines) = guard::outcome(&decision);
            lines.iter().for_each(|l| say(l));
            if let Some(l) = file_log.as_mut() {
                finish(l, exit);
            }
            exit
        }
        Command::StopAll { dir, images } => {
            let Ok((proof, mut log)) = checked(&dir) else {
                say("The install folder is not allowed.");
                return Exit::BadDir;
            };
            let t = log.begin("stop-all");
            let (exit, still_running) =
                stop::run(&mut StopAdapter, proof.path(), &images, &mut log);
            log.end("stop-all", t, exit.sentence());
            if exit == Exit::StillRunning {
                say(&stop::abort_message(
                    BAKED.product.unwrap_or("The app"),
                    &still_running,
                ));
            }
            finish(&mut log, exit)
        }
        Command::CleanRun { dir, scope } => {
            let Ok((proof, mut log)) = checked(&dir) else {
                return Exit::BadDir;
            };
            let done = runvalues::clean(
                &mut RegistryRuns,
                scope,
                proof.path(),
                BAKED.product.unwrap_or_default(),
                &mut log,
            );
            finish(
                &mut log,
                if done.failed > 0 {
                    Exit::Failed
                } else {
                    Exit::Ok
                },
            )
        }
        Command::CleanStaging { dir } => {
            let Ok((proof, mut log)) = checked(&dir) else {
                return Exit::BadDir;
            };
            let mut fs = staging_fs::UpdatesFolder {
                dir: PathBuf::from(proof.path()).join("updates"),
                skip: None,
            };
            let exit = match staging::clean(&mut fs, &mut log) {
                Ok(done) if done.failed == 0 => Exit::Ok,
                Ok(_) => Exit::Failed,
                Err(why) => {
                    log.line(
                        "clean-staging",
                        &format!("the updates folder could not be listed: {why}"),
                    );
                    Exit::Failed
                }
            };
            finish(&mut log, exit)
        }
        Command::ApplyUpdate {
            file,
            signature,
            version,
            size,
        } => apply_update(file, signature, version, size),
        Command::HoldInstalling { dir } => {
            let mut file_log = dir.as_deref().and_then(|d| checked(d).ok()).map(|(_, l)| l);
            let mut none = NoLog;
            let log: &mut dyn Sink = match file_log.as_mut() {
                Some(l) => l,
                None => &mut none,
            };
            let Some(id) = BAKED.identifier else {
                eprintln!("paddock-setup: this build does not know the app identifier");
                return Exit::Failed;
            };
            match hold::hold(id, log) {
                Ok(()) => Exit::Ok,
                Err(e) => {
                    log.line("hold-installing", &format!("failed: {e}"));
                    eprintln!("paddock-setup: {e}");
                    Exit::Failed
                }
            }
        }
    }
}

/// Prints a refusal for the caller (the updater reads the exit code and shows the sentence; a person running it sees it).
fn refused(r: crate::apply::Refusal) -> Exit {
    say(r.sentence());
    Exit::Refused(r)
}

fn apply_update(
    file: crate::localpath::LocalPath,
    signature: String,
    version: semver::Version,
    size: Option<u64>,
) -> Exit {
    use crate::apply::Refusal;
    let Ok(exe) = std::env::current_exe() else {
        return refused(Refusal::InstallDir);
    };
    let Some(instdir) = exe.parent().map(|p| p.to_string_lossy().into_owned()) else {
        return refused(Refusal::InstallDir);
    };
    // The log exists only after check-dir passed (a refusal returns before any write).
    let (proof, log) = match checked(&instdir) {
        Ok(x) => x,
        Err(why) => {
            eprintln!("paddock-setup: the install folder is not allowed: {why}");
            return refused(Refusal::InstallDir);
        }
    };
    let layout = crate::apply::Layout::new(proof.path());
    let (Some(main), Some(identifier), Some(product)) =
        (BAKED.main, BAKED.identifier, BAKED.product)
    else {
        eprintln!(
            "paddock-setup: this build does not know the app identifier, product and main binary"
        );
        return refused(Refusal::NoKey);
    };
    let key = BAKED
        .update_key
        .and_then(|k| paddock_updater::signature::UpdateKey::resolve([Some(k)]));
    let started = SystemClock.unix_ms();
    let request = crate::apply::Request {
        file,
        signature,
        version,
        size,
        product: product.to_string(),
        run: crate::apply::RunId::new(std::process::id(), started),
    };
    let cell = std::cell::RefCell::new(log);
    let result = {
        let mut port = apply::Real::new(
            layout.clone(),
            main.to_string(),
            identifier.to_string(),
            key,
            Shared(&cell),
        );
        crate::apply::run(&mut port, &layout, &request, &mut Shared(&cell))
    };
    let mut log = cell.into_inner();
    log.line(
        "apply-update",
        &format!(
            "finished in {} ms",
            SystemClock.unix_ms().saturating_sub(started)
        ),
    );
    match result {
        Ok(()) => finish(&mut log, Exit::Ok),
        Err(r) => {
            finish(&mut log, Exit::Refused(r));
            refused(r)
        }
    }
}

/// `clean-run` over the real registry.
struct RegistryRuns;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

fn views() -> [(&'static str, View); 2] {
    [("64", View::Wide), ("32", View::Narrow)]
}

fn values_of(key: &registry::Key) -> Vec<runvalues::RunValue> {
    key.string_values()
        .into_iter()
        .map(|(name, command)| runvalues::RunValue { name, command })
        .collect()
}

impl runvalues::RunPort for RegistryRuns {
    fn machine_values(&mut self) -> Vec<(&'static str, Vec<runvalues::RunValue>)> {
        views()
            .into_iter()
            .filter_map(|(label, view)| {
                registry::open_read(Root::Machine, RUN_KEY, view).map(|k| (label, values_of(&k)))
            })
            .collect()
    }
    fn delete_machine(&mut self, view: &str, name: &str) -> bool {
        views()
            .into_iter()
            .find(|(l, _)| *l == view)
            .and_then(|(_, v)| registry::open_write(Root::Machine, RUN_KEY, v))
            .is_some_and(|k| k.delete_value(name))
    }
    fn hives(&mut self) -> Vec<String> {
        registry::open_read(Root::Users, "", View::Wide)
            .map(|k| k.subkeys())
            .unwrap_or_default()
    }
    fn user_values(&mut self, hive: &str) -> Vec<runvalues::RunValue> {
        registry::open_read(Root::Users, &format!(r"{hive}\{RUN_KEY}"), View::Wide)
            .map(|k| values_of(&k))
            .unwrap_or_default()
    }
    fn delete_user(&mut self, hive: &str, name: &str) -> bool {
        registry::open_write(Root::Users, &format!(r"{hive}\{RUN_KEY}"), View::Wide)
            .is_some_and(|k| k.delete_value(name))
    }
}

/// `stop-all` over `paddock_proc::installed::stop_all_here` (plan S1), compiled in when that module is in the tree
/// (`has_proc_installed`, set by build.rs); otherwise the command fails with "no process stopper".
struct StopAdapter;

#[cfg(not(has_proc_installed))]
impl stop::StopPort for StopAdapter {
    fn stop_all(&mut self, _dir: &str, _images: &[String]) -> Result<stop::StopReport, String> {
        Err("this build of paddock-setup has no process stopper (paddock_proc::installed is not in this tree)".into())
    }
}

#[cfg(has_proc_installed)]
impl stop::StopPort for StopAdapter {
    fn stop_all(&mut self, dir: &str, images: &[String]) -> Result<stop::StopReport, String> {
        use paddock_proc::installed::{Bounds, Outcome, stop_all_here};
        let report = stop_all_here(std::path::Path::new(dir), images, &Bounds::default());
        let mut out = stop::StopReport {
            still_running: report.still_running.clone(),
            ..stop::StopReport::default()
        };
        for error in [&report.restart_manager_error, &report.privilege_error]
            .into_iter()
            .flatten()
        {
            out.notes.push(error.to_string());
        }
        for a in &report.actions {
            out.processes.push(crate::log::ProcessRecord {
                image: a.image.clone(),
                pid: a.id.map(|i| i.pid()),
                started_ms: a.id.map(|i| i.started().as_micros() / 1000),
                session: a.session,
                // The stopper no longer reports which step ended a process; its own log lines (ER-27) say.
                method: "stop-all".into(),
                outcome: match &a.outcome {
                    Outcome::Ended => "ended".into(),
                    Outcome::Skipped(why) => format!("skipped: {why}"),
                    Outcome::Failed(why) => format!("failed: {why}"),
                },
            });
        }
        Ok(out)
    }
}
