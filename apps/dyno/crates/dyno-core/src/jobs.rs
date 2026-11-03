//! Long operations as jobs (ER-17, ER-18, ER-19): what can be started and what a finished one hands back. Pure wire
//! types. The agent runs the jobs on `paddock-jobs`, the family's job mechanism; a job's status travels as
//! [`WireSnapshot`](paddock_jobs::wire::WireSnapshot) (the postcard-safe form every agent-based app shares) and is pushed to
//! the app as [`Message::Job`](crate::protocol::Message::Job) events; the shell turns them into `paddock://jobs` events. Starting
//! a job answers at once; no request waits for the work, so no request needs a long timeout.
//!
//! Wire shape rules (postcard): externally tagged enums, no `flatten`, no internal tags.

use crate::backup::BackupReport;
use crate::extras::files::{FinderQuery, FinderResult};
use crate::extras::{boot::BootReport, sleep::SleepReport, software::SoftwareInventory, usage::UsageReport};
use crate::issues::{IssuesOverview, IssuesQuery};
use crate::threads::Threads;
use crate::weblinks::{FileHashInfo, HashSource};
use serde::{Deserialize, Serialize};

/// The most jobs that may run at once, whatever the setting says.
pub const MAX_JOB_WORKERS: u8 = 8;

/// `Settings.jobs`. Every key is optional and merged over the defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JobSettings {
    /// How many jobs run at once (the rest wait as queued): Auto (4) or 1 to [`MAX_JOB_WORKERS`]. Read when Dyno starts.
    pub workers: Threads,
}

impl JobSettings {
    /// The values clamped into their allowed range.
    pub fn clamped(self) -> JobSettings {
        JobSettings {
            workers: self.workers.clamped(MAX_JOB_WORKERS),
        }
    }

    /// How many jobs run at once.
    pub fn worker_count(self) -> usize {
        self.workers.resolve(AUTO_JOB_WORKERS, MAX_JOB_WORKERS)
    }
}

/// The number of jobs that run at once when the setting is Auto.
pub const AUTO_JOB_WORKERS: usize = 4;

/// Finished jobs the agent remembers (their status and, for a done job, its result).
pub const KEPT_FINISHED: usize = 16;

/// What to run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum JobSpec {
    /// Large and duplicate files in a folder the user chose.
    FindFiles {
        /// What and where.
        query: FinderQuery,
    },
    /// Reads the OS logs now.
    RescanIssues {
        /// The window the result is built for.
        query: IssuesQuery,
    },
    /// Checks the backup tools now.
    RefreshBackup,
    /// The SHA-256 of a program's file.
    FileHash {
        /// Which file.
        source: HashSource,
    },
    /// What keeps the computer awake and what woke it (runs OS tools).
    SleepBlockers,
    /// Recent boots and what slowed the last one (runs OS tools).
    BootAnalysis,
    /// Network traffic and energy per app (runs OS tools).
    AppUsage,
    /// The installed software (runs the package tools).
    SoftwareInventory,
    /// Turns the privileged helper on, repairs, updates or turns it off (ADR 0027).
    Helper {
        /// What to do.
        action: crate::helper_wire::HelperAction,
    },
    /// Fetches one list subscription now (a download job with phases and Cancel), then rebuilds the index.
    UpdateList {
        /// The list's id.
        id: String,
    },
    /// Fetches the DB-IP location databases now (monthly files; at most one attempt a day).
    UpdateGeo,
    /// Rebuilds the match index from the user's lists (after an edit).
    RebuildIntel,
    /// Downloads the pinned tool (the user clicked "Download"), checks it and has the helper install it.
    InstallTool {
        /// Which tool.
        tool: crate::helper_requests::ToolKind,
    },
    /// Sends a Wake-on-LAN wake to a saved computer; `test` also waits for it to answer (ADR 0032).
    WolWake {
        /// The saved computer.
        id: u64,
        /// Wait for an answer and say whether it woke.
        test: bool,
    },
}

impl JobSpec {
    /// The kind of job this starts.
    pub fn kind(&self) -> JobKind {
        match self {
            JobSpec::FindFiles { .. } => JobKind::FindFiles,
            JobSpec::RescanIssues { .. } => JobKind::RescanIssues,
            JobSpec::RefreshBackup => JobKind::RefreshBackup,
            JobSpec::FileHash { .. } => JobKind::FileHash,
            JobSpec::SleepBlockers => JobKind::SleepBlockers,
            JobSpec::BootAnalysis => JobKind::BootAnalysis,
            JobSpec::AppUsage => JobKind::AppUsage,
            JobSpec::SoftwareInventory => JobKind::SoftwareInventory,
            JobSpec::InstallTool { .. } => JobKind::InstallTool,
            JobSpec::Helper { .. } => JobKind::Helper,
            JobSpec::UpdateList { .. } => JobKind::UpdateList,
            JobSpec::UpdateGeo => JobKind::UpdateGeo,
            JobSpec::RebuildIntel => JobKind::RebuildIntel,
            JobSpec::WolWake { .. } => JobKind::WolWake,
        }
    }
}

/// The kinds of job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    /// Large and duplicate files.
    FindFiles,
    /// Reading the OS logs.
    RescanIssues,
    /// Checking the backup tools.
    RefreshBackup,
    /// Hashing a file.
    FileHash,
    /// Reading what keeps the computer awake.
    SleepBlockers,
    /// Analysing the boots.
    BootAnalysis,
    /// Reading network and energy use per app.
    AppUsage,
    /// Listing the installed software.
    SoftwareInventory,
    /// Setting up the privileged helper.
    Helper,
    /// Updating one list subscription.
    UpdateList,
    /// Updating the location databases.
    UpdateGeo,
    /// Rebuilding the match index of the user's lists.
    RebuildIntel,
    /// Downloading the pinned disk-health tool and handing it to the helper.
    InstallTool,
    /// Waking a computer over the network.
    WolWake,
}

impl JobKind {
    /// The title the person sees (sentence case).
    pub fn title(self) -> &'static str {
        match self {
            JobKind::FindFiles => "Finding files",
            JobKind::RescanIssues => "Reading system logs",
            JobKind::RefreshBackup => "Checking backups",
            JobKind::FileHash => "Hashing a file",
            JobKind::SleepBlockers => "Reading sleep blockers",
            JobKind::BootAnalysis => "Analysing boots",
            JobKind::AppUsage => "Reading app usage",
            JobKind::SoftwareInventory => "Listing installed software",
            JobKind::InstallTool => "Installing the disk-health tool",
            JobKind::Helper => "Setting up administrator readings",
            JobKind::UpdateList => "Updating a list",
            JobKind::UpdateGeo => "Updating the location databases",
            JobKind::RebuildIntel => "Building the match index",
            JobKind::WolWake => "Waking a computer",
        }
    }

    /// The slug the job goes by in the job registry and on the `paddock://jobs` event (what the UI groups and picks icons by).
    pub fn slug(self) -> &'static str {
        match self {
            JobKind::FindFiles => "find-files",
            JobKind::RescanIssues => "rescan-issues",
            JobKind::RefreshBackup => "refresh-backup",
            JobKind::FileHash => "file-hash",
            JobKind::SleepBlockers => "sleep-blockers",
            JobKind::BootAnalysis => "boot-analysis",
            JobKind::AppUsage => "app-usage",
            JobKind::SoftwareInventory => "software-inventory",
            JobKind::InstallTool => "install-tool",
            JobKind::Helper => "helper",
            JobKind::UpdateList => "update-list",
            JobKind::UpdateGeo => "update-geo",
            JobKind::RebuildIntel => "rebuild-intel",
            JobKind::WolWake => "wol-wake",
        }
    }

    /// The kind with this slug.
    pub fn from_slug(slug: &str) -> Option<JobKind> {
        [
            JobKind::FindFiles,
            JobKind::RescanIssues,
            JobKind::RefreshBackup,
            JobKind::FileHash,
            JobKind::SleepBlockers,
            JobKind::BootAnalysis,
            JobKind::AppUsage,
            JobKind::SoftwareInventory,
            JobKind::Helper,
            JobKind::UpdateList,
            JobKind::UpdateGeo,
            JobKind::RebuildIntel,
            JobKind::InstallTool,
            JobKind::WolWake,
        ]
        .into_iter()
        .find(|k| k.slug() == slug)
    }
}

/// What a done job hands back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobOutput {
    /// From [`JobSpec::FindFiles`].
    Finder(Box<FinderResult>),
    /// From [`JobSpec::RescanIssues`].
    Issues(Box<IssuesOverview>),
    /// From [`JobSpec::RefreshBackup`].
    Backup(Box<BackupReport>),
    /// From [`JobSpec::FileHash`].
    FileHash(FileHashInfo),
    /// From [`JobSpec::SleepBlockers`].
    Sleep(Box<SleepReport>),
    /// From [`JobSpec::BootAnalysis`].
    Boot(Box<BootReport>),
    /// From [`JobSpec::AppUsage`].
    AppUsage(Box<UsageReport>),
    /// From [`JobSpec::SoftwareInventory`].
    Software(Box<SoftwareInventory>),
    /// From [`JobSpec::InstallTool`]: what the helper holds of the tool now.
    Tool(Box<crate::helper_wire::ToolStatus>),
    /// From [`JobSpec::Helper`]: the helper as it is now.
    Helper(Box<crate::helper_wire::HelperStatus>),
    /// From [`JobSpec::UpdateList`], [`JobSpec::UpdateGeo`] and [`JobSpec::RebuildIntel`]: the lists page after the job.
    Intel(Box<crate::intel_lists::IntelStatus>),
    /// From [`JobSpec::WolWake`].
    Wol(Box<crate::wol::WolOutcome>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extras::files::FinderMode;

    fn round<T: Serialize + serde::de::DeserializeOwned>(v: &T) -> T {
        postcard::from_bytes(&postcard::to_stdvec(v).unwrap()).unwrap()
    }

    #[test]
    fn a_spec_has_the_json_shape_of_api_types_and_its_kind() {
        let spec = JobSpec::FindFiles {
            query: FinderQuery {
                folder: "/x".into(),
                mode: FinderMode::Both,
                min_bytes: 5,
            },
        };
        assert_eq!(
            serde_json::to_value(&spec).unwrap(),
            serde_json::json!({"findFiles": {"query": {"folder": "/x", "mode": "both", "minBytes": 5}}})
        );
        assert_eq!(spec.kind(), JobKind::FindFiles);
        assert_eq!(serde_json::to_value(JobSpec::RefreshBackup).unwrap(), "refreshBackup");
        let hash: JobSpec = serde_json::from_value(serde_json::json!({"fileHash": {"source": {"path": "/a"}}})).unwrap();
        assert_eq!(hash.kind(), JobKind::FileHash);
        let rescan: JobSpec = serde_json::from_value(serde_json::json!({"rescanIssues": {"query": {"days": 7}}})).unwrap();
        assert_eq!(rescan.kind(), JobKind::RescanIssues);
    }

    #[test]
    fn the_job_workers_are_auto_or_a_validated_count() {
        assert_eq!(JobSettings::default().worker_count(), 4);
        assert_eq!(
            JobSettings {
                workers: Threads::Fixed(2)
            }
            .worker_count(),
            2
        );
        assert_eq!(
            JobSettings {
                workers: Threads::Fixed(99)
            }
            .worker_count(),
            8
        );
        assert_eq!(
            JobSettings {
                workers: Threads::Fixed(0)
            }
            .clamped()
            .workers,
            Threads::Fixed(1)
        );
        let s: JobSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, JobSettings::default());
    }

    #[test]
    fn every_kind_has_a_unique_slug_that_finds_it_again() {
        let specs = [
            JobSpec::RefreshBackup,
            JobSpec::SleepBlockers,
            JobSpec::BootAnalysis,
            JobSpec::AppUsage,
            JobSpec::SoftwareInventory,
            JobSpec::WolWake { id: 1, test: false },
        ];
        for spec in specs {
            assert_eq!(JobKind::from_slug(spec.kind().slug()), Some(spec.kind()));
            assert!(!spec.kind().title().is_empty());
        }
        assert_eq!(JobKind::from_slug("nope"), None);
        assert_eq!(serde_json::to_value(JobSpec::SleepBlockers).unwrap(), "sleepBlockers");
    }

    #[test]
    fn an_output_survives_the_wire() {
        let out = JobOutput::Backup(Box::default());
        assert_eq!(round(&out), out);
        assert_eq!(round(&JobOutput::Sleep(Box::default())), JobOutput::Sleep(Box::default()));
        assert_eq!(
            serde_json::to_value(JobOutput::FileHash(FileHashInfo {
                path: "/a".into(),
                bytes: 1,
                sha256: "0".repeat(64)
            }))
            .unwrap()["fileHash"]["bytes"],
            1
        );
    }
}
