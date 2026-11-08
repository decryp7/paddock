use crate::{
    EventSink, Hold, HoldRequest, PowerError, PowerEvent, PowerOs, SleepWakeSource, Subscription,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A [`PowerOs`] for tests: records every acquire and release and never calls the OS, so no test can keep a real machine
/// awake (ER-14).
#[derive(Default)]
pub struct FakePower {
    next_id: AtomicUsize,
    log: Arc<Mutex<Vec<String>>>,
    live: Arc<AtomicUsize>,
    fail_with: Mutex<Option<PowerError>>,
}

impl FakePower {
    /// A fake that succeeds.
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the next acquires fail with `error` until [`succeed`](Self::succeed).
    pub fn fail_with(&self, error: PowerError) {
        *lock(&self.fail_with) = Some(error);
    }

    /// Makes acquires succeed again.
    pub fn succeed(&self) {
        *lock(&self.fail_with) = None;
    }

    /// Every acquire (`acquire#id scope why`) and release (`release#id`) in order.
    pub fn log(&self) -> Vec<String> {
        lock(&self.log).clone()
    }

    /// Holds taken and not yet released.
    pub fn live(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }
}

struct FakeHold {
    id: usize,
    log: Arc<Mutex<Vec<String>>>,
    live: Arc<AtomicUsize>,
}

impl Hold for FakeHold {
    fn os_ref(&self) -> String {
        format!("fake#{}", self.id)
    }
}

impl Drop for FakeHold {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::SeqCst);
        lock(&self.log).push(format!("release#{}", self.id));
    }
}

impl PowerOs for FakePower {
    fn acquire(&self, request: &HoldRequest) -> Result<Box<dyn Hold>, PowerError> {
        if let Some(e) = lock(&self.fail_with).clone() {
            return Err(e);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        self.live.fetch_add(1, Ordering::SeqCst);
        lock(&self.log).push(format!("acquire#{id} {:?} {}", request.scope, request.why));
        Ok(Box::new(FakeHold {
            id,
            log: self.log.clone(),
            live: self.live.clone(),
        }))
    }
}

/// A [`SleepWakeSource`] for tests: `emit` plays the OS's notification to the current subscriber.
#[derive(Default)]
pub struct FakeEvents {
    sink: Arc<Mutex<Option<EventSink>>>,
    refuse: AtomicBool,
}

impl FakeEvents {
    /// A source without a subscriber.
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes `subscribe` fail as an OS without the notification would.
    pub fn refuse(&self, refuse: bool) {
        self.refuse.store(refuse, Ordering::SeqCst);
    }

    /// Delivers `event` to the subscriber; false when nobody is subscribed.
    pub fn emit(&self, event: PowerEvent) -> bool {
        let sink = lock(&self.sink).clone();
        match sink {
            Some(s) => {
                s(event);
                true
            }
            None => false,
        }
    }
}

struct FakeSubscription {
    sink: Arc<Mutex<Option<EventSink>>>,
}

impl Subscription for FakeSubscription {}

impl Drop for FakeSubscription {
    fn drop(&mut self) {
        *lock(&self.sink) = None;
    }
}

impl SleepWakeSource for FakeEvents {
    fn subscribe(&self, sink: EventSink) -> Result<Box<dyn Subscription>, PowerError> {
        if self.refuse.load(Ordering::SeqCst) {
            return Err(PowerError::Unavailable {
                reason: "no sleep notifications here".into(),
            });
        }
        *lock(&self.sink) = Some(sink);
        Ok(Box::new(FakeSubscription {
            sink: self.sink.clone(),
        }))
    }
}
