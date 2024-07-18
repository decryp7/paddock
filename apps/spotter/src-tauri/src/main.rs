// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod release_monitor;
mod build_version;
mod version_checker;
mod publisher;
mod version_updater;
mod config;

use std::{env, thread};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use anyhow::Error;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use figment::{Figment, Provider};
use figment::providers::{Format, Toml};
use serde::Serialize;
use tauri::{Manager, Window, SystemTray, SystemTrayMenu, SystemTrayEvent};
use tauri::api::notification::Notification;
use crate::build_version::BuildVersion;
use crate::config::VersionCheckerConfig;
use crate::release_monitor::ReleaseMonitor;
use crate::publisher::{Event, Subscription};
use crate::version_checker::{SharedFolderVersionChecker, VersionChecker};
use crate::version_updater::{FileCacheVersionUpdater, VersionUpdater};
use std::string::String;

fn main() {
    let mut version_checker_config = VersionCheckerConfig::default();
    match Figment::from(VersionCheckerConfig::default())
            .merge(Toml::file(env::current_exe().unwrap().parent().unwrap().join("app.toml")))
            .extract::<VersionCheckerConfig>() {
        Ok(c) => {
            version_checker_config = c;
        }
        Err(_) => {
            println!("Unable to read config!");
        }
    }

    tauri::Builder::default()
        .system_tray(SystemTray::new())
        .setup(move |app| {
            let version_checker =
                Box::new(SharedFolderVersionChecker::new( version_checker_config.path.as_str(), version_checker_config.file_regex.as_str()));
            let version_updater =
                Box::new(FileCacheVersionUpdater::new(env::temp_dir().join(r"version.txt").to_str().unwrap()));

            let mut title = String::new();
            match version_checker.get_latest_version(){
                Ok(v) => {
                    title = v.to_string();
                }
                Err(_) => {
                    title = String::from("Unable to retrieve latest version");
                }
            }

            let main_window = app.get_window("main").unwrap();
            main_window
                .set_title(title.as_str())
                .unwrap();
            main_window.emit("latest-version", title).unwrap();

            let app = Arc::new(app.handle());
            let subscription = Arc::new(Subscription::new(Box::new(move |v| {
                //println!("{}", v);
                let a = app.clone();
                let main_window = a.get_window("main").unwrap();
                main_window
                    .set_title(v.to_string().as_str())
                    .unwrap();
                main_window.emit("latest-version", v.to_string()).unwrap();
                let _ = Notification::new(&a.config().tauri.bundle.identifier)
                    .title("New T Version")
                    .body(v.to_string().as_str())
                    .show();
            })));

            let mut release_monitor = ReleaseMonitor::new(version_checker, version_updater);
            release_monitor.subscribe(Event::LatestVersion, subscription.clone());
            match release_monitor.start() {
                Ok(_) => { println!("Release monitor started!")}
                Err(_) => { println!("Failed to start monitor!") }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
