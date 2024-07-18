use std::collections::HashMap;
use std::ptr::eq;
use std::sync::{Arc};
use crate::build_version::BuildVersion;

/// An event type.
#[derive(PartialEq, Eq, Hash, Clone)]
pub enum Event {
    LatestVersion,
}

pub struct Subscription {
    pub func: Box<dyn Fn(BuildVersion) + Send + Sync + 'static>
}

impl PartialEq for Subscription{
    fn eq(&self, other: &Self) -> bool {
        eq(self, other)
    }
}

impl Subscription {
    pub fn new(func: Box<dyn Fn(BuildVersion) + Send + Sync + 'static>) -> Self {
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

    pub fn notify(&self, event_type: Event, version: BuildVersion) {
        let listeners = self.events.get(&event_type).unwrap();
        //println!("{}", listeners.len());
        for listener in listeners {
            (listener.func)(version.clone());
        }
    }
}