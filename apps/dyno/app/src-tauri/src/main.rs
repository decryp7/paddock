//! The Dyno app's entry point.
// No console window behind the app in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dyno_app_lib::run();
}
