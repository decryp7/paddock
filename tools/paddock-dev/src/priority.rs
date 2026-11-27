//! The one way a `./dev` command says its heavy steps are train or integrator work (user decision 91, 2026-11-26): `--priority train`.
//!
//! A train step runs through `throttle run --high` ([`crate::throttle::with_priority`]): while it waits for a slot it takes the next
//! free one before every normal waiter, and a normal step that arrives meanwhile queues behind it. Normal is the default, so a person's own
//! push and an agent's gate wait in order. The callers that mean it say it: `./dev merge check --gates` passes it to its gate and generator
//! steps, and an integrator adds it to `./dev check --pre-push --member-gate`, `--set slow` and `./dev gen check` (the train flow in
//! [`crate::pre_push::train_split`] prints it). There is no environment variable: one explicit switch, visible in the command line.

use clap::ValueEnum;

/// Who a heavy step is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Priority {
    /// Waits its turn behind everything queued (the default).
    #[default]
    Normal,
    /// Train or integrator work: goes before normal waiters.
    Train,
}

impl Priority {
    /// Whether the step takes the priority lane of `throttle`.
    pub fn is_train(self) -> bool {
        self == Priority::Train
    }

    /// The command-line words that ask for it from a child `./dev`: `--priority train`, or nothing for normal.
    pub fn args(self) -> &'static [&'static str] {
        match self {
            Priority::Normal => &[],
            Priority::Train => &["--priority", "train"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_train_asks_for_the_priority_lane_and_passes_the_flag_on() {
        assert!(!Priority::default().is_train() && Priority::default().args().is_empty());
        assert!(Priority::Train.is_train());
        assert_eq!(Priority::Train.args(), ["--priority", "train"]);
    }
}
