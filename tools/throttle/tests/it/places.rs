//! The real binary decides its own lock folder (user decision 2026-11-26): the environment, then `config.toml`, then the per-user state
//! folder; found through the user database when the process has no `HOME`; the old folder's history copied once.
#![cfg(unix)]

use crate::procutil::*;
use paddock_env::Os;
use std::fs;
use std::path::{Path, PathBuf};
use throttle::config::Config;
use throttle::places::Host;

/// The settings and state folder of the platform under `home` (macOS convention; XDG elsewhere, none of its variables set in a test).
fn state_under(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/throttle")
    } else {
        home.join(".local/state/throttle")
    }
}

fn config_under(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/throttle/config.toml")
    } else {
        home.join(".config/throttle/config.toml")
    }
}

fn dir_out(w: &World, env: &[(&str, &str)]) -> String {
    let (code, out, err) = heavy_out(w, &["dir"], env);
    assert_eq!(code, 0, "{err}");
    out.trim().to_string()
}

#[test]
fn the_precedence_is_the_environment_then_the_config_file_then_the_default() {
    let w = World::new();
    let home = w.path("home");
    // The default, with no folder in the environment.
    assert_eq!(dir_out(&w, &[("PLOCK_DIR", UNSET)]), state_under(&home).display().to_string());
    // The config file beats the default.
    let cfg = config_under(&home);
    fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    let chosen = w.path("chosen");
    fs::write(&cfg, format!("dir = '{}'\n", chosen.display())).unwrap();
    assert_eq!(dir_out(&w, &[("PLOCK_DIR", UNSET)]), chosen.display().to_string());
    // The environment beats the file, under every name.
    for k in ["THROTTLE_DIR", "PLOCK_DIR", "PLOCK_ROOT", "HEAVY_DIR"] {
        let env_dir = w.path("env-dir");
        let got = dir_out(&w, &[("PLOCK_DIR", UNSET), (k, env_dir.to_str().unwrap())]);
        assert_eq!(got, env_dir.display().to_string(), "{k}");
    }
}

/// What a launchd daemon with `UserName` set sees: no `HOME`, no XDG folder, no folder variable. The home then comes from the user
/// database, which the test injects through [`Host::user_home`] (the world's own folder stands in for the passwd entry): a test that ran
/// the binary read the real user's home and its `config.toml`, so it failed on any machine with one (ER-14; the coordinator's throttle
/// config of 2026-11-26). The resolver is the binary's own (`Config::from_env` with the host it builds), only the user database is fake.
#[test]
fn a_process_with_no_home_finds_it_through_the_user_database() {
    let w = World::new();
    let passwd_home = w.path("passwd-home");
    fs::create_dir_all(&passwd_home).unwrap();
    let user_home = || Some(passwd_home.clone());
    let host = Host {
        os: Os::current(),
        user_home: &user_home,
        read_file: &|p| fs::read_to_string(p).ok(),
        has_history: &|_| false,
        dev_targets: None,
    };
    let no_env = |_: &str| None;
    let (cfg, warnings) = Config::from_env(&no_env, &host);
    assert_eq!(cfg.root, state_under(&passwd_home), "the default under the user database's home");
    assert!(warnings.is_empty(), "{warnings:?}");
    // The settings file is read from that home too, and wins over the default.
    let cfg_file = config_under(&passwd_home);
    fs::create_dir_all(cfg_file.parent().unwrap()).unwrap();
    let chosen = w.path("chosen");
    fs::write(&cfg_file, format!("dir = '{}'\n", chosen.display())).unwrap();
    assert_eq!(
        Config::from_env(&no_env, &host).0.root,
        chosen,
        "the config file under the user database's home"
    );
}

#[test]
fn a_bad_config_file_is_said_and_the_default_used() {
    let w = World::new();
    let home = w.path("home");
    let cfg = config_under(&home);
    fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    fs::write(&cfg, "dir = \"relative\"\n").unwrap();
    let (code, out, err) = heavy_out(&w, &["dir"], &[("PLOCK_DIR", UNSET)]);
    assert_eq!((code, out.trim()), (0, state_under(&home).display().to_string().as_str()), "{err}");
    assert!(err.contains("config.toml") && err.contains("absolute"), "{err}");
}

#[test]
fn the_old_folders_history_is_copied_once_to_the_new_default() {
    let w = World::new();
    let home = w.path("home");
    let old = w.path("dev-targets/throttle");
    fs::create_dir_all(&old).unwrap();
    let old_file = old.join("heavy-history.tsv");
    fs::write(&old_file, "1000\t90000\told row\n").unwrap();
    let env = [("PLOCK_DIR", UNSET), ("THROTTLE_ROW", "new row")];
    let (code, _, e) = heavy_out(&w, &["run", "--no-gate", "M", "--", "true"], &env);
    assert_eq!(code, 0, "{e}");
    let new_file = state_under(&home).join("heavy-history.tsv");
    let text = fs::read_to_string(&new_file).unwrap();
    assert!(
        text.contains("\told row\n") && text.contains("\tnew row\n"),
        "the old lines came over: {text}"
    );
    // Once: a line added to the old file later is not copied, and the old file is left as it was.
    fs::write(&old_file, "2000\t1000\tlate row\n").unwrap();
    let (code, _, e) = heavy_out(&w, &["run", "--no-gate", "M", "--", "true"], &env);
    assert_eq!(code, 0, "{e}");
    let text = fs::read_to_string(&new_file).unwrap();
    assert!(!text.contains("late row"), "{text}");
    assert_eq!(fs::read_to_string(&old_file).unwrap(), "2000\t1000\tlate row\n");
}
