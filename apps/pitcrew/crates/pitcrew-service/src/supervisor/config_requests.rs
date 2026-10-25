//! The supervisor's handling of a runner's `config.yaml` (ADR 0011): reading it for the form, previewing a change,
//! saving it with a backup, restarting the runner after its job, and checking the declared labels afterwards.
//!
//! The text of the file is never written to a log: only the changed keys and counts are.

use super::*;
use pitcrew_core::notify::{NotificationKind, Severity};
use pitcrew_core::runner_config::{fingerprint, labels_mismatch, preview, view};

impl Supervisor {
    fn runner_config_path(&self, runner: &RunnerName) -> Option<PathBuf> {
        self.runners.get(runner).map(|r| layout::runner_config(&r.cfg))
    }

    /// `Request::GetRunnerConfig`.
    pub(super) fn get_runner_config(&self, runner: &RunnerName) -> Response {
        let Some(path) = self.runner_config_path(runner) else {
            return unknown(runner);
        };
        let text = match runner_files::read(&path) {
            Ok(t) => t,
            Err(e) => return err(ErrorCode::BadRequest, e.to_string()),
        };
        let restart_pending = self
            .runners
            .get(runner)
            .is_some_and(|r| r.ctl.pending() == Some(pitcrew_core::controller::Pending::Restart));
        Response::RunnerConfig {
            path: path.display().to_string(),
            fingerprint: fingerprint(&text),
            view: view(&text),
            backups: runner_files::backups(&path),
            restart_pending,
            text,
        }
    }

    /// `Request::PreviewRunnerConfig`.
    pub(super) fn preview_runner_config(&self, runner: &RunnerName, edit: &ConfigEdit) -> Response {
        let Some(path) = self.runner_config_path(runner) else {
            return unknown(runner);
        };
        match runner_files::read(&path) {
            Ok(current) => Response::RunnerConfigPreview {
                preview: preview(&current, edit),
            },
            Err(e) => err(ErrorCode::BadRequest, e.to_string()),
        }
    }

    /// `Request::SaveRunnerConfig`.
    pub(super) fn save_runner_config(&mut self, runner: &RunnerName, edit: &ConfigEdit, base: &str, now: Timestamp) -> Response {
        let Some(path) = self.runner_config_path(runner) else {
            return unknown(runner);
        };
        let current = match runner_files::read(&path) {
            Ok(t) => t,
            Err(e) => return err(ErrorCode::BadRequest, e.to_string()),
        };
        // Validate again here, whatever the app checked (the service never trusts the app).
        if fingerprint(&current) != base {
            return err(
                ErrorCode::Busy,
                "the runner's configuration changed on disk after you opened it; reload it and make your change again",
            );
        }
        let p = preview(&current, edit);
        if let Some(first) = p.issues.iter().find(|i| i.severity == pitcrew_core::runner_schema::Severity::Error) {
            let where_ = if first.path.is_empty() {
                String::new()
            } else {
                format!("{}: ", first.path)
            };
            return err(
                ErrorCode::BadRequest,
                format!("the configuration was not saved. {where_}{}", first.message),
            );
        }
        if !p.changed {
            return Response::RunnerConfigSaved {
                backup: None,
                restart: RestartPlan::NotNeeded,
                fingerprint: fingerprint(&current),
                changed_keys: vec![],
            };
        }
        let saved = match runner_files::save(&path, &p.text, now) {
            Ok(s) => s,
            Err(e) => return err(ErrorCode::Internal, format!("the configuration could not be saved: {e}")),
        };
        let mut plan = RestartPlan::NotNeeded;
        if let Some(r) = self.runners.get_mut(runner) {
            if p.restart_needed {
                plan = if r.ctl.process_alive() && r.ctl.wanted() {
                    RestartPlan::AfterJob
                } else {
                    RestartPlan::NotRunning
                };
                r.label_check = (!p.labels.is_empty()).then(|| LabelCheck {
                    expected: p.labels.clone(),
                    after_generation: r.generation,
                });
                let keys = if p.changed_keys.is_empty() {
                    "layout".to_string()
                } else {
                    p.changed_keys.join(", ")
                };
                r.log.supervisor(
                    now,
                    &format!(
                        "the runner's configuration was saved from Pitcrew ({keys}); a backup was made{}",
                        if plan == RestartPlan::AfterJob {
                            "; the runner restarts after its current job"
                        } else {
                            ""
                        }
                    ),
                );
            } else {
                r.log.supervisor(
                    now,
                    "the runner's configuration was saved from Pitcrew (comments or layout only; no restart needed)",
                );
            }
        }
        self.state.runners.entry(runner.to_string()).or_default().history.push(
            now,
            HistoryKind::ConfigSaved {
                restart: plan == RestartPlan::AfterJob,
                changed: p.changed_keys.clone(),
            },
        );
        self.dirty = true;
        if plan == RestartPlan::AfterJob {
            // The controller's own "restart after the current job" (the same as the Restart button).
            let _ = self.control(runner, Input::Restart, now);
        }
        Response::RunnerConfigSaved {
            backup: saved.backup,
            restart: plan,
            fingerprint: fingerprint(&p.text),
            changed_keys: p.changed_keys,
        }
    }

    /// The restarted runner declared itself: do its labels match the file?
    pub(super) fn verify_labels(&mut self, runner: &RunnerName, declared: &[String], generation: u64, now: Timestamp) {
        let Some(r) = self.runners.get_mut(runner) else { return };
        let Some(check) = r.label_check.take_if(|c| generation > c.after_generation) else {
            return;
        };
        match labels_mismatch(&check.expected, declared) {
            None => {
                r.log.supervisor(
                    now,
                    &format!("the runner declared the labels from its configuration: {}", declared.join(", ")),
                );
                let labels = declared.to_vec();
                self.state
                    .runners
                    .entry(runner.to_string())
                    .or_default()
                    .history
                    .push(now, HistoryKind::LabelsVerified { labels });
            }
            Some(m) => {
                let text = format!(
                    "the runner declared [{}] but its configuration asks for [{}]{}{}",
                    declared.join(", "),
                    check.expected.join(", "),
                    if m.missing.is_empty() {
                        String::new()
                    } else {
                        format!("; missing: {}", m.missing.join(", "))
                    },
                    if m.unexpected.is_empty() {
                        String::new()
                    } else {
                        format!("; not in the file: {}", m.unexpected.join(", "))
                    },
                );
                r.log.supervisor(now, &text);
                self.state.runners.entry(runner.to_string()).or_default().history.push(
                    now,
                    HistoryKind::LabelsMismatch {
                        expected: check.expected,
                        declared: declared.to_vec(),
                    },
                );
                self.notify(Notification {
                    kind: NotificationKind::LabelMismatch,
                    severity: Severity::Warning,
                    runner: runner.clone(),
                    title: format!("{runner} declared other labels than configured"),
                    body: "After the restart the runner's labels differ from its configuration file. If it registered with its own labels, the registration may need updating.".into(),
                    at: now,
                });
            }
        }
        self.dirty = true;
    }
}
