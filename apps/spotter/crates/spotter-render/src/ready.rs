//! When a page can be read: the load finished, the awaited element is present and the DOM has been quiet for a
//! while. Not "network idle". Pure: the caller polls and passes the observations.

use std::time::Duration;

/// What one poll saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    /// `document.readyState === "complete"`.
    pub load_finished: bool,
    /// Whether the awaited element exists; `None` when the watch names none.
    pub element_present: Option<bool>,
    /// How long no DOM mutation happened.
    pub quiet: Duration,
}

/// How long the DOM must be still.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    /// Required mutation-free time.
    pub quiet: Duration,
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            quiet: Duration::from_millis(500),
        }
    }
}

/// The verdict of a poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// Read it now.
    Ready,
    /// The load event has not finished.
    Loading,
    /// The awaited element is not there (yet).
    WaitingForElement,
    /// Present, but the DOM still changes.
    Changing,
}

/// Applies the readiness rule.
pub fn readiness(o: &Observation, rule: &Rule) -> Readiness {
    if !o.load_finished {
        Readiness::Loading
    } else if o.element_present == Some(false) {
        Readiness::WaitingForElement
    } else if o.quiet < rule.quiet {
        Readiness::Changing
    } else {
        Readiness::Ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn o(load: bool, el: Option<bool>, ms: u64) -> Observation {
        Observation {
            load_finished: load,
            element_present: el,
            quiet: Duration::from_millis(ms),
        }
    }

    #[test]
    fn rule() {
        let r = Rule::default();
        assert_eq!(readiness(&o(false, Some(true), 9999), &r), Readiness::Loading);
        assert_eq!(readiness(&o(true, Some(false), 9999), &r), Readiness::WaitingForElement);
        assert_eq!(readiness(&o(true, Some(true), 100), &r), Readiness::Changing);
        assert_eq!(readiness(&o(true, Some(true), 500), &r), Readiness::Ready);
        assert_eq!(readiness(&o(true, None, 600), &r), Readiness::Ready);
        assert_eq!(readiness(&o(true, None, 499), &r), Readiness::Changing);
    }
}
