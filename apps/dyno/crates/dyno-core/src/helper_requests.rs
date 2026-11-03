//! The request side of the helper's wire (ADR 0027 section 5): the typed arguments of every [`HelperRequest`], their caps,
//! and the re-validation the helper runs after decoding (parse, don't validate: nothing here is a free-form string).
//!
//! [`HelperRequest`]: crate::helper_wire::HelperRequest

use serde::{Deserialize, Serialize};

/// The most processes one `Processes` page carries.
pub const MAX_PROCESS_PAGE: u16 = 1_024;
/// The page size when the caller has no preference.
pub const DEFAULT_PROCESS_PAGE: u16 = 512;
/// The most counter ids one `Counters` request names.
pub const MAX_COUNTER_IDS: usize = 8;
/// The most disks one `Smart` read visits.
pub const MAX_SMART_DISKS: u8 = 32;
/// The most log lines one `LogTail` reply carries.
pub const MAX_LOG_REPLY: u32 = 500;
/// The most bytes an `InstallTool` request streams (under the 4 MB frame; the helper also checks the pinned size).
pub const MAX_TOOL_BYTES: usize = 2 * 1024 * 1024;

/// How urgent a read is (Paddock ADR 0024 section 1): a visible view or history sampling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestClass {
    /// Something on screen waits for it.
    User,
    /// History sampling.
    Background,
}

/// One page of the process list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessQuery {
    /// The paging cursor: the last pid of the previous page (pid order).
    pub after: Option<u32>,
    /// Items wanted, 1 to [`MAX_PROCESS_PAGE`].
    pub limit: u16,
    /// The class the helper runs the read at.
    pub class: RequestClass,
}

impl Default for ProcessQuery {
    fn default() -> Self {
        ProcessQuery {
            after: None,
            limit: DEFAULT_PROCESS_PAGE,
            class: RequestClass::User,
        }
    }
}

/// The readings that need the helper (a closed list; ADR 0022 section 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CounterId {
    /// Temperatures of the machine's thermal zones.
    Thermal,
    /// CPU package energy.
    CpuEnergy,
    /// Fan speeds.
    Fans,
}

/// A `Counters` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterQuery {
    /// The readings wanted: 1 to [`MAX_COUNTER_IDS`], each once.
    pub ids: Vec<CounterId>,
    /// The class the helper runs the read at.
    pub class: RequestClass,
}

/// A `Smart` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmartQuery {
    /// The most disks to read, 1 to [`MAX_SMART_DISKS`]. Disks come from the helper's own scan, never from the caller.
    pub max_disks: u8,
}

impl Default for SmartQuery {
    fn default() -> Self {
        SmartQuery {
            max_disks: MAX_SMART_DISKS,
        }
    }
}

/// The tools the helper can hold in its tools folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolKind {
    /// smartmontools (GPL-2.0-or-later, downloaded by the user, run as a separate program; ADR 0027 section 7).
    Smartmontools,
}

/// The Windows extras the helper reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExtrasKind {
    /// `powercfg /requests`.
    WakeRequests,
    /// `wbadmin get versions`.
    BackupVersions,
}

/// Why a decoded request is refused before it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestError(pub String);

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn refuse<T>(why: impl Into<String>) -> Result<T, RequestError> {
    Err(RequestError(why.into()))
}

impl ProcessQuery {
    /// Checks the page size.
    pub fn validate(&self) -> Result<(), RequestError> {
        if self.limit == 0 || self.limit > MAX_PROCESS_PAGE {
            return refuse(format!("A process page holds 1 to {MAX_PROCESS_PAGE} items, not {}.", self.limit));
        }
        Ok(())
    }
}

impl CounterQuery {
    /// Checks the number of ids and that none repeats.
    pub fn validate(&self) -> Result<(), RequestError> {
        if self.ids.is_empty() || self.ids.len() > MAX_COUNTER_IDS {
            return refuse(format!(
                "A counters request names 1 to {MAX_COUNTER_IDS} readings, not {}.",
                self.ids.len()
            ));
        }
        let mut seen = self.ids.clone();
        seen.sort_by_key(|id| *id as u8);
        seen.dedup();
        if seen.len() != self.ids.len() {
            return refuse("A counters request names each reading once.");
        }
        Ok(())
    }
}

impl SmartQuery {
    /// Checks the disk cap.
    pub fn validate(&self) -> Result<(), RequestError> {
        if self.max_disks == 0 || self.max_disks > MAX_SMART_DISKS {
            return refuse(format!("A SMART read visits 1 to {MAX_SMART_DISKS} disks, not {}.", self.max_disks));
        }
        Ok(())
    }
}

/// Checks the bytes of an `InstallTool` request (the helper then checks the pinned size and SHA-256 itself).
pub fn validate_tool_bytes(len: usize) -> Result<(), RequestError> {
    if len == 0 || len > MAX_TOOL_BYTES {
        return refuse(format!("A tool package is 1 to {MAX_TOOL_BYTES} bytes, not {len}."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_pages_are_capped_on_both_sides() {
        assert!(ProcessQuery::default().validate().is_ok());
        for limit in [1, MAX_PROCESS_PAGE] {
            assert!(ProcessQuery {
                limit,
                ..Default::default()
            }
            .validate()
            .is_ok());
        }
        for limit in [0, MAX_PROCESS_PAGE + 1, u16::MAX] {
            assert!(
                ProcessQuery {
                    limit,
                    ..Default::default()
                }
                .validate()
                .is_err(),
                "{limit}"
            );
        }
    }

    #[test]
    fn counters_need_one_to_eight_distinct_ids() {
        let q = |ids: Vec<CounterId>| CounterQuery {
            ids,
            class: RequestClass::Background,
        };
        assert!(q(vec![CounterId::Thermal]).validate().is_ok());
        assert!(q(vec![CounterId::Thermal, CounterId::CpuEnergy, CounterId::Fans])
            .validate()
            .is_ok());
        assert!(q(vec![]).validate().is_err());
        assert!(q(vec![CounterId::Fans, CounterId::Fans]).validate().is_err());
        assert!(q(vec![CounterId::Fans; MAX_COUNTER_IDS + 1]).validate().is_err());
    }

    #[test]
    fn smart_and_tool_sizes_have_a_cap_and_a_floor() {
        assert!(SmartQuery::default().validate().is_ok());
        assert!(SmartQuery { max_disks: 0 }.validate().is_err());
        assert!(SmartQuery {
            max_disks: MAX_SMART_DISKS + 1
        }
        .validate()
        .is_err());
        assert!(validate_tool_bytes(1).is_ok());
        assert!(validate_tool_bytes(MAX_TOOL_BYTES).is_ok());
        assert!(validate_tool_bytes(0).is_err());
        assert!(validate_tool_bytes(MAX_TOOL_BYTES + 1).is_err());
        const { assert!(MAX_TOOL_BYTES < crate::helper_wire::MAX_FRAME) };
    }
}
