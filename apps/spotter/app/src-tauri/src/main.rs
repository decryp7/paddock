//! The Spotter app's entry point.
// No console window behind the app in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    spotter_app_lib::run();
}
