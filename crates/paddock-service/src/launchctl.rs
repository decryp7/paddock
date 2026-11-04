//! `launchctl` through the [`System`] port: the one reader of a job and the one `bootout`, shared by this crate's backend
//! and by apps that drive the repair themselves. Tests use [`crate::testing::FakeSystem`], never the real launchd.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::parse::{JobReport, parse_job};
use crate::paths::LAUNCHCTL;
use crate::port::System;

/// `launchctl print` of a service launchd does not know exits with this.
pub const NOT_FOUND: i32 = 113;

/// launchd's report of the job: `Some(default)` when launchd does not know it, `None` when it could not be asked (a
/// timeout, a permission error): unknown is never "absent".
pub fn job_report(sys: &mut dyn System, target: &str) -> Option<JobReport> {
    let out = sys.run(LAUNCHCTL, &["print", target]).ok()?;
    if out.success() {
        Some(parse_job(&out.stdout))
    } else if out.code == Some(NOT_FOUND) || out.stderr.contains("Could not find service") {
        Some(JobReport::default())
    } else {
        None
    }
}

/// `launchctl bootout <target>`; a job launchd does not know is fine.
pub fn bootout(sys: &mut dyn System, target: &str) -> Result<(), String> {
    match sys.run(LAUNCHCTL, &["bootout", target]) {
        Ok(o) if o.success() => Ok(()),
        Ok(o)
            if o.code == Some(3)
                || o.code == Some(NOT_FOUND)
                || o.stderr.contains("No such process") =>
        {
            Ok(())
        }
        Ok(o) => Err(format!("launchctl bootout: {}", o.reason())),
        Err(e) => Err(format!("launchctl bootout {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port::{Output, RunError};
    use crate::testing::FakeSystem;

    #[test]
    fn a_job_launchd_does_not_know_is_absent_and_one_it_cannot_be_asked_about_is_unknown() {
        let mut sys = FakeSystem::new();
        sys.answer("/bin/launchctl print gui/501/gone", Output::code(113));
        sys.on("/bin/launchctl print gui/501/denied", |_, _| {
            Ok(Output {
                code: Some(1),
                stderr: "Operation not permitted".into(),
                ..Output::default()
            })
        });
        sys.on("/bin/launchctl print gui/501/slow", |_, _| {
            Err(RunError::TimedOut("launchctl".into()))
        });
        sys.answer(
            "/bin/launchctl print gui/501/up",
            Output::ok("state = running\n"),
        );
        assert_eq!(
            job_report(&mut sys, "gui/501/gone"),
            Some(JobReport::default())
        );
        sys.on("/bin/launchctl print gui/501/named", |_, _| {
            Ok(Output {
                code: Some(1),
                stderr: "Could not find service \"x\" in domain for user gui: 501".into(),
                ..Output::default()
            })
        });
        assert_eq!(
            job_report(&mut sys, "gui/501/named"),
            Some(JobReport::default())
        );
        assert_eq!(job_report(&mut sys, "gui/501/denied"), None);
        assert_eq!(job_report(&mut sys, "gui/501/slow"), None);
        assert!(job_report(&mut sys, "gui/501/up").is_some_and(|j| j.running));
    }

    #[test]
    fn bootout_of_a_job_that_is_already_gone_is_fine_and_other_failures_are_reported() {
        let mut sys = FakeSystem::new();
        for (code, stderr) in [(0, ""), (3, ""), (113, ""), (1, "No such process")] {
            sys.on("/bin/launchctl bootout", move |_, _| {
                Ok(Output {
                    code: Some(code),
                    stderr: stderr.into(),
                    ..Output::default()
                })
            });
            assert_eq!(bootout(&mut sys, "gui/501/x"), Ok(()), "{code} {stderr}");
        }
        sys.answer(
            "/bin/launchctl bootout",
            Output {
                code: Some(5),
                stderr: "denied".into(),
                ..Output::default()
            },
        );
        assert_eq!(
            bootout(&mut sys, "gui/501/x"),
            Err("launchctl bootout: denied".into())
        );
        sys.on("/bin/launchctl bootout", |_, _| {
            Err(RunError::TimedOut("launchctl".into()))
        });
        assert!(bootout(&mut sys, "gui/501/x").is_err());
    }
}
