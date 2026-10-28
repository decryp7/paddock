//! The one result enum (design review 1.1, P1): infrastructure failures are kept apart from product failures, so a lost VM
//! never counts as a failed test and flake statistics see only real failures.

/// How a step, a plan or a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// Everything as expected.
    Passed,
    /// Passed after a recovery (retry, popup dismissed, re-locate).
    PassedWithRecovery,
    /// The application under test misbehaved (an assertion failed, the element is missing).
    Failed,
    /// Infrastructure failed: target lost, capture black, VM revert failed, model missing.
    Error,
    /// Cannot run until something is labelled or recorded.
    Blocked,
    /// The user stopped the run.
    Stopped,
    /// Not run.
    Skipped,
}

impl Outcome {
    /// True for [`Outcome::Passed`] and [`Outcome::PassedWithRecovery`].
    pub fn is_success(self) -> bool {
        matches!(self, Outcome::Passed | Outcome::PassedWithRecovery)
    }

    /// True when the failure is the product's (JUnit `<failure>`); only these feed flake statistics.
    pub fn is_product_failure(self) -> bool {
        self == Outcome::Failed
    }

    /// True when the infrastructure failed (JUnit `<error>`); never counted as a product failure.
    pub fn is_infrastructure_error(self) -> bool {
        self == Outcome::Error
    }

    /// The more severe of two outcomes, for rolling steps up into a plan.
    pub fn worst(self, other: Outcome) -> Outcome {
        if self.severity() >= other.severity() { self } else { other }
    }

    fn severity(self) -> u8 {
        match self {
            Outcome::Skipped => 0,
            Outcome::Passed => 1,
            Outcome::PassedWithRecovery => 2,
            Outcome::Blocked => 3,
            Outcome::Stopped => 4,
            Outcome::Failed => 5,
            Outcome::Error => 6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_not_failures() {
        assert!(Outcome::Error.is_infrastructure_error() && !Outcome::Error.is_product_failure());
        assert!(Outcome::Failed.is_product_failure() && !Outcome::Failed.is_infrastructure_error());
        for o in [
            Outcome::Passed,
            Outcome::PassedWithRecovery,
            Outcome::Blocked,
            Outcome::Stopped,
            Outcome::Skipped,
        ] {
            assert!(!o.is_product_failure() && !o.is_infrastructure_error(), "{o:?}");
        }
        assert!(Outcome::Passed.is_success() && Outcome::PassedWithRecovery.is_success() && !Outcome::Failed.is_success());
    }

    #[test]
    fn worst_rolls_up_by_severity() {
        assert_eq!(Outcome::Passed.worst(Outcome::Failed), Outcome::Failed);
        assert_eq!(Outcome::Error.worst(Outcome::Failed), Outcome::Error);
        assert_eq!(Outcome::Skipped.worst(Outcome::Passed), Outcome::Passed);
        assert_eq!(Outcome::PassedWithRecovery.worst(Outcome::Passed), Outcome::PassedWithRecovery);
        assert_eq!(Outcome::Blocked.worst(Outcome::Stopped), Outcome::Stopped);
        assert_eq!(Outcome::Stopped.worst(Outcome::Blocked), Outcome::Stopped);
    }
}
