use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::ptr::eq;
use std::sync::{Arc};
use crate::build_version::BuildVersion;

/// An event type.
#[derive(PartialEq, Eq, Hash, Clone)]
pub enum Event {
    LatestVersion,
    NewVersion
}

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct NewVersion {
    pub version: BuildVersion,
    pub notify: bool
}

impl NewVersion {
    pub fn new(version: BuildVersion, notify: bool) -> Self{
        Self { version, notify}
    }
}

impl Display for NewVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "R{}.{:0>2}.{:0>2}T{:0>2}", self.version.major, self.version.minor, self.version.patch, self.version.t)
    }
}

pub struct Subscription {
    pub func: Box<dyn Fn(NewVersion) + Send + Sync + 'static>
}

impl PartialEq for Subscription{
    fn eq(&self, other: &Self) -> bool {
        eq(self, other)
    }
}

impl Subscription {
    pub fn new(func: Box<dyn Fn(NewVersion) + Send + Sync + 'static>) -> Self {
        Self { func }
    }
}

/// Publisher sends events to subscribers (listeners).
#[derive(Default)]
pub struct Publisher {
    events: HashMap<Event, Vec<Arc<Subscription>>>,
}

impl Publisher {
    pub fn subscribe(&mut self, event_type: Event, listener: Arc<Subscription>) {
        self.events.entry(event_type.clone()).or_default();
        self.events.get_mut(&event_type).unwrap().push(listener);
    }

    pub fn unsubscribe(&mut self, event_type: Event, listener: Arc<Subscription>) {
        self.events
            .get_mut(&event_type)
            .unwrap()
            .retain(|x| *x != listener);
    }

    pub fn notify(&self, event_type: Event, version: NewVersion) {
        match self.events.get(&event_type) {
            None => {}
            Some(listeners) => {
                //println!("{}", listeners.len());
                for listener in listeners {
                    (listener.func)(version.clone());
                }
            }
        }
    }
}