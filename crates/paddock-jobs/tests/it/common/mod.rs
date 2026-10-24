#![allow(dead_code)]
use paddock_jobs::{EventSink, JobSnapshot};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct Collect(pub Mutex<Vec<JobSnapshot>>);

impl Collect {
    pub fn new() -> Arc<Self> {
        Arc::new(Collect::default())
    }
    pub fn all(&self) -> Vec<JobSnapshot> {
        self.0.lock().unwrap().clone()
    }
}

impl EventSink for Collect {
    fn emit(&self, s: &JobSnapshot) {
        self.0.lock().unwrap().push(s.clone());
    }
}

pub struct Panicky;
impl EventSink for Panicky {
    fn emit(&self, _: &JobSnapshot) {
        panic!("sink bug");
    }
}
