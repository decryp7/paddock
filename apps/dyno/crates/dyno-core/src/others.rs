//! The "Other users" row: processes owned by other people's accounts are folded into one aggregate row in the process
//! list, while the current user's processes and the system's and services' are listed one by one. Pure; the agent calls it
//! per tick. While Dyno runs as administrator the grouping is off and every process is listed.

use crate::process::{Measured, Owner, ProcessSample, Total};
use crate::read_via::ReadVia;
use serde::{Deserialize, Serialize};

/// Where a process list came from and what the administrator helper added to it (WP-PROC). The default is a list read on this
/// computer only.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProcessProvenance {
    /// Who read the process list.
    pub processes: ReadVia,
    /// Who read the "Other users" total.
    pub others: ReadVia,
    /// The helper already lists other accounts' processes one by one, so they are not folded into "Other users".
    pub lists_others: bool,
    /// The total of other accounts' processes the helper counted for a caller who may not see them one by one; it replaces what
    /// the list adds up to.
    pub others_total: Option<OtherUsers>,
}

impl ProcessProvenance {
    /// Whether other accounts' processes are folded into one row: the product choice unless the helper lists them.
    pub fn fold_others(&self, product_choice: bool) -> bool {
        product_choice && !self.lists_others
    }

    /// `(listed, other_users)` for `processes`: [`split`] with this provenance applied.
    pub fn split(&self, processes: &[ProcessSample], product_choice: bool) -> (Vec<ProcessSample>, Option<OtherUsers>) {
        let (listed, other) = split(processes, self.fold_others(product_choice));
        match &self.others_total {
            Some(total) if product_choice => (listed, (total.count > 0).then(|| total.clone())),
            _ => (listed, other),
        }
    }
}

/// The aggregate of other users' processes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherUsers {
    /// How many processes are folded in.
    pub count: u32,
    /// The sum of the CPU that could be read, percent of the machine; unavailable when none could be read.
    pub cpu: Measured<f32>,
    /// The sum of the memory that could be read, in bytes; unavailable when none could be read.
    pub memory: Measured<u64>,
    /// Whether some processes' CPU or memory could not be read, so the sums are lower than the real totals.
    pub partial: bool,
}

/// Splits `processes` into those listed individually and the "Other users" aggregate. With `group_others` off nothing is
/// folded in. The aggregate is `None` when no process is folded in.
pub fn split(processes: &[ProcessSample], group_others: bool) -> (Vec<ProcessSample>, Option<OtherUsers>) {
    if !group_others {
        return (processes.to_vec(), None);
    }
    let (others, listed): (Vec<&ProcessSample>, Vec<&ProcessSample>) = processes.iter().partition(|p| p.owner == Owner::Other);
    let listed: Vec<ProcessSample> = listed.into_iter().cloned().collect();
    if others.is_empty() {
        return (listed, None);
    }
    let cpu = Total::of(others.iter().map(|p| &p.cpu));
    let memory = Total::of(others.iter().map(|p| &p.memory));
    let other = OtherUsers {
        count: u32::try_from(others.len()).unwrap_or(u32::MAX),
        cpu: cpu.sum,
        memory: memory.sum,
        partial: cpu.partial || memory.partial,
    };
    (listed, Some(other))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::fixtures::proc;
    use crate::process::UnavailableReason;

    fn owned(pid: u32, owner: Owner) -> ProcessSample {
        let mut p = proc(pid, None, "p.exe", 2.0, 10);
        p.owner = owner;
        p
    }

    #[test]
    fn own_system_and_unknown_processes_are_listed_and_others_are_folded_in() {
        let all = vec![
            owned(1, Owner::Own),
            owned(2, Owner::System),
            owned(3, Owner::Other),
            owned(4, Owner::Other),
            owned(5, Owner::Unknown),
        ];
        let (listed, other) = split(&all, true);
        assert_eq!(listed.iter().map(|p| p.pid).collect::<Vec<_>>(), [1, 2, 5]);
        let o = other.unwrap();
        assert_eq!(o.count, 2);
        assert_eq!(o.cpu, Measured::Value(4.0));
        assert_eq!(o.memory, Measured::Value(20 << 20));
        assert!(!o.partial);
    }

    #[test]
    fn the_aggregate_sums_only_what_was_read_and_says_partial() {
        let mut hidden = owned(3, Owner::Other);
        hidden.cpu = Measured::unavailable(UnavailableReason::NeedsAdmin);
        let (_, other) = split(&[hidden.clone(), owned(4, Owner::Other)], true);
        let o = other.unwrap();
        assert_eq!(
            (o.cpu, o.memory, o.partial),
            (Measured::Value(2.0), Measured::Value(20 << 20), true)
        );
        hidden.memory = Measured::unavailable(UnavailableReason::NeedsAdmin);
        let o = split(&[hidden], true).1.unwrap();
        assert_eq!(
            o.cpu,
            Measured::unavailable(UnavailableReason::NeedsAdmin),
            "nothing readable is not 0"
        );
        assert!(o.partial);
    }

    #[test]
    fn no_other_users_means_no_row_and_grouping_off_lists_everyone() {
        let all = vec![owned(1, Owner::Own), owned(3, Owner::Other)];
        assert_eq!(split(&all[..1], true).1, None);
        let (listed, other) = split(&all, false);
        assert_eq!(listed.len(), 2);
        assert_eq!(other, None);
    }
}
