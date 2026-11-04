//! The real program against real Win32 objects. Opt-in: every test is `#[ignore]` and also refuses to run without
//! `PADDOCK_SETUP_VM=1`, which only the VM script sets (`cargo test -- --ignored`). ER-14: CI never creates Program Files
//! folders or HKLM values; the tests touch only folders, values and processes they make, inside throw-away folders under
//! `%ProgramFiles%` of the Windows test VM and its temporary folder, and remove them.
//!
//! Run on the Windows test VM as an administrator (`docs/testing/windows-test-vm.md`):
//! `PADDOCK_SETUP_EXE` is a build with `PADDOCK_SETUP_IDENTIFIER=net.decryptology.paddock-setup-test`,
//! `PADDOCK_SETUP_PRODUCT=PaddockSetupTest`, `PADDOCK_SETUP_MAIN=paddock-setup.exe`, `PADDOCK_SETUP_VERSION=0.1.0` and
//! `PADDOCK_SETUP_UPDATE_KEY` = the text of `paddock-updater/tests/fixtures/key.pub`;
//! `PADDOCK_SETUP_EXE_STOP` is a build on a tree that has `paddock_proc::installed` (cfg `has_proc_installed`);
//! `PADDOCK_SETUP_EXE_E2E_NEWER` is the e2e build with `PADDOCK_SETUP_VERSION=9.9.9` (for the downgrade refusal).
//! The success path needs a program that really is signed: `PADDOCK_SETUP_EXE_E2E` is a 0.1.0 build with a throw-away
//! test key, `PADDOCK_SETUP_E2E_INSTALLER` the `fake_installer` example signed for 9.9.9 with that key and
//! `PADDOCK_SETUP_E2E_SIGNATURE` the file with its signature text (made by the script that runs these tests);
//! `PADDOCK_SETUP_E2E_SIGNATURE_OTHER` is the same bytes signed under another product's file name.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Why these tests do not run by default: they create folders in Program Files and values in HKLM (ER-14).
const VM_ONLY: &str = "creates folders under Program Files and HKLM values: runs only on the Windows test VM with PADDOCK_SETUP_VM=1";

/// The explicit opt-in. A run without it (CI, a developer's PC) fails loudly instead of touching the machine.
fn gate() {
    assert_eq!(
        std::env::var("PADDOCK_SETUP_VM").as_deref(),
        Ok("1"),
        "{VM_ONLY}"
    );
}

const UPDATE: &[u8] = include_bytes!("../../../paddock-updater/tests/fixtures/update.bin");
const UPDATE_SIG: &str = include_str!("../../../paddock-updater/tests/fixtures/update.bin.sig");
const OTHER_KEY_SIG: &str = include_str!("../../../paddock-updater/tests/fixtures/other-key.sig");
const OTHER_VERSION_SIG: &str =
    include_str!("../../../paddock-updater/tests/fixtures/other-version.sig");

fn exe(var: &str) -> PathBuf {
    gate();
    PathBuf::from(std::env::var(var).unwrap_or_else(|_| {
        panic!("{var} must name a paddock-setup.exe build (see the module docs)")
    }))
}

fn program_files() -> PathBuf {
    PathBuf::from(std::env::var("ProgramFiles").expect("ProgramFiles"))
}

/// A folder `%ProgramFiles%\PaddockSetupTest-<pid>-<name>` removed on drop.
struct Site(PathBuf);

impl Site {
    fn new(name: &str) -> Site {
        gate();
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
        // Windows Defender may still be scanning a program that was just started; remove with a bounded retry.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while self.0.exists() && std::time::Instant::now() < deadline {
            let _ = Command::new("cmd")
                .args(["/c", "rmdir", "/s", "/q"])
                .arg(&self.0)
                .output();
            if self.0.exists() {
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
        if self.0.exists() {
            eprintln!("could not remove the test folder {}", self.0.display());
        }
    }
}

fn run(program: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(program)
        .args(args)
        .output()
        .expect("run paddock-setup");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn scratch(name: &str) -> PathBuf {
    gate();
    let p = std::env::temp_dir().join(format!("paddock-setup-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn junction(link: &Path, target: &Path) {
    let st = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("mklink");
    assert!(
        st.status.success(),
        "mklink: {}",
        String::from_utf8_lossy(&st.stdout)
    );
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn self_test_names_the_baked_values() {
    let (code, out) = run(&exe("PADDOCK_SETUP_EXE"), &["self-test"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("identifier=net.decryptology.paddock-setup-test"),
        "{out}"
    );
    assert!(
        out.contains("product=PaddockSetupTest") && out.contains("main=paddock-setup.exe"),
        "{out}"
    );
    assert!(
        out.contains("key_fingerprint=") && !out.contains("key_fingerprint=none"),
        "{out}"
    );
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn the_binary_carries_the_update_description_in_its_version_resource() {
    let ps = format!(
        "(Get-Item '{}').VersionInfo | ForEach-Object {{ $_.FileDescription; $_.ProductName; $_.FileVersion }}",
        exe("PADDOCK_SETUP_EXE").display()
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .output()
        .expect("powershell");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert_eq!(
        lines,
        ["PaddockSetupTest update", "PaddockSetupTest", "0.1.0"],
        "{text}"
    );
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn same_folder_ignores_case_a_trailing_backslash_and_short_names_but_not_a_junction() {
    let a = scratch("same-a");
    let upper = a.to_string_lossy().to_uppercase();
    let with_slash = format!("{}\\", a.display());
    let p = exe("PADDOCK_SETUP_EXE");
    assert_eq!(
        run(
            &p,
            &[
                "same-folder",
                "--a",
                &a.to_string_lossy(),
                "--b",
                &with_slash
            ]
        )
        .0,
        0
    );
    assert_eq!(
        run(
            &p,
            &["same-folder", "--a", &a.to_string_lossy(), "--b", &upper]
        )
        .0,
        0
    );
    let b = scratch("same-b");
    assert_eq!(
        run(
            &p,
            &[
                "same-folder",
                "--a",
                &a.to_string_lossy(),
                "--b",
                &b.to_string_lossy()
            ]
        )
        .0,
        1
    );
    let missing = a.join("missing");
    assert_eq!(
        run(
            &p,
            &[
                "same-folder",
                "--a",
                &a.to_string_lossy(),
                "--b",
                &missing.to_string_lossy()
            ]
        )
        .0,
        1
    );
    // An 8.3 short name names the same folder.
    let long = scratch("a-folder-with-a-long-name");
    let ps = format!(
        "(New-Object -ComObject Scripting.FileSystemObject).GetFolder('{}').ShortPath",
        long.display()
    );
    let short = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .output()
        .unwrap();
    let short = String::from_utf8_lossy(&short.stdout).trim().to_string();
    assert!(
        !short.is_empty() && !short.eq_ignore_ascii_case(&long.to_string_lossy()),
        "the test folder has no 8.3 name: {short}"
    );
    {
        assert_eq!(
            run(
                &p,
                &["same-folder", "--a", &long.to_string_lossy(), "--b", &short]
            )
            .0,
            0,
            "{short}"
        );
    }
    // A junction is opened without following it, so it is not the same folder as its target.
    let link = a.join("link");
    junction(&link, &b);
    assert_eq!(
        run(
            &p,
            &[
                "same-folder",
                "--a",
                &link.to_string_lossy(),
                "--b",
                &b.to_string_lossy()
            ]
        )
        .0,
        1
    );
    let _ = std::fs::remove_dir_all(a);
    let _ = std::fs::remove_dir_all(b);
    let _ = std::fs::remove_dir_all(long);
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn check_dir_accepts_a_program_files_child_makes_updates_and_opens_the_log() {
    let site = Site::new("ok");
    let (code, out) = run(
        &exe("PADDOCK_SETUP_EXE"),
        &["check-dir", "--dir", &site.0.to_string_lossy()],
    );
    assert_eq!((code, out.starts_with("ok ")), (0, true), "{out}");
    assert!(site.updates().is_dir());
    let log = site.log();
    assert!(log.contains("phase=check-dir folder accepted"), "{log}");
    assert!(
        !log.contains(&program_files().to_string_lossy().into_owned()),
        "no profile or install paths in the log"
    );
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn check_dir_refuses_the_temp_folder_and_writes_nothing() {
    let dir = scratch("refused");
    let (code, out) = run(
        &exe("PADDOCK_SETUP_EXE"),
        &["check-dir", "--dir", &dir.to_string_lossy()],
    );
    assert_eq!(code, 3, "{out}");
    assert!(
        out.starts_with("Install PaddockSetupTest in Program Files"),
        "{out}"
    );
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        0,
        "a refusal writes nothing"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn check_dir_refuses_a_nested_folder_and_a_junction_in_program_files() {
    let nested = Site::new("nested");
    let inner = nested.0.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    assert_eq!(
        run(
            &exe("PADDOCK_SETUP_EXE"),
            &["check-dir", "--dir", &inner.to_string_lossy()]
        )
        .0,
        3
    );
    let target = scratch("junction-target");
    let link = program_files().join(format!("PaddockSetupTest-{}-junction", std::process::id()));
    junction(&link, &target);
    let (code, _) = run(
        &exe("PADDOCK_SETUP_EXE"),
        &["check-dir", "--dir", &link.to_string_lossy()],
    );
    let _ = Command::new("cmd")
        .args(["/c", "rmdir"])
        .arg(&link)
        .status();
    assert_eq!(code, 3);
    assert_eq!(
        std::fs::read_dir(&target).unwrap().count(),
        0,
        "nothing is written through the junction"
    );
    let _ = std::fs::remove_dir_all(target);
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn a_hostile_updates_folder_is_renamed_aside() {
    let site = Site::new("aside");
    let target = scratch("aside-target");
    junction(&site.updates(), &target);
    let (code, _) = run(
        &exe("PADDOCK_SETUP_EXE"),
        &["check-dir", "--dir", &site.0.to_string_lossy()],
    );
    assert_eq!(code, 0);
    assert!(
        site.0.join("updates.refused-1").exists(),
        "the junction was renamed aside"
    );
    assert!(site.updates().is_dir() && site.log().contains("folder accepted"));
    assert_eq!(
        std::fs::read_dir(&target).unwrap().count(),
        0,
        "nothing was written through the junction"
    );
    let _ = Command::new("cmd")
        .args(["/c", "rmdir"])
        .arg(site.0.join("updates.refused-1"))
        .status();
    let _ = std::fs::remove_dir_all(target);
}

fn reg(args: &[&str]) -> bool {
    Command::new("reg")
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn clean_run_removes_machine_values_that_point_into_the_folder_only() {
    const KEY: &str = r"HKLM\Software\Microsoft\Windows\CurrentVersion\Run";
    let site = Site::new("run");
    let mine = format!("PaddockSetupTestMine{}", std::process::id());
    let other = format!("PaddockSetupTestOther{}", std::process::id());
    let command = format!("\"{}\\app.exe\" --hidden", site.0.display());
    assert!(reg(&[
        "add", KEY, "/v", &mine, "/t", "REG_SZ", "/d", &command, "/f"
    ]));
    assert!(reg(&[
        "add",
        KEY,
        "/v",
        &other,
        "/t",
        "REG_SZ",
        "/d",
        r"C:\Windows\System32\notepad.exe",
        "/f"
    ]));
    let (code, _) = run(
        &exe("PADDOCK_SETUP_EXE"),
        &[
            "clean-run",
            "--dir",
            &site.0.to_string_lossy(),
            "--scope",
            "machine",
        ],
    );
    let (mine_left, other_left) = (
        reg(&["query", KEY, "/v", &mine]),
        reg(&["query", KEY, "/v", &other]),
    );
    reg(&["delete", KEY, "/v", &other, "/f"]);
    reg(&["delete", KEY, "/v", &mine, "/f"]);
    assert_eq!((code, mine_left, other_left), (0, false, true));
    assert!(
        site.log().contains("phase=clean-run removed 1 value(s)"),
        "{}",
        site.log()
    );
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn clean_staging_keeps_only_the_log() {
    let site = Site::new("staging");
    assert_eq!(
        run(
            &exe("PADDOCK_SETUP_EXE"),
            &["check-dir", "--dir", &site.0.to_string_lossy()]
        )
        .0,
        0
    );
    std::fs::write(site.updates().join("update-1.0.0.exe"), b"x").unwrap();
    std::fs::create_dir_all(site.updates().join("tmp")).unwrap();
    std::fs::write(site.updates().join("tmp").join("a"), b"x").unwrap();
    let (code, _) = run(
        &exe("PADDOCK_SETUP_EXE"),
        &["clean-staging", "--dir", &site.0.to_string_lossy()],
    );
    assert_eq!(code, 0);
    let mut left: Vec<String> = std::fs::read_dir(site.updates())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["setup.log"]);
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn stop_all_without_the_stopper_says_so_and_fails_cleanly() {
    let site = Site::new("stop");
    let (code, _) = run(
        &exe("PADDOCK_SETUP_EXE"),
        &[
            "stop-all",
            "--dir",
            &site.0.to_string_lossy(),
            "--images",
            "nothing.exe",
        ],
    );
    assert_eq!(code, 5);
    assert!(site.log().contains("no process stopper"), "{}", site.log());
}

// ---- apply-update ----

fn write_download(name: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(name);
    let file = dir.join("Update_9.9.9_x64-setup.exe");
    std::fs::write(&file, UPDATE).unwrap();
    (dir, file)
}

/// The signature text as the app passes it: one line of base64.
fn sig_text(text: &str) -> String {
    text.trim().to_string()
}

fn apply_with(program: &Path, file: &Path, sig: &str, version: &str) -> (i32, String) {
    run(
        program,
        &[
            "apply-update",
            "--file",
            &file.to_string_lossy(),
            "--signature",
            sig,
            "--version",
            version,
            "--size",
            &std::fs::metadata(file)
                .map(|m| m.len())
                .unwrap_or(0)
                .to_string(),
            "--restart",
        ],
    )
}

fn apply(site: &Site, file: &Path, sig: &str, version: &str) -> (i32, String) {
    apply_with(&site.setup(), file, sig, version)
}

/// What is in `updates\` apart from the log: nothing may be left behind by a refused update.
fn leftovers(site: &Site) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(site.updates())
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.retain(|n| n != "setup.log");
    v
}

/// The e2e installer with a signature for `version` (the script signs 9.9.9 and 9.9.8).
fn e2e_files_for(version: &str) -> (PathBuf, String, String) {
    let installer = PathBuf::from(
        std::env::var("PADDOCK_SETUP_E2E_INSTALLER").expect("PADDOCK_SETUP_E2E_INSTALLER"),
    );
    let read = |var: String| {
        sig_text(
            &std::fs::read_to_string(std::env::var(&var).unwrap_or_else(|_| panic!("{var}")))
                .unwrap(),
        )
    };
    let suffix = if version == "9.9.9" {
        String::new()
    } else {
        format!("_{}", version.replace('.', "_"))
    };
    (
        installer,
        read(format!("PADDOCK_SETUP_E2E_SIGNATURE{suffix}")),
        read("PADDOCK_SETUP_E2E_SIGNATURE_OTHER".to_string()),
    )
}

fn e2e_files() -> (PathBuf, String, String) {
    let installer = PathBuf::from(
        std::env::var("PADDOCK_SETUP_E2E_INSTALLER").expect("PADDOCK_SETUP_E2E_INSTALLER"),
    );
    let read = |var: &str| {
        sig_text(
            &std::fs::read_to_string(std::env::var(var).unwrap_or_else(|_| panic!("{var}")))
                .unwrap(),
        )
    };
    (
        installer,
        read("PADDOCK_SETUP_E2E_SIGNATURE"),
        read("PADDOCK_SETUP_E2E_SIGNATURE_OTHER"),
    )
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn a_validly_signed_update_of_another_family_app_is_refused_and_leaves_nothing() {
    // The fixture is signed with the right key but for `p-9.9.9.bin`; the e2e installer under another product's name.
    let site = Site::with_exe("otherapp", "PADDOCK_SETUP_EXE");
    let (dir, file) = write_download("otherapp-src");
    let (code, out) = apply(&site, &file, &sig_text(UPDATE_SIG), "9.9.9");
    assert_eq!(code, 29, "{out}\n{}", site.log());
    let log = site.log();
    assert!(
        log.contains("signature and signed version verified on the held copy"),
        "the signature itself is good: {log}"
    );
    assert!(
        log.contains("phase=apply-update refused: The downloaded update is for another app"),
        "{log}"
    );
    assert_eq!(
        leftovers(&site),
        Vec::<String>::new(),
        "a refused update leaves nothing in updates\\"
    );
    assert!(
        out.contains("for another app"),
        "the refusal is printed: {out}"
    );
    let site2 = Site::with_exe("otherapp-e2e", "PADDOCK_SETUP_EXE_E2E");
    let (installer, _good, other) = e2e_files();
    let (code2, _) = apply(&site2, &installer, &other, "9.9.9");
    assert_eq!(code2, 29, "{}", site2.log());
    assert_eq!(leftovers(&site2), Vec::<String>::new());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn apply_update_refuses_a_wrong_key_a_wrong_signed_version_and_garbage_and_leaves_nothing() {
    let site = Site::with_exe("refuse", "PADDOCK_SETUP_EXE");
    let (dir, file) = write_download("refuse-src");
    for (text, version) in [
        (OTHER_KEY_SIG, "9.9.9"),
        (OTHER_VERSION_SIG, "9.9.9"),
        (UPDATE_SIG, "9.9.8"),
        ("bm90IGEgc2lnbmF0dXJl", "9.9.9"),
    ] {
        let (code, _) = apply(&site, &file, &sig_text(text), version);
        assert_eq!(code, 26, "{text:.20} {version}\n{}", site.log());
        assert_eq!(leftovers(&site), Vec::<String>::new());
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn apply_update_refuses_an_update_older_than_the_installed_version_and_leaves_nothing() {
    // The installed copy says 9.9.9, the update is a correctly signed 9.9.9 of this product: not newer? A release over the
    // same numbers is allowed (it may be the release of an installed prerelease), so the update announced here is older.
    let site = Site::with_exe("downgrade", "PADDOCK_SETUP_EXE_E2E_NEWER");
    let (installer, good_8, _other) = e2e_files_for("9.9.8");
    let (code, _) = apply(&site, &installer, &good_8, "9.9.8");
    assert_eq!(code, 27, "{}", site.log());
    assert_eq!(leftovers(&site), Vec::<String>::new());
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn apply_update_refuses_hostile_file_arguments_before_opening_anything() {
    let site = Site::with_exe("hostile", "PADDOCK_SETUP_EXE");
    let sig = sig_text(UPDATE_SIG);
    let (dir, file) = write_download("hostile-src");
    // A named pipe: the elevated process must never open it (a server behind the name could answer it).
    let pipe = format!(r"\\.\pipe\paddock-setup-test-{}", std::process::id());
    let (c, _) = run(
        &site.setup(),
        &[
            "apply-update",
            "--file",
            &pipe,
            "--signature",
            &sig,
            "--version",
            "9.9.9",
            "--restart",
        ],
    );
    assert_eq!(c, 4, "a pipe path is a usage error");
    // UNC, device and namespace paths.
    for hostile in [
        r"\\localhost\C$\Windows\System32\cmd.exe",
        r"\\?\C:\Windows\System32\cmd.exe",
        r"\\.\C:\x",
        r"C:\Windows\System32\cmd.exe:evil",
        r"C:\NUL",
    ] {
        let (c, _) = run(
            &site.setup(),
            &[
                "apply-update",
                "--file",
                hostile,
                "--signature",
                &sig,
                "--version",
                "9.9.9",
                "--restart",
            ],
        );
        assert_eq!(c, 4, "{hostile}");
    }
    // The signature argument is text: a path (even a local one) is refused.
    let (c, _) = run(
        &site.setup(),
        &[
            "apply-update",
            "--file",
            &file.to_string_lossy(),
            "--signature",
            &file.to_string_lossy(),
            "--version",
            "9.9.9",
            "--restart",
        ],
    );
    assert_eq!(c, 4);
    assert!(
        !site.updates().exists(),
        "usage errors happen before any folder is made"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn apply_update_refuses_a_hard_link_a_symlink_a_junction_and_a_file_larger_than_announced() {
    let site = Site::with_exe("sources", "PADDOCK_SETUP_EXE");
    let (dir, file) = write_download("sources-src");
    let sig = sig_text(UPDATE_SIG);
    // A second hard link: the file can still be changed through the other name.
    let linked = dir.join("linked.exe");
    assert!(
        Command::new("cmd")
            .args(["/c", "mklink", "/H"])
            .arg(&linked)
            .arg(&file)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(apply(&site, &linked, &sig, "9.9.9").0, 23, "{}", site.log());
    std::fs::remove_file(&linked).unwrap();
    // A symbolic link to the file is a reparse point.
    let sym = dir.join("sym.exe");
    assert!(
        Command::new("cmd")
            .args(["/c", "mklink"])
            .arg(&sym)
            .arg(&file)
            .output()
            .unwrap()
            .status
            .success(),
        "mklink needs an elevated administrator"
    );
    assert_eq!(apply(&site, &sym, &sig, "9.9.9").0, 23, "{}", site.log());
    // A junction (a folder) cannot be read as a file.
    let junc = dir.join("junction");
    junction(&junc, &scratch("sources-target"));
    assert_eq!(apply(&site, &junc, &sig, "9.9.9").0, 25, "{}", site.log());
    // Larger than announced.
    let (code, _) = run(
        &site.setup(),
        &[
            "apply-update",
            "--file",
            &file.to_string_lossy(),
            "--signature",
            &sig,
            "--version",
            "9.9.9",
            "--size",
            "100",
            "--restart",
        ],
    );
    assert_eq!(code, 24, "{}", site.log());
    assert_eq!(leftovers(&site), Vec::<String>::new());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn apply_update_outside_program_files_is_refused_before_anything_is_written() {
    let dir = scratch("elsewhere");
    std::fs::copy(exe("PADDOCK_SETUP_EXE"), dir.join("paddock-setup.exe")).unwrap();
    let (src, file) = write_download("elsewhere-src");
    let (code, out) = apply_with(
        &dir.join("paddock-setup.exe"),
        &file,
        &sig_text(UPDATE_SIG),
        "9.9.9",
    );
    assert_eq!(code, 22);
    assert!(
        out.contains("protected Program Files folder"),
        "the refusal is printed: {out}"
    );
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        1,
        "only the program itself is there"
    );
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(src);
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn a_hostile_programdata_paddock_folder_is_never_touched() {
    let site = Site::with_exe("programdata", "PADDOCK_SETUP_EXE");
    let pd = PathBuf::from(std::env::var("ProgramData").expect("ProgramData")).join("Paddock");
    let existed = pd.exists();
    std::fs::create_dir_all(&pd).unwrap();
    let before: Vec<_> = std::fs::read_dir(&pd)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let (dir, file) = write_download("pd-src");
    let _ = apply(&site, &file, &sig_text(UPDATE_SIG), "9.9.9");
    let _ = run(
        &site.setup(),
        &["clean-staging", "--dir", &site.0.to_string_lossy()],
    );
    let after: Vec<_> = std::fs::read_dir(&pd)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    if !existed {
        let _ = std::fs::remove_dir(&pd);
    }
    assert_eq!(before, after, "nothing was written under ProgramData");
    let _ = std::fs::remove_dir_all(dir);
}

fn acl_identities(path: &Path) -> Vec<String> {
    let ps = format!(
        "(Get-Acl -LiteralPath '{}').Access | ForEach-Object {{ $_.IdentityReference.Value }}",
        path.display()
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .output()
        .unwrap();
    let mut v: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The success path: signed program, copied, verified on the held copy, started with the fixed arguments and the run
/// folder's `TEMP`; the staged copy and its folder are readable by Administrators and SYSTEM only.
#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn apply_update_starts_the_signed_installer_with_fixed_arguments_and_a_protected_temp() {
    let site = Site::with_exe("e2e", "PADDOCK_SETUP_EXE_E2E");
    let (installer, good, _other) = e2e_files();
    let (code, out) = apply(&site, &installer, &good, "9.9.9");
    assert_eq!(code, 0, "{out}\n{}", site.log());
    let runs: Vec<PathBuf> = std::fs::read_dir(site.updates())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    assert_eq!(runs.len(), 1, "one run folder: {runs:?}");
    let run_dir = &runs[0];
    let record = run_dir.join("tmp").join("fake-installer.txt");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    // A wait on the file the child writes, not a fixed sleep: the loop ends when it exists (bounded).
    while !record.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let text = std::fs::read_to_string(&record)
        .unwrap_or_else(|e| panic!("the installer did not run: {e}\n{}", site.log()));
    let tmp = run_dir.join("tmp").to_string_lossy().to_lowercase();
    let lower = text.to_lowercase();
    assert!(text.contains("args=/P /UPDATE /R\n"), "{text}");
    assert!(
        lower.contains(&format!("temp={tmp}\n")) && lower.contains(&format!("tmp={tmp}\n")),
        "{text}"
    );
    assert!(
        lower.contains(&format!(
            "exe={}\\update-9.9.9.exe",
            run_dir.to_string_lossy().to_lowercase()
        )),
        "{text}"
    );
    assert!(
        site.log().contains("handed off: the installer"),
        "{}",
        site.log()
    );
    // Users never read the staged copy or its folder.
    let staged = run_dir.join("update-9.9.9.exe");
    for path in [run_dir.as_path(), staged.as_path()] {
        let who = acl_identities(path);
        assert_eq!(
            who,
            ["BUILTIN\\Administrators", "NT AUTHORITY\\SYSTEM"],
            "{} {who:?}",
            path.display()
        );
    }
}

/// The installer's own `hold-installing` takes the lock from `apply-update`: no gap, no clock.
#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn hold_installing_logs_and_releases_when_its_parent_ends() {
    let site = Site::with_exe("hold", "PADDOCK_SETUP_EXE");
    assert_eq!(
        run(
            &site.setup(),
            &["check-dir", "--dir", &site.0.to_string_lossy()]
        )
        .0,
        0
    );
    // The holder's parent is a cmd that lives about three seconds.
    let line = format!(
        "start /b \"\" \"{}\" hold-installing --dir \"{}\" & ping -n 4 127.0.0.1 > nul",
        site.setup().display(),
        site.0.display()
    );
    assert!(
        Command::new("cmd")
            .args(["/c", &line])
            .output()
            .unwrap()
            .status
            .success()
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !site.log().contains("the installer ended; lock released")
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let log = site.log();
    for needle in [
        "phase=hold-installing the lock was made",
        "phase=hold-installing holding the lock until the installer ends",
        "phase=hold-installing no hand-over event: this install was started by hand",
        "phase=hold-installing the installer ended; lock released",
    ] {
        assert!(log.contains(needle), "{needle} not in\n{log}");
    }
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn clean_staging_keeps_what_is_in_use_and_removes_the_rest() {
    let site = Site::with_exe("staging-busy", "PADDOCK_SETUP_EXE");
    assert_eq!(
        run(
            &site.setup(),
            &["check-dir", "--dir", &site.0.to_string_lossy()]
        )
        .0,
        0
    );
    let busy = site.updates().join("run-1-a");
    let idle = site.updates().join("run-2-b");
    std::fs::create_dir_all(&busy).unwrap();
    std::fs::create_dir_all(&idle).unwrap();
    std::fs::write(idle.join("x"), b"x").unwrap();
    let ping = PathBuf::from(std::env::var("SystemRoot").unwrap())
        .join("System32")
        .join("PING.EXE");
    std::fs::copy(&ping, busy.join("installer.exe")).unwrap();
    let mut child = Command::new(busy.join("installer.exe"))
        .args(["-t", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let (code, _) = run(
        &site.setup(),
        &["clean-staging", "--dir", &site.0.to_string_lossy()],
    );
    let (busy_left, idle_left) = (busy.exists(), idle.exists());
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(
        (code, busy_left, idle_left),
        (0, true, false),
        "{}",
        site.log()
    );
    assert!(
        site.log().contains("kept run-1-a (in use)"),
        "{}",
        site.log()
    );
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn check_dir_accepts_an_8_3_short_name_and_prints_the_long_one() {
    let site = Site::new("short name");
    let rel = site.0.file_name().unwrap().to_string_lossy().into_owned();
    let short = format!(r"C:\PROGRA~1\{rel}");
    assert!(
        Path::new(&short).exists(),
        "8.3 names must be on for the system volume: {short}"
    );
    let (code, out) = run(&exe("PADDOCK_SETUP_EXE"), &["check-dir", "--dir", &short]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.to_lowercase()
            .contains(&site.0.to_string_lossy().to_lowercase()),
        "the canonical long name is reported: {out}"
    );
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn check_dir_refuses_a_folder_that_users_can_change() {
    let site = Site::new("users-modify");
    let grant = Command::new("icacls")
        .arg(&site.0)
        .args(["/grant", "*S-1-5-32-545:(OI)(CI)M"])
        .output()
        .unwrap();
    assert!(
        grant.status.success(),
        "{}",
        String::from_utf8_lossy(&grant.stdout)
    );
    let (code, out) = run(
        &exe("PADDOCK_SETUP_EXE"),
        &["check-dir", "--dir", &site.0.to_string_lossy()],
    );
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("not protected"), "{out}");
    assert!(!site.updates().exists(), "a refusal writes nothing");
}

#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn same_folder_logs_its_answer() {
    let site = Site::with_exe("samelog", "PADDOCK_SETUP_EXE");
    assert_eq!(
        run(
            &site.setup(),
            &["check-dir", "--dir", &site.0.to_string_lossy()]
        )
        .0,
        0
    );
    let d = site.0.to_string_lossy().to_string();
    assert_eq!(
        run(&site.setup(), &["same-folder", "--a", &d, "--b", &d]).0,
        0
    );
    assert!(
        site.log()
            .contains("phase=same-folder the installer runs from the install folder: a reinstall"),
        "{}",
        site.log()
    );
}

/// `stop-all` over the real stopper (a build with the `proc-stop` feature, `PADDOCK_SETUP_EXE_STOP`): a listed program that
/// runs from the folder is ended; a program with the same file name from another folder is not.
#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn stop_all_ends_a_listed_program_from_the_folder_and_leaves_a_namesake_alone() {
    let site = Site::with_exe("stopall", "PADDOCK_SETUP_EXE_STOP");
    let other = Site::new("stopall-other");
    let ping = PathBuf::from(std::env::var("SystemRoot").expect("SystemRoot"))
        .join("System32")
        .join("PING.EXE");
    let mine = site.0.join("paddocktarget.exe");
    let theirs = other.0.join("paddocktarget.exe");
    std::fs::copy(&ping, &mine).unwrap();
    std::fs::copy(&ping, &theirs).unwrap();
    // `ping -t` runs until it is ended; both children are made and ended by this test only.
    let mut child_mine = Command::new(&mine)
        .args(["-t", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("start the listed program");
    let mut child_theirs = Command::new(&theirs)
        .args(["-t", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("start the namesake");
    let (code, _) = run(
        &site.setup(),
        &[
            "stop-all",
            "--dir",
            &site.0.to_string_lossy(),
            "--images",
            "paddocktarget.exe",
        ],
    );
    let mine_ended = child_mine.try_wait().unwrap().is_some();
    let theirs_running = child_theirs.try_wait().unwrap().is_none();
    let _ = child_mine.kill();
    let _ = child_theirs.kill();
    let _ = child_mine.wait();
    let _ = child_theirs.wait();
    let log = site.log();
    assert_eq!((code, mine_ended, theirs_running), (0, true, true), "{log}");
    assert!(
        log.contains("phase=process image=paddocktarget.exe pid="),
        "{log}"
    );
    assert!(log.contains("phase=exit code=0"), "{log}");
}

/// `guard`: a per-user uninstall key in the installer account's hive blocks a fresh install (exit 2, the message on the
/// first stdout line); without the key, or when an HKLM key exists, it does not.
#[test]
#[ignore = "Windows test VM only: PADDOCK_SETUP_VM=1 (ER-14)"]
fn guard_blocks_on_the_installer_accounts_uninstall_key_only() {
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\PaddockSetupTest";
    let site = Site::new("guard");
    let dir = site.0.to_string_lossy().into_owned();
    let args = [
        "guard",
        "--product",
        "PaddockSetupTest",
        "--main",
        "paddock-setup.exe",
        "--dir",
        dir.as_str(),
    ];
    let p = exe("PADDOCK_SETUP_EXE");
    assert_eq!(run(&p, &args).0, 0, "no key: nothing blocks");
    assert!(reg(&[
        "add",
        KEY,
        "/v",
        "UninstallString",
        "/t",
        "REG_SZ",
        "/d",
        r#""C:\Nowhere\PaddockSetupTest\uninstall.exe""#,
        "/f"
    ]));
    let (code, out) = run(&p, &args);
    let mut with_restart = args.to_vec();
    with_restart.extend(["--update", "--restart"]);
    let (code2, out2) = run(&p, &with_restart);
    reg(&["delete", KEY, "/f"]);
    assert_eq!(code, 2, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        [
            "Uninstall the older PaddockSetupTest first (Settings > Apps), then run this installer again."
        ]
    );
    assert_eq!(code2, 2);
    assert!(
        out2.lines()
            .next()
            .unwrap()
            .ends_with("Then start PaddockSetupTest again."),
        "{out2}"
    );
    assert!(
        !out2.contains("RESTART:"),
        "the program is not there, so nothing is started"
    );
    assert!(
        site.log()
            .contains("phase=guard hit role=installer key=yes file=no (blocks)"),
        "{}",
        site.log()
    );
    assert_eq!(run(&p, &args).0, 0, "the key is gone again");
}
