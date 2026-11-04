//! The real program against real Win32 objects (ER-14: only folders, values and processes these tests make, inside a
//! throw-away folder under `%ProgramFiles%` of the Windows test VM and the VM's temporary folder; everything is removed).
//!
//! Run on the Windows test VM as an administrator (`docs/testing/windows-test-vm.md`):
//! `PADDOCK_SETUP_EXE` is a build with `PADDOCK_SETUP_IDENTIFIER=net.decryptology.paddock-setup-test`,
//! `PADDOCK_SETUP_PRODUCT=PaddockSetupTest`, `PADDOCK_SETUP_MAIN=paddock-setup.exe`, `PADDOCK_SETUP_VERSION=0.1.0` and
//! `PADDOCK_SETUP_UPDATE_KEY` = the text of `paddock-updater/tests/fixtures/key.pub`;
//! `PADDOCK_SETUP_EXE_NEWER` is the same build with `PADDOCK_SETUP_VERSION=9.9.9` (for the downgrade refusal).

use std::path::{Path, PathBuf};
use std::process::Command;

const UPDATE: &[u8] = include_bytes!("../../../paddock-updater/tests/fixtures/update.bin");
const UPDATE_SIG: &str = include_str!("../../../paddock-updater/tests/fixtures/update.bin.sig");
const OTHER_KEY_SIG: &str = include_str!("../../../paddock-updater/tests/fixtures/other-key.sig");
const OTHER_VERSION_SIG: &str = include_str!("../../../paddock-updater/tests/fixtures/other-version.sig");

fn exe(var: &str) -> PathBuf {
    PathBuf::from(std::env::var(var).unwrap_or_else(|_| panic!("{var} must name a paddock-setup.exe build (see the module docs)")))
}

fn program_files() -> PathBuf {
    PathBuf::from(std::env::var("ProgramFiles").expect("ProgramFiles"))
}

/// A folder `%ProgramFiles%\PaddockSetupTest-<pid>-<name>` removed on drop.
struct Site(PathBuf);

impl Site {
    fn new(name: &str) -> Site {
        let p = program_files().join(format!("PaddockSetupTest-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&p).expect("make the test folder (run as an administrator)");
        Site(p)
    }
    fn with_exe(name: &str, var: &str) -> Site {
        let s = Site::new(name);
        std::fs::copy(exe(var), s.0.join("paddock-setup.exe")).expect("copy the program");
        s
    }
    fn setup(&self) -> PathBuf {
        self.0.join("paddock-setup.exe")
    }
    fn updates(&self) -> PathBuf {
        self.0.join("updates")
    }
    fn log(&self) -> String {
        std::fs::read_to_string(self.updates().join("setup.log")).unwrap_or_default()
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = Command::new("cmd").args(["/c", "rmdir", "/s", "/q"]).arg(&self.0).status();
    }
}

fn run(program: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(program).args(args).output().expect("run paddock-setup");
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned())
}

fn scratch(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("paddock-setup-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn junction(link: &Path, target: &Path) {
    let st = Command::new("cmd").args(["/c", "mklink", "/J"]).arg(link).arg(target).output().expect("mklink");
    assert!(st.status.success(), "mklink: {}", String::from_utf8_lossy(&st.stdout));
}

#[test]
fn self_test_names_the_baked_values() {
    let (code, out) = run(&exe("PADDOCK_SETUP_EXE"), &["self-test"]);
    assert_eq!(code, 0);
    assert!(out.contains("identifier=net.decryptology.paddock-setup-test"), "{out}");
    assert!(out.contains("product=PaddockSetupTest") && out.contains("main=paddock-setup.exe"), "{out}");
    assert!(out.contains("key_fingerprint=") && !out.contains("key_fingerprint=none"), "{out}");
}

#[test]
fn the_binary_carries_the_update_description_in_its_version_resource() {
    let ps = format!("(Get-Item '{}').VersionInfo | ForEach-Object {{ $_.FileDescription; $_.ProductName; $_.FileVersion }}", exe("PADDOCK_SETUP_EXE").display());
    let out = Command::new("powershell").args(["-NoProfile", "-Command", &ps]).output().expect("powershell");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert_eq!(lines, ["PaddockSetupTest update", "PaddockSetupTest", "0.1.0"], "{text}");
}

#[test]
fn same_folder_ignores_case_and_a_trailing_backslash_and_follows_a_junction_by_its_final_name() {
    let a = scratch("same-a");
    let upper = a.to_string_lossy().to_uppercase();
    let with_slash = format!("{}\\", a.display());
    let p = exe("PADDOCK_SETUP_EXE");
    assert_eq!(run(&p, &["same-folder", "--a", &a.to_string_lossy(), "--b", &with_slash]).0, 0);
    assert_eq!(run(&p, &["same-folder", "--a", &a.to_string_lossy(), "--b", &upper]).0, 0);
    let b = scratch("same-b");
    assert_eq!(run(&p, &["same-folder", "--a", &a.to_string_lossy(), "--b", &b.to_string_lossy()]).0, 1);
    let missing = a.join("missing");
    assert_eq!(run(&p, &["same-folder", "--a", &a.to_string_lossy(), "--b", &missing.to_string_lossy()]).0, 1);
    // An 8.3 short name names the same folder.
    let long = scratch("a-folder-with-a-long-name");
    let short = Command::new("cmd").args(["/c", "for", "%I", "in"]).arg(format!("(\"{}\")", long.display())).args(["do", "@echo", "%~sI"]).output().unwrap();
    let short = String::from_utf8_lossy(&short.stdout).trim().to_string();
    if !short.is_empty() && !short.eq_ignore_ascii_case(&long.to_string_lossy()) {
        assert_eq!(run(&p, &["same-folder", "--a", &long.to_string_lossy(), "--b", &short]).0, 0, "{short}");
    }
    // A junction and its target are different final names only when asked about the link's own folder; the target is the same.
    let link = a.join("link");
    junction(&link, &b);
    assert_eq!(run(&p, &["same-folder", "--a", &link.to_string_lossy(), "--b", &b.to_string_lossy()]).0, 0);
    let _ = std::fs::remove_dir_all(a);
    let _ = std::fs::remove_dir_all(b);
    let _ = std::fs::remove_dir_all(long);
}

#[test]
fn check_dir_accepts_a_program_files_child_makes_updates_and_opens_the_log() {
    let site = Site::new("ok");
    let (code, out) = run(&exe("PADDOCK_SETUP_EXE"), &["check-dir", "--dir", &site.0.to_string_lossy()]);
    assert_eq!((code, out.starts_with("ok ")), (0, true), "{out}");
    assert!(site.updates().is_dir());
    let log = site.log();
    assert!(log.contains("phase=check-dir folder accepted"), "{log}");
    assert!(!log.contains(&program_files().to_string_lossy().into_owned()), "no profile or install paths in the log");
}

#[test]
fn check_dir_refuses_the_temp_folder_and_writes_nothing() {
    let dir = scratch("refused");
    let (code, out) = run(&exe("PADDOCK_SETUP_EXE"), &["check-dir", "--dir", &dir.to_string_lossy()]);
    assert_eq!(code, 3, "{out}");
    assert!(out.starts_with("Install PaddockSetupTest in Program Files"), "{out}");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "a refusal writes nothing");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn check_dir_refuses_a_nested_folder_and_a_junction_in_program_files() {
    let nested = Site::new("nested");
    let inner = nested.0.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    assert_eq!(run(&exe("PADDOCK_SETUP_EXE"), &["check-dir", "--dir", &inner.to_string_lossy()]).0, 3);
    let target = scratch("junction-target");
    let link = program_files().join(format!("PaddockSetupTest-{}-junction", std::process::id()));
    junction(&link, &target);
    let (code, _) = run(&exe("PADDOCK_SETUP_EXE"), &["check-dir", "--dir", &link.to_string_lossy()]);
    let _ = Command::new("cmd").args(["/c", "rmdir"]).arg(&link).status();
    assert_eq!(code, 3);
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0, "nothing is written through the junction");
    let _ = std::fs::remove_dir_all(target);
}

#[test]
fn a_hostile_updates_folder_is_renamed_aside() {
    let site = Site::new("aside");
    let target = scratch("aside-target");
    junction(&site.updates(), &target);
    let (code, _) = run(&exe("PADDOCK_SETUP_EXE"), &["check-dir", "--dir", &site.0.to_string_lossy()]);
    assert_eq!(code, 0);
    assert!(site.0.join("updates.refused-1").exists(), "the junction was renamed aside");
    assert!(site.updates().is_dir() && site.log().contains("folder accepted"));
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0, "nothing was written through the junction");
    let _ = Command::new("cmd").args(["/c", "rmdir"]).arg(site.0.join("updates.refused-1")).status();
    let _ = std::fs::remove_dir_all(target);
}

fn reg(args: &[&str]) -> bool {
    Command::new("reg").args(args).output().map(|o| o.status.success()).unwrap_or(false)
}

#[test]
fn clean_run_removes_machine_values_that_point_into_the_folder_only() {
    const KEY: &str = r"HKLM\Software\Microsoft\Windows\CurrentVersion\Run";
    let site = Site::new("run");
    let mine = format!("PaddockSetupTestMine{}", std::process::id());
    let other = format!("PaddockSetupTestOther{}", std::process::id());
    let command = format!("\"{}\\app.exe\" --hidden", site.0.display());
    assert!(reg(&["add", KEY, "/v", &mine, "/t", "REG_SZ", "/d", &command, "/f"]));
    assert!(reg(&["add", KEY, "/v", &other, "/t", "REG_SZ", "/d", r"C:\Windows\System32\notepad.exe", "/f"]));
    let (code, _) = run(&exe("PADDOCK_SETUP_EXE"), &["clean-run", "--dir", &site.0.to_string_lossy(), "--scope", "machine"]);
    let (mine_left, other_left) = (reg(&["query", KEY, "/v", &mine]), reg(&["query", KEY, "/v", &other]));
    reg(&["delete", KEY, "/v", &other, "/f"]);
    reg(&["delete", KEY, "/v", &mine, "/f"]);
    assert_eq!((code, mine_left, other_left), (0, false, true));
    assert!(site.log().contains("phase=clean-run removed 1 value(s)"), "{}", site.log());
}

#[test]
fn clean_staging_keeps_only_the_log() {
    let site = Site::new("staging");
    assert_eq!(run(&exe("PADDOCK_SETUP_EXE"), &["check-dir", "--dir", &site.0.to_string_lossy()]).0, 0);
    std::fs::write(site.updates().join("update-1.0.0.exe"), b"x").unwrap();
    std::fs::create_dir_all(site.updates().join("tmp")).unwrap();
    std::fs::write(site.updates().join("tmp").join("a"), b"x").unwrap();
    let (code, _) = run(&exe("PADDOCK_SETUP_EXE"), &["clean-staging", "--dir", &site.0.to_string_lossy()]);
    assert_eq!(code, 0);
    let mut left: Vec<String> = std::fs::read_dir(site.updates()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, ["setup.log"]);
}

#[test]
fn stop_all_without_the_stopper_says_so_and_fails_cleanly() {
    let site = Site::new("stop");
    let (code, _) = run(&exe("PADDOCK_SETUP_EXE"), &["stop-all", "--dir", &site.0.to_string_lossy(), "--images", "nothing.exe"]);
    assert_eq!(code, 5);
    assert!(site.log().contains("no process stopper"), "{}", site.log());
}

fn write_download(name: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(name);
    let file = dir.join("Update_9.9.9_x64-setup.exe");
    std::fs::write(&file, UPDATE).unwrap();
    (dir, file)
}

fn sig_file(dir: &Path, text: &str) -> PathBuf {
    let p = dir.join("update.sig");
    std::fs::write(&p, text.trim()).unwrap();
    p
}

fn apply(site: &Site, file: &Path, sig: &Path, version: &str) -> (i32, String) {
    run(&site.setup(), &["apply-update", "--file", &file.to_string_lossy(), "--signature", &sig.to_string_lossy(), "--version", version, "--size", &UPDATE.len().to_string(), "--restart"])
}

/// The fixture signature file is the base64 of the signature text, which is what `latest.json` carries; the file holds that text.
#[test]
fn apply_update_copies_verifies_and_reaches_the_start() {
    let site = Site::with_exe("apply", "PADDOCK_SETUP_EXE");
    let (dir, file) = write_download("apply-src");
    let sig = sig_file(&dir, UPDATE_SIG);
    let (code, _) = apply(&site, &file, &sig, "9.9.9");
    // The fixture is signed but is not a program: every step up to the start passes, the start itself is refused by Windows.
    assert_eq!(code, 28, "{}", site.log());
    let log = site.log();
    for needle in ["copied 49152 bytes", "signature and signed version verified on the held copy", "refused: The installer could not be started"] {
        assert!(log.contains(needle), "{needle} not in\n{log}");
    }
    assert!(site.updates().join("update-9.9.9.exe").is_file());
    assert!(site.updates().join("tmp").is_dir());
    assert!(!log.contains("Users"), "no profile paths in the log");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn apply_update_refuses_a_wrong_key_a_wrong_signed_version_and_garbage() {
    let site = Site::with_exe("refuse", "PADDOCK_SETUP_EXE");
    let (dir, file) = write_download("refuse-src");
    for (text, version) in [(OTHER_KEY_SIG, "9.9.9"), (OTHER_VERSION_SIG, "9.9.9"), (UPDATE_SIG, "9.9.8"), ("not a signature", "9.9.9")] {
        let sig = sig_file(&dir, text);
        let (code, _) = apply(&site, &file, &sig, version);
        assert_eq!(code, 26, "{text:.20} {version}\n{}", site.log());
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn apply_update_refuses_an_update_that_is_not_newer() {
    let site = Site::with_exe("downgrade", "PADDOCK_SETUP_EXE_NEWER");
    let (dir, file) = write_download("downgrade-src");
    let sig = sig_file(&dir, UPDATE_SIG);
    let (code, _) = apply(&site, &file, &sig, "9.9.9");
    assert_eq!(code, 27, "{}", site.log());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn apply_update_refuses_a_directory_a_junction_and_a_file_larger_than_announced() {
    let site = Site::with_exe("sources", "PADDOCK_SETUP_EXE");
    let (dir, file) = write_download("sources-src");
    let sig = sig_file(&dir, UPDATE_SIG);
    // A directory.
    assert_eq!(apply(&site, &dir, &sig, "9.9.9").0, 25, "a directory cannot be read as a file: {}", site.log());
    // A junction to a directory.
    let link = dir.join("link");
    junction(&link, &scratch("sources-target"));
    let (code, _) = apply(&site, &link, &sig, "9.9.9");
    assert!(code == 23 || code == 25, "{code}");
    // Larger than announced.
    let (code, _) = run(&site.setup(), &["apply-update", "--file", &file.to_string_lossy(), "--signature", &sig.to_string_lossy(), "--version", "9.9.9", "--size", "100", "--restart"]);
    assert_eq!(code, 24, "{}", site.log());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn apply_update_outside_program_files_is_refused_before_anything_is_written() {
    let dir = scratch("elsewhere");
    std::fs::copy(exe("PADDOCK_SETUP_EXE"), dir.join("paddock-setup.exe")).unwrap();
    let (src, file) = write_download("elsewhere-src");
    let sig = sig_file(&src, UPDATE_SIG);
    let (code, _) = run(&dir.join("paddock-setup.exe"), &["apply-update", "--file", &file.to_string_lossy(), "--signature", &sig.to_string_lossy(), "--version", "9.9.9", "--restart"]);
    assert_eq!(code, 22);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "only the program itself is there");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(src);
}

#[test]
fn a_hostile_programdata_paddock_folder_is_never_touched() {
    let site = Site::with_exe("programdata", "PADDOCK_SETUP_EXE");
    let pd = PathBuf::from(std::env::var("ProgramData").expect("ProgramData")).join("Paddock");
    let existed = pd.exists();
    std::fs::create_dir_all(&pd).unwrap();
    let before: Vec<_> = std::fs::read_dir(&pd).unwrap().map(|e| e.unwrap().file_name()).collect();
    let (dir, file) = write_download("pd-src");
    let sig = sig_file(&dir, UPDATE_SIG);
    let _ = apply(&site, &file, &sig, "9.9.9");
    let _ = run(&site.setup(), &["clean-staging", "--dir", &site.0.to_string_lossy()]);
    let after: Vec<_> = std::fs::read_dir(&pd).unwrap().map(|e| e.unwrap().file_name()).collect();
    if !existed {
        let _ = std::fs::remove_dir(&pd);
    }
    assert_eq!(before, after, "nothing was written under ProgramData");
    let _ = std::fs::remove_dir_all(dir);
}
