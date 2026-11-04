//! `stop-all`: the command around `paddock_proc::installed::stop_all` (ADR 0053 section 3). The stopping is
//! `paddock-proc`'s (plan S1); this module builds the image list, runs the port, logs every process acted on and maps the
//! report to the exit code.

use crate::exit::Exit;
use crate::log::{ProcessRecord, Sink};

/// `paddock-setup.exe` itself is always in the list (a hand-off that is still exiting); the stopper never ends its own pid.
pub const SELF_IMAGE: &str = "paddock-setup.exe";

/// What the stopper found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StopReport {
    /// Every process acted on or skipped.
    pub processes: Vec<ProcessRecord>,
    /// Image names of listed programs from the folder that still run.
    pub still_running: Vec<String>,
}

/// The stopper (`paddock_proc::installed::stop_all` in the program).
pub trait StopPort {
    /// Stops every program of `images` that runs from `dir`, in every session.
    fn stop_all(&mut self, dir: &str, images: &[String]) -> Result<StopReport, String>;
}

/// The list to stop: `images` plus [`SELF_IMAGE`], without duplicates (case-insensitive), in order.
pub fn image_list(images: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for image in images.iter().map(String::as_str).chain(std::iter::once(SELF_IMAGE)) {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(image)) {
            out.push(image.to_string());
        }
    }
    out
}

/// Runs the stop and returns the exit.
pub fn run(port: &mut dyn StopPort, dir: &str, images: &[String], log: &mut dyn Sink) -> Exit {
    let list = image_list(images);
    log.line("stop-all", &format!("stopping {} program name(s) from the install folder", list.len()));
    match port.stop_all(dir, &list) {
        Err(reason) => {
            log.line("stop-all", &format!("failed: {reason}"));
            Exit::Failed
        }
        Ok(report) => {
            for rec in &report.processes {
                log.process(rec);
            }
            if report.still_running.is_empty() {
                log.line("stop-all", "done: nothing listed runs from the install folder");
                Exit::Ok
            } else {
                log.line("stop-all", &format!("still running: {}", report.still_running.join(", ")));
                Exit::StillRunning
            }
        }
    }
}

/// The installer's message for exit 11.
pub fn abort_message(app: &str, still_running: &[String]) -> String {
    format!(
        "{app} could not be closed for every signed-in user. Ask them to quit {app}, then run this installer again. Still running: {}.",
        still_running.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Lines(Vec<String>);
    impl Sink for Lines {
        fn line(&mut self, phase: &str, message: &str) {
            self.0.push(format!("{phase}: {message}"));
        }
    }
    struct Fake(Result<StopReport, String>, Vec<(String, Vec<String>)>);
    impl StopPort for Fake {
        fn stop_all(&mut self, dir: &str, images: &[String]) -> Result<StopReport, String> {
            self.1.push((dir.into(), images.to_vec()));
            self.0.clone()
        }
    }
    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn image_list_adds_self_once() {
        assert_eq!(image_list(&s(&["dyno.exe", "Dyno-Agent.exe"])), s(&["dyno.exe", "Dyno-Agent.exe", "paddock-setup.exe"]));
        assert_eq!(image_list(&s(&["PADDOCK-SETUP.exe", "a.exe", "A.EXE"])), s(&["PADDOCK-SETUP.exe", "a.exe"]));
        assert_eq!(image_list(&[]), s(&["paddock-setup.exe"]));
    }

    #[test]
    fn nothing_running_is_exit_ok_and_the_port_gets_the_full_list() {
        let mut port = Fake(Ok(StopReport::default()), vec![]);
        let mut log = Lines::default();
        assert_eq!(run(&mut port, r"C:\Program Files\Dyno", &s(&["dyno.exe"]), &mut log), Exit::Ok);
        assert_eq!(port.1, vec![(r"C:\Program Files\Dyno".to_string(), s(&["dyno.exe", "paddock-setup.exe"]))]);
        assert!(log.0.iter().any(|l| l.starts_with("stop-all: done")));
    }

    #[test]
    fn something_still_running_is_exit_11_and_named() {
        let rec = ProcessRecord { image: "dyno.exe".into(), pid: 9, started_ms: 1, session: 3, method: "by-handle".into(), outcome: "still running".into() };
        let mut port = Fake(Ok(StopReport { processes: vec![rec], still_running: s(&["dyno.exe"]) }), vec![]);
        let mut log = Lines::default();
        let exit = run(&mut port, "D", &s(&["dyno.exe"]), &mut log);
        assert_eq!(exit, Exit::StillRunning);
        assert_eq!(exit.code(), 11);
        assert!(log.0.iter().any(|l| l.contains("dyno.exe") && l.contains("still running")));
    }

    #[test]
    fn a_port_error_is_a_failure_with_its_reason_logged() {
        let mut port = Fake(Err("restart manager session failed".into()), vec![]);
        let mut log = Lines::default();
        assert_eq!(run(&mut port, "D", &[], &mut log), Exit::Failed);
        assert!(log.0.iter().any(|l| l.contains("restart manager session failed")));
    }

    #[test]
    fn the_abort_message_names_the_app_and_programs() {
        let m = abort_message("Dyno", &s(&["dyno.exe", "dyno-agent.exe"]));
        assert_eq!(m, "Dyno could not be closed for every signed-in user. Ask them to quit Dyno, then run this installer again. Still running: dyno.exe, dyno-agent.exe.");
    }
}
