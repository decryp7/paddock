//! When Debrief puts a window's events in time order by itself, and when it leaves that to the user: one small state
//! machine (states and events in, actions out), so the stale mark, automatic starts, Cancel, failures, live follow,
//! opening and worker restarts follow one table instead of flags spread over the callers.
//!
//! - An incomplete order (after an open, a live refresh, a store-worker restart, a clock correction that failed) starts
//!   the ordering job once per episode.
//! - When that job is cancelled or fails, Debrief does not start it again: the user gets a message whose action runs it
//!   (a retry of the job), or starts it from the Store pane. Repeating a multi-minute job that holds the store, after
//!   the user stopped it or after it ran out of memory, would undo the user's Cancel (ER-18) and fail the same way.
//! - A finished ordering ends the episode and resolves the message; opening a store starts a new episode.

/// Where the window's ordering stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum State {
    /// Nothing is known to be wrong, or a new episode: an incomplete order starts the job.
    #[default]
    Ready,
    /// An ordering job is queued or running; `auto`: Debrief started it.
    Running { auto: bool },
    /// This episode's ordering was cancelled or failed: Debrief does not start it again.
    LeftToUser,
}

/// How an ordering job ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Done,
    Cancelled,
    Failed,
}

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Event {
    /// A store was opened in the window: a new episode.
    Opened,
    /// The store says the order is incomplete.
    Incomplete,
    /// The user started the ordering (the Store pane, or the message's action).
    UserStarted,
    /// An ordering job of the window ended (`job` is its id).
    Finished { outcome: Outcome, job: u64 },
}

/// What the caller does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Nothing,
    /// Start the ordering job (Debrief's own start).
    StartJob,
    /// Tell the user the events are not in time order, with an action that runs the ordering again (a retry of `job`).
    AskUser {
        job: u64,
    },
    /// The events are in time order: resolve the message.
    Resolve,
}

/// The next state and the action for `event` in `state`.
pub(crate) fn step(state: State, event: Event) -> (State, Action) {
    match (state, event) {
        (_, Event::Opened) => (State::Ready, Action::Nothing),
        (State::Ready, Event::Incomplete) => (State::Running { auto: true }, Action::StartJob),
        (s @ (State::Running { .. } | State::LeftToUser), Event::Incomplete) => (s, Action::Nothing),
        (_, Event::UserStarted) => (State::Running { auto: false }, Action::Nothing),
        (
            _,
            Event::Finished {
                outcome: Outcome::Done, ..
            },
        ) => (State::Ready, Action::Resolve),
        (State::Running { .. }, Event::Finished { job, .. }) => (State::LeftToUser, Action::AskUser { job }),
        (s @ (State::Ready | State::LeftToUser), Event::Finished { .. }) => (s, Action::Nothing),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTO: State = State::Running { auto: true };
    const USER: State = State::Running { auto: false };

    fn finished(outcome: Outcome) -> Event {
        Event::Finished { outcome, job: 7 }
    }

    #[test]
    fn the_table() {
        let table = [
            // An incomplete order starts the job once per episode.
            (State::Ready, Event::Incomplete, AUTO, Action::StartJob),
            (AUTO, Event::Incomplete, AUTO, Action::Nothing),
            (USER, Event::Incomplete, USER, Action::Nothing),
            // Cancelled or failed: left to the user, asked once, never restarted by the next refresh or restart.
            (AUTO, finished(Outcome::Cancelled), State::LeftToUser, Action::AskUser { job: 7 }),
            (AUTO, finished(Outcome::Failed), State::LeftToUser, Action::AskUser { job: 7 }),
            (USER, finished(Outcome::Cancelled), State::LeftToUser, Action::AskUser { job: 7 }),
            (State::LeftToUser, Event::Incomplete, State::LeftToUser, Action::Nothing),
            // The user starts it again (Store pane, the message's action).
            (State::LeftToUser, Event::UserStarted, USER, Action::Nothing),
            (State::Ready, Event::UserStarted, USER, Action::Nothing),
            // Done ends the episode and resolves the message.
            (AUTO, finished(Outcome::Done), State::Ready, Action::Resolve),
            (USER, finished(Outcome::Done), State::Ready, Action::Resolve),
            (State::LeftToUser, finished(Outcome::Done), State::Ready, Action::Resolve),
            // Opening a store is a new episode.
            (State::LeftToUser, Event::Opened, State::Ready, Action::Nothing),
            (AUTO, Event::Opened, State::Ready, Action::Nothing),
            // A stray end (a job of before an open) changes nothing.
            (State::Ready, finished(Outcome::Failed), State::Ready, Action::Nothing),
            (State::LeftToUser, finished(Outcome::Cancelled), State::LeftToUser, Action::Nothing),
        ];
        for (state, event, next, action) in table {
            assert_eq!(step(state, event), (next, action), "{state:?} + {event:?}");
        }
    }

    #[test]
    fn a_live_episode_cancelled_by_the_user_is_not_restarted_by_later_refreshes() {
        let mut s = State::default();
        let mut started = 0;
        for event in [
            Event::Incomplete,
            finished(Outcome::Cancelled),
            Event::Incomplete,
            Event::Incomplete,
            Event::Incomplete,
        ] {
            let (next, action) = step(s, event);
            started += usize::from(action == Action::StartJob);
            s = next;
        }
        assert_eq!((s, started), (State::LeftToUser, 1));
    }
}
