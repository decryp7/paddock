use std::cell::{Cell, RefCell};
use std::{fs, thread};
use std::fs::DirEntry;
use std::io::Error;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use regex::Regex;
use tauri::AppHandle;
use tauri::Manager;
use tracing::info;
use crate::build_version::BuildVersion;
use crate::publisher;
use crate::publisher::{Event, Publisher, Subscription, NewVersion};
use crate::version_checker::VersionChecker;
use crate::version_updater::VersionUpdater;

pub struct ReleaseMonitor {
    publisher: Arc<Mutex<Publisher>>,
    version_checker: Arc<dyn VersionChecker + Send + Sync>,
    version_updater: Arc<dyn VersionUpdater + Send + Sync>,
    stop: Arc<AtomicBool>,
    interval_seconds: u32,
    cached_version: Arc<Mutex<BuildVersion>>
}

impl ReleaseMonitor {
    pub fn new(version_checker: Arc<dyn VersionChecker + Send + Sync>,
               version_updater: Arc<dyn VersionUpdater + Send + Sync>,
                interval_seconds: u32) -> ReleaseMonitor {
        Self {
            publisher: Arc::new(Mutex::new(Publisher::default())),
            version_checker,
            version_updater,
            stop: Arc::new(AtomicBool::new(false)),
            interval_seconds,
            cached_version : Arc::new(Mutex::new(BuildVersion::default()))
        }
    }

    pub fn acknowledge(&self, version: BuildVersion){
        self.version_updater.set_version(version);
    }

    pub fn reset(&self){
        self.version_updater.reset();
    }

    pub fn stop(&mut self){
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn subscribe(&self, event_type: Event, listener: Arc<Subscription>) -> () {
        self.publisher.lock().unwrap().subscribe(event_type, listener);
    }

    pub fn unsubscribe(&self, event_type: Event, listener: Arc<Subscription>) -> () {
        self.publisher.lock().unwrap().unsubscribe(event_type, listener);
    }

    pub fn start(&self) -> Result<(), anyhow::Error> {
        let vc = self.version_checker.clone();
        let vu = self.version_updater.clone();
        let p = self.publisher.clone();
        let stop = self.stop.clone();
        let interval = self.interval_seconds.clone();
        let cv = self.cached_version.clone();
        thread::spawn(move ||{
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }

                thread::sleep(Duration::from_secs(interval as u64));

                let latest_version = vc.get_latest_version().unwrap();
                let acked_version = vu.get_version();

                if latest_version == BuildVersion::default() {
                    continue;
                }

                let mut notify = false;
                let mut cached_version = cv.lock().unwrap();

                if *cached_version != latest_version {
                    *cached_version = latest_version;
                    notify = false;
                }else if acked_version != latest_version {
                    notify = true;
                }

                p.lock().unwrap().notify(Event::NewVersion, NewVersion::new(latest_version, notify));
                info!("Detected new version. vc: {}, latest: {}, cached: {}",
                    acked_version,
                    latest_version,
                    *cached_version);

            }
        });
        Ok(())
    }
}