//! When a page can be read: the load finished, the awaited element is there (or the region of interest has items), and
//! the DOM has been quiet for a while. Not "network idle". Pure: the caller polls and passes the observations.

use std::time::Duration;

/// What one poll saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    /// `document.readyState === "complete"`.
    pub load_finished: bool,
    /// Whether the awaited element exists; `None` when none is awaited.
    pub element_present: Option<bool>,
    /// Items in the region of interest (links in a result list, say); `None` when there is no such region.
    pub region_items: Option<usize>,
    /// How long no node changed.
    pub quiet: Duration,
}

/// How long the DOM must be still.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    /// Mutation-free time before a page with content is read.
    pub quiet: Duration,
    /// An empty region is accepted after this much quiet (a search without results).
    pub quiet_empty: Duration,
}

impl Default for Rule {
    /// 500 ms of quiet, 2 s for an empty region.
    fn default() -> Self {
        Rule {
            quiet: Duration::from_millis(500),
            quiet_empty: Duration::from_secs(2),
        }
    }
}

/// The verdict of a poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// Read it now.
    Ready,
    /// The load has not finished.
    Loading,
    /// The awaited element is not there (yet).
    WaitingForElement,
    /// The region of interest is empty and has not been quiet long enough to accept that.
    RegionEmpty,
    /// The DOM still changes.
    Changing,
}

/// Applies the readiness rule.
pub fn readiness(o: &Observation, rule: &Rule) -> Readiness {
    if !o.load_finished {
        Readiness::Loading
    } else if o.element_present == Some(false) {
        Readiness::WaitingForElement
    } else if o.region_items == Some(0) {
        if o.quiet >= rule.quiet_empty {
            Readiness::Ready
        } else {
            Readiness::RegionEmpty
        }
    } else if o.quiet < rule.quiet {
        Readiness::Changing
    } else {
        Readiness::Ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn o(load: bool, el: Option<bool>, items: Option<usize>, ms: u64) -> Observation {
        Observation {
            load_finished: load,
            element_present: el,
            region_items: items,
            quiet: Duration::from_millis(ms),
        }
    }

    #[test]
    fn the_rule_of_both_apps() {
        let r = Rule::default();
        assert_eq!(
            readiness(&o(false, Some(true), Some(9), 9999), &r),
            Readiness::Loading
        );
        assert_eq!(
            readiness(&o(true, Some(false), None, 9999), &r),
            Readiness::WaitingForElement
        );
        assert_eq!(
            readiness(&o(true, Some(true), None, 100), &r),
            Readiness::Changing
        );
        assert_eq!(
            readiness(&o(true, Some(true), None, 500), &r),
            Readiness::Ready
        );
        assert_eq!(
            readiness(&o(true, None, None, 499), &r),
            Readiness::Changing
        );
        assert_eq!(
            readiness(&o(true, None, Some(9), 600), &r),
            Readiness::Ready
        );
        assert_eq!(
            readiness(&o(true, None, Some(0), 600), &r),
            Readiness::RegionEmpty
        );
        assert_eq!(
            readiness(&o(true, None, Some(0), 2000), &r),
            Readiness::Ready
        );
    }
}
