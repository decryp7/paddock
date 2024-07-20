// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod release_monitor;
mod build_version;
mod version_checker;
mod publisher;
mod version_updater;
mod config;

use std::{env, fs, thread};
use std::any::Any;
use std::collections::HashMap;
use std::fs::{metadata, OpenOptions};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use anyhow::Error;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use figment::{Figment, Provider};
use figment::providers::{Format, Toml};
use serde::Serialize;
use tauri::{Manager, Window, SystemTray, SystemTrayMenu, SystemTrayEvent, CustomMenuItem, SystemTrayMenuItem, State};
use tauri::api::notification::Notification;
use crate::build_version::BuildVersion;
use crate::config::VersionCheckerConfig;
use crate::release_monitor::ReleaseMonitor;
use crate::publisher::{Event, Subscription};
use crate::version_checker::{SharedFolderVersionChecker, VersionChecker};
use crate::version_updater::{FileCacheVersionUpdater, VersionUpdater};
use std::string::String;
use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use directories::ProjectDirs;
use tracing::{error, info};
use tracing_appender::rolling::{RollingFileAppender, Rotation};

fn get_auto_launch_builder() -> AutoLaunch {
    return AutoLaunchBuilder::new()
        .set_app_name("release-monitor")
        .set_app_path(env::current_exe().unwrap().to_str().unwrap())
        .set_use_launch_agent(true)
        .build()
        .unwrap();
}

#[tauri::command]
fn get_auto_launch() -> bool {
    return get_auto_launch_builder().is_enabled().unwrap();
}

#[tauri::command]
fn set_auto_launch(auto_launch: bool) -> bool {
    let auto = get_auto_launch_builder();

    if auto_launch {
        auto.enable().unwrap();
    }else{
        auto.disable().unwrap();
    }
    println!("{}", auto.is_enabled().unwrap());
    return auto.is_enabled().unwrap();
}


#[tauri::command]
fn get_latest_version(services: tauri::State<HashMap<&str, Arc<dyn Any +Send + Sync>>>) -> String {
    match services.get("version_checker") {
        None => {}
        Some(r) => {
            let vc : Arc<SharedFolderVersionChecker> = r.clone().downcast::<SharedFolderVersionChecker>().unwrap();
            let build_version = vc.get_latest_version().unwrap();
            return build_version.to_string();
        }
    }

    return String::from("");
}

#[tauri::command]
fn acknowledge(app_handle: tauri::AppHandle, services: tauri::State<HashMap<&str, Arc<dyn Any +Send + Sync>>>, version:String) -> bool {
    let v = BuildVersion::parse(version.as_str()).unwrap();
    if v == BuildVersion::default() {
        return false;
    }

    match services.get("release_monitor") {
        None => {}
        Some(r) => {
            let vc : Arc<ReleaseMonitor> = r.clone().downcast::<ReleaseMonitor>().unwrap();
            vc.acknowledge(v);
            app_handle.tray_handle().set_icon(tauri::Icon::Raw(include_bytes!("../icons/icon.ico").to_vec())).unwrap();
            return true;
        }
    }

    return false;
}

fn main() {
    let project_dirs = ProjectDirs::from("com", "decryptology",  "releasemonitor").unwrap();
    let config_path = Arc::new(project_dirs.config_dir().to_path_buf());

    let logfile = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("release-monitor")
        .filename_suffix("log")
        .max_log_files(10)
        .build(&*config_path)
        .unwrap();

    tracing_subscriber::fmt()
        .with_writer(logfile)
        .init();

    let mut version_checker_config = VersionCheckerConfig::default();
    match Figment::from(VersionCheckerConfig::default())
            .merge(Toml::file(&config_path.join("app.toml")))
            .extract::<VersionCheckerConfig>() {
        Ok(c) => {
            version_checker_config = c;
        }
        Err(_) => {
            error!("Unable to read config!");
        }
    }

    let version_checker =
        Arc::new(SharedFolderVersionChecker::new( version_checker_config.path.as_str(), version_checker_config.file_regex.as_str()));
    let version_updater =
        Arc::new(FileCacheVersionUpdater::new(env::temp_dir().join(r"version.txt").to_str().unwrap()));

    let release_monitor = Arc::new(ReleaseMonitor::new(version_checker.clone(), version_updater.clone(), version_checker_config.interval_seconds));
    match release_monitor.start() {
        Ok(_) => { info!("Release monitor started!")}
        Err(_) => { error!("Failed to start monitor!") }
    }

    let mut services : HashMap<&str, Arc<dyn Any +Send + Sync>> = HashMap::new();
    services.insert("release_monitor", release_monitor.clone());
    services.insert("version_checker", version_checker.clone());
    services.insert("version_updater", version_updater.clone());

    let show = CustomMenuItem::new("show".to_string(), "Show");
    let edit_config = CustomMenuItem::new("edit_config".to_string(), "Edit Config");
    let reset = CustomMenuItem::new("reset".to_string(), "Reset");
    let logs = CustomMenuItem::new("logs".to_string(), "Logs");
    let restart = CustomMenuItem::new("restart".to_string(), "Restart");
    let quit = CustomMenuItem::new("quit".to_string(), "Quit");
    let tray_menu = SystemTrayMenu::new()
        .add_item(show)
        .add_item(edit_config)
        .add_item(reset)
        .add_item(logs)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(restart)
        .add_item(quit);
    let tray = SystemTray::new().with_menu(tray_menu);

    let c = config_path.clone();
    tauri::Builder::default()
        .on_window_event(|event| match event.event() {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                event.window().hide().unwrap();
                api.prevent_close();
            }
            _ => {}
        })
        .system_tray(tray)
        .on_system_tray_event(move |app, event| match event {
            SystemTrayEvent::MenuItemClick { id,.. } => {
                match id.as_str() {
                    "show" => {
                        let window = app.get_window("main").unwrap();
                        window.show().unwrap();
                    }
                    "edit_config" => {
                        let config_file = c.join("app.toml");

                        if !metadata(&config_file).is_ok() {
                            OpenOptions::new().create(true).write(true).open(&config_file).unwrap();
                        }
                        match open::that(&config_file) {
                            Ok(_) => {}
                            Err(e) => {
                                error!("Failed to open config file. Error: {}", e);
                            }
                        }
                    }
                    "reset" => {
                        let services : State<HashMap<&str, Arc<dyn Any +Send + Sync>>> = app.state();
                        match services.get("version_updater") {
                            None => {}
                            Some(r) => {
                                let vc : Arc<FileCacheVersionUpdater> = r.clone().downcast::<FileCacheVersionUpdater>().unwrap();
                                vc.reset();
                            }
                        }
                    }
                    "logs" => {
                        let project_dirs = ProjectDirs::from("com", "decryptology",  "releasemonitor").unwrap();
                        let config_path = project_dirs.config_dir();

                        match open::that(&config_path) {
                            Ok(_) => {}
                            Err(e) => {
                                error!("Failed to open config dir. Error: {}", e);
                            }
                        }
                    }
                    "restart" => {
                        app.restart();
                    }
                    "quit" => {
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
            SystemTrayEvent::LeftClick { .. } => {}
            SystemTrayEvent::RightClick { .. } => {}
            SystemTrayEvent::DoubleClick { .. } => {
                info!("system tray received a double click");
                let window = app.get_window("main").unwrap();
                window.show().unwrap();
            }
            _ => {}
        })
        .manage(services)
        .invoke_handler(tauri::generate_handler![get_latest_version, acknowledge, get_auto_launch, set_auto_launch])
        .setup(move |app| {

            let app = Arc::new(app.handle());
            let app_one = app.clone();
            thread::spawn(move || {
                let mut title = String::new();
                match version_checker.get_latest_version(){
                    Ok(v) => {
                        title = v.to_string();
                    }
                    Err(_) => {
                        title = String::from("Unable to retrieve latest version");
                    }
                }

                let main_window = app_one.get_window("main").unwrap();
                main_window.emit("latest-version", title).unwrap();
            });

            let app_two = app.clone();
            let subscription = Arc::new(Subscription::new(Box::new(move |v| {
                //println!("{}", v);
                let main_window = app_two.get_window("main").unwrap();
                main_window.emit("latest-version", v.to_string()).unwrap();
                app_two.tray_handle().set_icon(tauri::Icon::Raw(include_bytes!("../icons/icon-blue.ico").to_vec())).unwrap();
                match Notification::new(&app_two.config().tauri.bundle.identifier)
                    .title("Aiyoyo! Got new build version!")
                    .body(format!("Mai tu liao! Must install {} right now!",v.to_string().as_str()))
                    .show() {
                    Ok(_) => {}
                    Err(e) => {
                        error!("Unable to show notification! Error: {}", e);
                    }
                }
            })));
            release_monitor.subscribe(Event::LatestVersion, subscription.clone());

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app_handle, event| match event {
            tauri::RunEvent::ExitRequested { api, .. } => {
                api.prevent_exit();
            }
            _ => {}
        });
}
