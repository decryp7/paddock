//! A stand-in for an app's NSIS installer in the Windows tests (`tests/vm/windows.rs`): records how it was started
//! (arguments, `TEMP`, `TMP`, its own path) in `%TEMP%\fake-installer.txt`. When `PADDOCK_FAKE_HOLD` names a
//! `paddock-setup.exe` it starts that program's `hold-installing` as the real include does and stays alive for a few
//! seconds, so the hand-over from `apply-update` can be observed. Test support only; it installs nothing.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let line = |name: &str| format!("{name}={}\n", std::env::var(name).unwrap_or_default());
    let me = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let text = format!(
        "args={}\n{}{}exe={me}\n",
        args.join(" "),
        line("TEMP"),
        line("TMP")
    );
    let temp = std::env::var_os("TEMP")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    // Written under a temporary name and renamed, so a reader never sees half a file.
    let tmp = temp.join("fake-installer.txt.part");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, temp.join("fake-installer.txt"));
    }
    if let Some(holder) = std::env::var_os("PADDOCK_FAKE_HOLD") {
        // `--dir` makes hold-installing log into the install folder's setup.log, as the real include's call does.
        let dir = std::path::Path::new(&holder)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let _ = std::process::Command::new(&holder)
            .arg("hold-installing")
            .arg("--dir")
            .arg(dir)
            .spawn();
        let seconds = std::env::var("PADDOCK_FAKE_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4);
        std::thread::sleep(std::time::Duration::from_secs(seconds));
    }
}
