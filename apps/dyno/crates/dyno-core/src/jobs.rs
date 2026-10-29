//! Long operations as jobs (ER-17, ER-18, ER-19): what can be started, how a running job reports its phase and
//! progress, and what a finished one hands back. Pure wire types; the agent runs the jobs (on `paddock-jobs`, the
//! family's job mechanism) and the app polls [`Request::Jobs`](crate::protocol::Request::Jobs), the way it polls
//! the benchmarks. Starting a job answers at once; no request waits for the work, so no request needs a long timeout.
//!
//! Wire shape rules (postcard): externally tagged enums, no `flatten`, no internal tags.

use crate::backup::BackupReport;
use crate::extras::files::{FinderQuery, FinderResult};
use crate::issues::{IssuesOverview, IssuesQuery};
use crate::weblinks::{FileHashInfo, HashSource};
use serde::{Deserialize, Serialize};

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
}

impl JobSpec {
    /// The kind of job this starts.
    pub fn kind(&self) -> JobKind {
        match self {
            JobSpec::FindFiles { .. } => JobKind::FindFiles,
            JobSpec::RescanIssues { .. } => JobKind::RescanIssues,
            JobSpec::RefreshBackup => JobKind::RefreshBackup,
            JobSpec::FileHash { .. } => JobKind::FileHash,
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
}

impl JobKind {
    /// The title the person sees (sentence case).
    pub fn title(self) -> &'static str {
        match self {
            JobKind::FindFiles => "Finding files",
            JobKind::RescanIssues => "Reading system logs",
            JobKind::RefreshBackup => "Checking backups",
            JobKind::FileHash => "Hashing a file",
        }
    }
}

/// What a phase counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobUnit {
    /// Files, logs, tools.
    Items,
    /// Bytes read.
    Bytes,
}

/// One stage of a job, on its own scale. `total` is `None` while the size is not known: the count just grows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobPhase {
    /// A stable slug ("walk", "hash").
    pub id: String,
    /// What the person reads, sentence case ("Listing files").
    pub label: String,
    /// What `done` and `total` count.
    pub unit: JobUnit,
    /// So far.
    pub done: u64,
    /// In all, when known.
    pub total: Option<u64>,
}

/// Where a job stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum JobState {
    /// Waiting for a worker.
    Queued,
    /// Working; `phase` is `None` until the first stage starts.
    Running {
        /// The stage now.
        phase: Option<JobPhase>,
    },
    /// Finished; [`Request::JobResult`](crate::protocol::Request::JobResult) has the result.
    Done,
    /// Stopped by an error.
    Failed {
        /// What to fix, for the person.
        message: String,
    },
    /// Stopped by the person; nothing was changed.
    Cancelled,
}

impl JobState {
    /// Whether nothing follows.
    pub fn is_final(&self) -> bool {
        matches!(self, JobState::Done | JobState::Failed { .. } | JobState::Cancelled)
    }
}

/// One job as the app sees it. Rises in `seq` with every change; a consumer keeps the highest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    /// The job's id.
    pub id: u64,
    /// What kind.
    pub kind: JobKind,
    /// The title (see [`JobKind::title`]).
    pub title: String,
    /// What it works on (a folder, a window), for display only.
    pub subject: Option<String>,
    /// Where it stands.
    pub state: JobState,
    /// Rises with every change.
    pub seq: u64,
    /// Milliseconds since it was registered.
    pub elapsed_ms: u64,
    /// Milliseconds left, only when the pace so far makes it trustworthy.
    pub eta_ms: Option<u64>,
    /// False while a step that cannot be interrupted runs; Cancel then takes effect when that step ends.
    pub cancellable: bool,
    /// Why not, when `cancellable` is false.
    pub not_cancellable_reason: Option<String>,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extras::files::FinderMode;

    fn status(state: JobState) -> JobStatus {
        JobStatus {
            id: 7,
            kind: JobKind::FindFiles,
            title: JobKind::FindFiles.title().into(),
            subject: Some("/x".into()),
            state,
            seq: 3,
            elapsed_ms: 1200,
            eta_ms: None,
            cancellable: true,
            not_cancellable_reason: None,
        }
    }

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
    fn a_status_has_the_json_shape_of_api_types_and_survives_the_wire() {
        let running = status(JobState::Running {
            phase: Some(JobPhase {
                id: "walk".into(),
                label: "Listing files".into(),
                unit: JobUnit::Items,
                done: 12,
                total: None,
            }),
        });
        let j = serde_json::to_value(&running).unwrap();
        assert_eq!(j["state"]["running"]["phase"]["label"], "Listing files");
        assert_eq!(j["state"]["running"]["phase"]["total"], serde_json::Value::Null);
        assert_eq!(j["elapsedMs"], 1200);
        assert_eq!(j["notCancellableReason"], serde_json::Value::Null);
        for s in [
            running,
            status(JobState::Queued),
            status(JobState::Done),
            status(JobState::Cancelled),
            status(JobState::Failed { message: "no".into() }),
        ] {
            assert_eq!(round(&s), s);
        }
        assert_eq!(serde_json::to_value(JobState::Cancelled).unwrap(), "cancelled");
    }

    #[test]
    fn only_a_finished_job_is_final() {
        assert!(!JobState::Queued.is_final());
        assert!(!JobState::Running { phase: None }.is_final());
        assert!(JobState::Done.is_final() && JobState::Cancelled.is_final());
        assert!(JobState::Failed { message: String::new() }.is_final());
    }

    #[test]
    fn an_output_survives_the_wire() {
        let out = JobOutput::Backup(Box::default());
        assert_eq!(round(&out), out);
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
