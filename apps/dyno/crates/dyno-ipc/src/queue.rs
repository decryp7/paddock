//! A small bounded, closable multi-producer multi-consumer queue (std's channels cannot send with a timeout).

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub(crate) struct Queue<T> {
    state: Mutex<State<T>>,
    not_empty: Condvar,
    not_full: Condvar,
    capacity: usize,
}

struct State<T> {
    items: VecDeque<T>,
    closed: bool,
}

/// Why an item was not queued.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PushError {
    Closed,
    Full,
}

impl<T> Queue<T> {
    pub(crate) fn new(capacity: usize) -> Self {
        Queue {
            state: Mutex::new(State {
                items: VecDeque::new(),
                closed: false,
            }),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
            capacity: capacity.max(1),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State<T>> {
        // The state is a plain queue: still consistent after a panic elsewhere.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Waits for room up to `timeout` (`None`: as long as it takes).
    pub(crate) fn push(&self, item: T, timeout: Option<Duration>) -> Result<(), PushError> {
        let deadline = timeout.map(|t| Instant::now() + t);
        let mut s = self.lock();
        loop {
            if s.closed {
                return Err(PushError::Closed);
            }
            if s.items.len() < self.capacity {
                s.items.push_back(item);
                self.not_empty.notify_one();
                return Ok(());
            }
            s = match deadline {
                None => self.not_full.wait(s).unwrap_or_else(|e| e.into_inner()),
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err(PushError::Full);
                    }
                    self.not_full.wait_timeout(s, left).unwrap_or_else(|e| e.into_inner()).0
                }
            };
        }
    }

    /// Queues `item` only if there is room now.
    pub(crate) fn try_push(&self, item: T) -> Result<(), PushError> {
        self.push(item, Some(Duration::ZERO))
    }

    /// The next item; `None` once the queue is closed.
    pub(crate) fn pop(&self) -> Option<T> {
        let mut s = self.lock();
        loop {
            if s.closed {
                return None;
            }
            if let Some(item) = s.items.pop_front() {
                self.not_full.notify_one();
                return Some(item);
            }
            s = self.not_empty.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Wakes every waiter; queued items are dropped.
    pub(crate) fn close(&self) {
        let mut s = self.lock();
        s.closed = true;
        s.items.clear();
        self.not_empty.notify_all();
        self.not_full.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn a_full_queue_times_out_and_a_closed_one_refuses_and_wakes() {
        let q = Arc::new(Queue::new(1));
        q.push(1, None).unwrap();
        assert_eq!(q.try_push(2), Err(PushError::Full));
        assert_eq!(q.push(2, Some(Duration::from_millis(20))), Err(PushError::Full));
        assert_eq!(q.pop(), Some(1));
        let q2 = q.clone();
        let waiter = std::thread::spawn(move || q2.pop());
        std::thread::sleep(Duration::from_millis(20));
        q.close();
        assert_eq!(waiter.join().unwrap(), None);
        assert_eq!(q.push(3, None), Err(PushError::Closed));
    }

    #[test]
    fn a_blocked_producer_continues_when_room_appears() {
        let q = Arc::new(Queue::new(1));
        q.push(1, None).unwrap();
        let q2 = q.clone();
        let producer = std::thread::spawn(move || q2.push(2, None));
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(q.pop(), Some(1));
        producer.join().unwrap().unwrap();
        assert_eq!(q.pop(), Some(2));
    }
}
