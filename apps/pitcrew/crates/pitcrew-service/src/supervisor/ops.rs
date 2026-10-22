//! The supervisor's housekeeping requests that are not about one runner's process: the container engine and the
//! Actions cache folders (ADR 0016). They run on a worker thread so the supervisor loop never waits for a prune; a
//! prune or a trim is refused while any job runs.

use super::*;
use crate::hygiene;
use pitcrew_core::runner_settings::RunnerSettings;

impl Supervisor {
    fn any_job_running(&self, now: Timestamp) -> bool {
        self.runners
            .values()
            .any(|r| r.jobs.running(now) || !matches!(r.update.phase(), UpdatePhase::Idle))
    }

    /// The engine and cache requests (`None`: the worker answers).
    pub(super) fn ops_request(&mut self, req: Request, reply: Reply, now: Timestamp) -> Option<(Reply, Response)> {
        let path = (self.env.base_path)();
        let home = hygiene::home_dir();
        let busy_text =
            "a job is running; Pitcrew will not remove anything while a runner works. Try again when it is idle, or drain the runner first";
        match req {
            Request::EngineReport => {
                std::thread::spawn(move || {
                    let _ = reply.send(Response::EngineReport {
                        report: hygiene::report(path.as_deref(), home.as_deref(), now),
                    });
                });
                None
            }
            Request::EnginePrunePreview { selection } => {
                std::thread::spawn(move || {
                    let _ = reply.send(match hygiene::preview(path.as_deref(), &selection) {
                        Ok(preview) => Response::EnginePreview { preview },
                        Err(e) => err(ErrorCode::BadRequest, e),
                    });
                });
                None
            }
            Request::EnginePrune { selection } => {
                if self.any_job_running(now) {
                    return Some((reply, err(ErrorCode::Busy, busy_text)));
                }
                if let Err(e) = selection.validate() {
                    return Some((reply, err(ErrorCode::BadRequest, e)));
                }
                self.log_service(now, &format!("pruning the container engine on request: {:?}", selection.kinds));
                std::thread::spawn(move || {
                    let _ = reply.send(match hygiene::prune(path.as_deref(), &selection, now) {
                        Ok(result) => Response::EnginePruned { result },
                        Err(e) => err(ErrorCode::BadRequest, e),
                    });
                });
                None
            }
            Request::CacheReport { runner } => {
                let Some((settings, dir)) = self.cache_inputs(&runner) else {
                    return Some((reply, unknown(&runner)));
                };
                let free = self.probe.disk_space(&dir, now).map(|(f, _)| f);
                std::thread::spawn(move || {
                    let _ = reply.send(match hygiene::cache_report(&settings, &dir, home.as_deref(), free) {
                        Ok(cache) => Response::CacheReport { cache },
                        Err(e) => err(ErrorCode::BadRequest, e),
                    });
                });
                None
            }
            Request::CachePreview { runner, limits } => self.cache_plan_request(&runner, limits, false, reply, now, home),
            Request::CacheClean { runner, limits } => {
                if self.any_job_running(now) {
                    return Some((reply, err(ErrorCode::Busy, busy_text)));
                }
                self.cache_plan_request(&runner, limits, true, reply, now, home)
            }
            _ => Some((reply, err(ErrorCode::Internal, "not an engine or cache request"))),
        }
    }

    /// A cache preview (`clean` false) or trim on a worker thread.
    fn cache_plan_request(
        &mut self,
        runner: &RunnerName,
        limits: pitcrew_core::hygiene::CacheLimits,
        clean: bool,
        reply: Reply,
        now: Timestamp,
        home: Option<PathBuf>,
    ) -> Option<(Reply, Response)> {
        let Some((settings, runner_dir)) = self.cache_inputs(runner) else {
            return Some((reply, unknown(runner)));
        };
        let Some((dir, _)) = hygiene::cache_dir_of(&settings, &runner_dir, home.as_deref()) else {
            return Some((
                reply,
                err(
                    ErrorCode::BadRequest,
                    "the cache folder is not known: this account has no home folder",
                ),
            ));
        };
        if clean {
            self.log_service(now, &format!("trimming the cache of {runner} on request"));
        }
        std::thread::spawn(move || {
            let answer = match hygiene::cache_plan(&dir, &limits, home.as_deref(), now) {
                Err(e) => err(ErrorCode::BadRequest, e),
                Ok(plan) if !clean => Response::CachePreview { plan },
                Ok(plan) => {
                    let failures = hygiene::cache_clean(&dir, &plan);
                    Response::CacheCleaned { plan, failures }
                }
            };
            let _ = reply.send(answer);
        });
        None
    }

    /// The runner's settings and folder.
    fn cache_inputs(&self, runner: &RunnerName) -> Option<(RunnerSettings, PathBuf)> {
        let r = self.runners.get(runner)?;
        let text = runner_files::read(&layout::runner_config(&r.cfg)).unwrap_or_default();
        let settings = RunnerSettings::from_text(&text).map(|(s, _)| s).unwrap_or_default();
        Some((settings, r.cfg.dir.clone()))
    }
}
