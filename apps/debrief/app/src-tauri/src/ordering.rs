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
    /// An ordering job is queued or running; `auto`: Debrief started it; `job`: its id, once the jobs service gave it
    /// (until then an end is taken as this job's: only the job just started can end in the window).
    Running { auto: bool, job: Option<u64> },
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
    /// The window's store was opened or replaced: a new episode.
    Opened,
    /// The store says the order is incomplete.
    Incomplete,
    /// The user asks for the ordering (the Store pane, or the message's action).
    UserStarted,
    /// The job [`Action::StartJob`] asked for was started with id `job`.
    Started { job: u64 },
    /// An ordering job of the window ended (`job` is its id).
    Finished { outcome: Outcome, job: u64 },
}

/// What the caller does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Nothing,
    /// Start the ordering job (`auto`: Debrief's own start), then report [`Event::Started`].
    StartJob {
        auto: bool,
    },
    /// The user asked while an ordering job runs: say so, start nothing.
    AlreadyRunning,
    /// Tell the user the events are not in time order, with an action that runs the ordering again (a retry of `job`).
    AskUser {
        job: u64,
    },
    /// The events are in time order: resolve the message.
    Resolve,
}

/// The next state and the action for `event` in `state`.
pub(crate) fn step(state: State, event: Event) -> (State, Action) {
    use State::*;
    match (state, event) {
        (_, Event::Opened) => (Ready, Action::Nothing),
        (Ready, Event::Incomplete) => (Running { auto: true, job: None }, Action::StartJob { auto: true }),
        (s @ (Running { .. } | LeftToUser), Event::Incomplete) => (s, Action::Nothing),
        (s @ Running { .. }, Event::UserStarted) => (s, Action::AlreadyRunning),
        (Ready | LeftToUser, Event::UserStarted) => (Running { auto: false, job: None }, Action::StartJob { auto: false }),
        (Running { auto, job: None }, Event::Started { job }) => (Running { auto, job: Some(job) }, Action::Nothing),
        (s, Event::Started { .. }) => (s, Action::Nothing),
        // An end of another job (one of before an open, or a second one) is not this episode's.
        (s @ Running { job: Some(ours), .. }, Event::Finished { job, .. }) if job != ours => (s, Action::Nothing),
        (
            Running { .. },
            Event::Finished {
                outcome: Outcome::Done, ..
            },
        ) => (Ready, Action::Resolve),
        (Running { .. }, Event::Finished { job, .. }) => (LeftToUser, Action::AskUser { job }),
        (Ready | LeftToUser, Event::Finished { .. }) => (state, Action::Nothing),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn auto(job: Option<u64>) -> State {
        State::Running { auto: true, job }
    }
    const fn user(job: Option<u64>) -> State {
        State::Running { auto: false, job }
    }

    fn finished(outcome: Outcome, job: u64) -> Event {
        Event::Finished { outcome, job }
    }

    #[test]
    fn the_table() {
        let table = [
            // An incomplete order starts the job once per episode; its id is recorded once started.
            (State::Ready, Event::Incomplete, auto(None), Action::StartJob { auto: true }),
            (auto(None), Event::Started { job: 7 }, auto(Some(7)), Action::Nothing),
            (auto(Some(7)), Event::Incomplete, auto(Some(7)), Action::Nothing),
            (user(Some(7)), Event::Incomplete, user(Some(7)), Action::Nothing),
            (auto(Some(7)), Event::Started { job: 9 }, auto(Some(7)), Action::Nothing),
            // The user asks while it runs: nothing is started (the Store pane cannot start a second one).
            (auto(Some(7)), Event::UserStarted, auto(Some(7)), Action::AlreadyRunning),
            (user(None), Event::UserStarted, user(None), Action::AlreadyRunning),
            // Cancelled or failed: left to the user, asked once, never restarted by the next refresh or restart.
            (
                auto(Some(7)),
                finished(Outcome::Cancelled, 7),
                State::LeftToUser,
                Action::AskUser { job: 7 },
            ),
            (
                auto(Some(7)),
                finished(Outcome::Failed, 7),
                State::LeftToUser,
                Action::AskUser { job: 7 },
            ),
            (
                user(Some(7)),
                finished(Outcome::Cancelled, 7),
                State::LeftToUser,
                Action::AskUser { job: 7 },
            ),
            (
                auto(None),
                finished(Outcome::Failed, 7),
                State::LeftToUser,
                Action::AskUser { job: 7 },
            ),
            (State::LeftToUser, Event::Incomplete, State::LeftToUser, Action::Nothing),
            // An end of another job is not this one's.
            (auto(Some(7)), finished(Outcome::Failed, 3), auto(Some(7)), Action::Nothing),
            (auto(Some(7)), finished(Outcome::Done, 3), auto(Some(7)), Action::Nothing),
            // The user starts it again (Store pane, the message's action).
            (State::LeftToUser, Event::UserStarted, user(None), Action::StartJob { auto: false }),
            (State::Ready, Event::UserStarted, user(None), Action::StartJob { auto: false }),
            // Done ends the episode and resolves the message.
            (auto(Some(7)), finished(Outcome::Done, 7), State::Ready, Action::Resolve),
            (user(Some(7)), finished(Outcome::Done, 7), State::Ready, Action::Resolve),
            // Opening or replacing the store is a new episode.
            (State::LeftToUser, Event::Opened, State::Ready, Action::Nothing),
            (auto(Some(7)), Event::Opened, State::Ready, Action::Nothing),
            // A stray end changes nothing.
            (State::Ready, finished(Outcome::Failed, 7), State::Ready, Action::Nothing),
            (
                State::LeftToUser,
                finished(Outcome::Cancelled, 7),
                State::LeftToUser,
                Action::Nothing,
            ),
            (State::LeftToUser, finished(Outcome::Done, 7), State::LeftToUser, Action::Nothing),
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
            Event::Started { job: 7 },
            finished(Outcome::Cancelled, 7),
            Event::Incomplete,
            Event::Incomplete,
        ] {
            let (next, action) = step(s, event);
            started += usize::from(matches!(action, Action::StartJob { .. }));
            s = next;
        }
        assert_eq!((s, started), (State::LeftToUser, 1));
    }
}
