//! `dev ci host-tools`: the pinned `cargo-nextest` on every OS (the port of the `host-tools` action). A host runner keeps the
//! checksum-verified release in the CI home's shared tools folder (`tools/cargo-nextest/<version>`, job-isolation) and downloads
//! it only when that version is missing; the Linux image brings its own. Every runner then checks that
//! `cargo nextest --version` is the pin.
//!
//! The pin is the version the paddock-ci image has (`ci-image/Dockerfile`, via the garage image): bump both together with the SHA-256
//! values of the release archives ([`PIN`]; a conformance test checks the shape). The download is `curl` through
//! [`ToolCall`](crate::ci_tool::ToolCall) with a bounded run; the checksum is read here (`sha2`), not by `sha256sum`/`shasum`.
//! Two jobs installing one version at once each rename a whole copy into place (a part file of their own), so no reader sees half
//! a program.

use super::safe_remove;
use super::{Step, is_executable, on_path};
use crate::ci_isolation::home;
use crate::ci_tool::{NETWORK_ENV, ToolCall};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One release archive of the pinned version.
#[derive(Debug, Clone)]
pub struct Asset {
    /// The archive's name suffix (`universal-apple-darwin.tar.gz`).
    pub archive: &'static str,
    /// Its SHA-256.
    pub sum: &'static str,
    /// The program inside.
    pub bin: &'static str,
}

/// The pin: the version and the archives of the host runner classes.
#[derive(Debug, Clone)]
pub struct Pin {
    /// The release version.
    pub version: &'static str,
    /// macOS, any architecture.
    pub macos: Asset,
    /// Windows on x86_64.
    pub windows: Asset,
    /// Where the releases are (`<base>/cargo-nextest-<version>/cargo-nextest-<version>-<archive>`).
    pub base_url: &'static str,
}

/// The version the Linux image has (`ci-image/Dockerfile`), with the SHA-256 values of the two host archives.
pub const PIN: Pin = Pin {
    version: "0.9.149",
    macos: Asset {
        archive: "universal-apple-darwin.tar.gz",
        sum: "47b9941765a9ff94e304a3fe5444ac1c7593da655525b8b69f8f2b31a9217507",
        bin: "cargo-nextest",
    },
    windows: Asset {
        archive: "x86_64-pc-windows-msvc.zip",
        sum: "bf904b85c7c5c2a29c340053a8d9d8e8027ff0d55a2bf7cef7d23032f8166438",
        bin: "cargo-nextest.exe",
    },
    base_url: "https://github.com/nextest-rs/nextest/releases/download",
};

/// How long the download, the extraction and the version probe may each run.
const TOOL_TIMEOUT: Duration = Duration::from_secs(300);

/// The command over the real machine.
pub fn run(env: &HashMap<String, String>, windows: bool, out: &mut dyn Write) -> i32 {
    run_with(env, windows, std::env::consts::ARCH, &PIN, out)
}

/// The command over an explicit architecture and pin (tests).
pub fn run_with(env: &HashMap<String, String>, windows: bool, arch: &str, pin: &Pin, out: &mut dyn Write) -> i32 {
    let mut step = Step {
        env,
        out,
        windows,
        name: "host-tools",
    };
    let Some(os) = step.var("RUNNER_OS").map(str::to_string) else {
        return fail_plain(&mut step, "RUNNER_OS is not set.", "run this step in a runner job.");
    };
    let mut env = env.clone();
    if os != "Linux" {
        let asset = match (os.as_str(), arch) {
            ("macOS", _) => &pin.macos,
            ("Windows", "x86_64") => &pin.windows,
            _ => {
                return fail_plain(
                    &mut step,
                    &format!("no pinned cargo-nextest for {os} {arch}"),
                    "add the platform to the pin in tools/paddock-dev/src/ci_host/host_tools.rs.",
                );
            }
        };
        let tools = match step.isolation(&["path", "tools"]) {
            Ok(p) => p,
            Err(e) => {
                step.say(e);
                return 1;
            }
        };
        let dest = Path::new(&tools).join("cargo-nextest").join(pin.version);
        if is_executable(&dest.join(asset.bin)) {
            step.say(format!("cargo-nextest {} kept in {}", pin.version, dest.display()));
        } else if let Err(e) = install(&mut step, pin, asset, &dest, Path::new(&tools)) {
            return e;
        }
        let sep = if windows { ';' } else { ':' };
        let path = env.get("PATH").cloned().unwrap_or_default();
        env.insert("PATH".into(), format!("{}{sep}{path}", dest.display()));
        let shown = if windows {
            home::native(&dest.display().to_string())
        } else {
            dest.display().to_string()
        };
        if let Err(e) = step.append_to("GITHUB_PATH", &shown) {
            return fail_plain(&mut step, &e, "run this step in a runner job.");
        }
    }
    let got = nextest_version(&env);
    if got != pin.version {
        let shown = if got.is_empty() { "missing" } else { &got };
        return fail_plain(
            &mut step,
            &format!("cargo-nextest is {shown} on this runner, the pin is {}", pin.version),
            "use the paddock-ci image (it has the pin) or bump the pin in .gitea/actions/host-tools and ci-image/Dockerfile together.",
        );
    }
    step.say(format!("cargo-nextest {got}"));
    0
}

/// `host-tools` messages carry no step prefix (the action printed `cargo-nextest ...` lines): one `::error::` line.
fn fail_plain(step: &mut Step<'_>, cause: &str, next: &str) -> i32 {
    step.say(crate::ci_tool::error_line(cause, next));
    1
}

/// The version `cargo nextest --version` reports (second word of its first line), empty when it cannot run.
fn nextest_version(env: &HashMap<String, String>) -> String {
    let call = ToolCall::new("cargo-nextest").args(["nextest", "--version"]).timeout(TOOL_TIMEOUT);
    match call.run(env, &mut std::io::sink()) {
        Ok(r) if r.success => r
            .stdout
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

/// Downloads, verifies, unpacks and renames the program into `dest`.
fn install(step: &mut Step<'_>, pin: &Pin, asset: &Asset, dest: &Path, tools: &Path) -> Result<(), i32> {
    let archive = format!("cargo-nextest-{}-{}", pin.version, asset.archive);
    let url = format!("{}/cargo-nextest-{}/{archive}", pin.base_url, pin.version);
    // A work folder of this job inside the shared tools folder (never the user's temp folder, ER-14).
    let work = tools.join(format!(".download-{}", std::process::id()));
    let home_dir = step.var("HOME").or_else(|| step.var("USERPROFILE")).map(PathBuf::from);
    let result = install_in(step, pin, asset, dest, &work, &archive, &url);
    if let Err(e) = safe_remove::remove_under(tools, &work, home_dir.as_deref()) {
        step.say(format!("::warning::host-tools: could not remove {}: {e}", work.display()));
    }
    result
}

fn install_in(step: &mut Step<'_>, pin: &Pin, asset: &Asset, dest: &Path, work: &Path, archive: &str, url: &str) -> Result<(), i32> {
    let fail = |step: &mut Step<'_>, cause: String, next: &str| Err(fail_plain(step, &cause, next));
    let unpacked = work.join("x");
    if let Err(e) = std::fs::create_dir_all(&unpacked) {
        return fail(
            step,
            format!("cannot create {}: {e}", unpacked.display()),
            "check the CI home's permissions and free space.",
        );
    }
    let file = work.join(archive);
    let mut log = Vec::new();
    let curl = ToolCall::new("curl")
        .args(["-fsSL", "--retry", "3", "-o", &file.display().to_string(), url])
        .pass(NETWORK_ENV.iter().copied())
        .timeout(TOOL_TIMEOUT);
    let ran = curl.run(step.env, &mut log);
    step.out.write_all(&log).ok();
    match ran {
        Ok(r) if r.success => {}
        Ok(r) => {
            return fail(
                step,
                format!("cargo-nextest {} download failed ({}): {}", pin.version, r.how, r.stderr.trim()),
                "check the runner's network access to github.com, then re-run the job.",
            );
        }
        Err(e) => {
            return fail(
                step,
                format!("cargo-nextest {} download failed: {e}", pin.version),
                "install curl on the runner, then re-run the job.",
            );
        }
    }
    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(e) => return fail(step, format!("cannot read the download {}: {e}", file.display()), "re-run the job."),
    };
    let got = hex(&Sha256::digest(&bytes));
    if got != asset.sum {
        return fail(
            step,
            format!("cargo-nextest {} checksum mismatch for {} (got {got})", pin.version, asset.archive),
            "the download is not the pinned release: check the network path, or the pin's sums in host_tools.rs.",
        );
    }
    let extract = if asset.archive.ends_with(".zip") {
        unzip_call(step.env, &file, &unpacked)
    } else {
        tar_call(&file, &unpacked)
    };
    let mut log = Vec::new();
    let ran = extract.run(step.env, &mut log);
    step.out.write_all(&log).ok();
    match ran {
        Ok(r) if r.success => {}
        Ok(r) => {
            return fail(
                step,
                format!("cannot unpack {archive} ({}): {}", r.how, r.stderr.trim()),
                "re-run the job; if it repeats, delete the CI home's tools/cargo-nextest folder.",
            );
        }
        Err(e) => {
            return fail(
                step,
                format!("cannot unpack {archive}: {e}"),
                "install tar (or PowerShell on Windows) on the runner.",
            );
        }
    }
    let part = dest.join(format!("{}.part.{}", asset.bin, std::process::id()));
    let target = dest.join(asset.bin);
    let moved = std::fs::create_dir_all(dest)
        .and_then(|()| std::fs::copy(unpacked.join(asset.bin), &part).map(|_| ()))
        .and_then(|()| make_executable(&part))
        .and_then(|()| std::fs::rename(&part, &target));
    if let Err(e) = moved {
        return fail(
            step,
            format!("cannot install {} into {}: {e}", asset.bin, dest.display()),
            "check the CI home's permissions and free space.",
        );
    }
    step.say(format!("cargo-nextest {} installed in {}", pin.version, dest.display()));
    Ok(())
}

fn tar_call(file: &Path, into: &Path) -> ToolCall {
    ToolCall::new("tar")
        .args(["-xzf", &file.display().to_string(), "-C", &into.display().to_string()])
        .timeout(TOOL_TIMEOUT)
}

/// `unzip` where it exists, else PowerShell's `Expand-Archive`; the paths go as environment variables, never into the script.
fn unzip_call(env: &HashMap<String, String>, file: &Path, into: &Path) -> ToolCall {
    if on_path(env, "unzip") {
        return ToolCall::new("unzip")
            .args(["-q", &file.display().to_string(), "-d", &into.display().to_string()])
            .timeout(TOOL_TIMEOUT);
    }
    ToolCall::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Expand-Archive -Path $env:PADDOCK_ZIP -DestinationPath $env:PADDOCK_OUT",
        ])
        .env("PADDOCK_ZIP", home::native(&file.display().to_string()))
        .env("PADDOCK_OUT", home::native(&into.display().to_string()))
        .timeout(TOOL_TIMEOUT)
}

fn make_executable(p: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    struct Scene {
        d: tempfile::TempDir,
        env: HashMap<String, String>,
    }

    fn scene(os: &str) -> Scene {
        let d = crate::ci_host::canonical_tempdir();
        std::fs::create_dir_all(d.path().join("home")).unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let env = [
            ("RUNNER_OS", os.to_string()),
            ("PADDOCK_CI_SLOT", "s1".into()),
            ("HOME", d.path().join("home").display().to_string()),
            ("PATH", format!("{}:/usr/bin:/bin", bin.display())),
            ("GITHUB_PATH", d.path().join("github-path").display().to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        Scene { d, env }
    }

    fn go(sc: &Scene, pin: &Pin) -> (i32, String) {
        let mut out = Vec::new();
        let code = run_with(&sc.env, false, "aarch64", pin, &mut out);
        (code, String::from_utf8(out).unwrap())
    }

    fn kept_dir(sc: &Scene) -> PathBuf {
        sc.d.path().join("home/.paddock-ci/tools/cargo-nextest").join(PIN.version)
    }

    #[test]
    fn a_kept_pinned_program_is_used_without_a_download_and_put_on_the_path() {
        let sc = scene("macOS");
        script(
            &kept_dir(&sc).join("cargo-nextest"),
            &format!("echo 'cargo-nextest {} (abc)'", PIN.version),
        );
        script(&sc.d.path().join("bin/curl"), "echo curl was called >&2; exit 7");
        let (code, out) = go(&sc, &PIN);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains(&format!("cargo-nextest {} kept in", PIN.version)), "{out}");
        assert!(out.ends_with(&format!("cargo-nextest {}\n", PIN.version)), "{out}");
        assert_eq!(
            std::fs::read_to_string(sc.d.path().join("github-path")).unwrap(),
            format!("{}\n", kept_dir(&sc).display())
        );
    }

    #[test]
    fn a_kept_program_of_another_version_fails_the_check_naming_both() {
        let sc = scene("macOS");
        script(&kept_dir(&sc).join("cargo-nextest"), "echo 'cargo-nextest 0.9.1 (abc)'");
        let (code, out) = go(&sc, &PIN);
        assert_eq!(code, 1);
        assert!(
            out.contains(&format!(
                "::error::cargo-nextest is 0.9.1 on this runner, the pin is {}",
                PIN.version
            )),
            "{out}"
        );
        assert!(out.contains("Next step:"), "{out}");
    }

    #[test]
    fn linux_checks_the_image_copy_only_and_a_missing_one_is_an_error() {
        let sc = scene("Linux");
        script(
            &sc.d.path().join("bin/cargo-nextest"),
            &format!("echo 'cargo-nextest {} (x)'", PIN.version),
        );
        let (code, out) = go(&sc, &PIN);
        assert_eq!((code, out.as_str()), (0, format!("cargo-nextest {}\n", PIN.version).as_str()));
        assert!(!sc.d.path().join("github-path").exists());
        std::fs::remove_file(sc.d.path().join("bin/cargo-nextest")).unwrap();
        let (code, out) = go(&sc, &PIN);
        assert_eq!(code, 1);
        assert!(out.contains("cargo-nextest is missing on this runner"), "{out}");
    }

    /// A pin whose archive is built here: a tar.gz with a shim program, and its real SHA-256.
    fn fixture_pin(sc: &Scene, version: &'static str, lie: bool) -> Pin {
        let pack = sc.d.path().join("pack");
        script(&pack.join("cargo-nextest"), &format!("echo 'cargo-nextest {version} (fixture)'"));
        let tgz = sc.d.path().join("fixture.tar.gz");
        let ok = paddock_proc::command("tar")
            .args([
                "-czf",
                &tgz.display().to_string(),
                "-C",
                &pack.display().to_string(),
                "cargo-nextest",
            ])
            .status()
            .unwrap();
        assert!(ok.success());
        let sum: &'static str = if lie {
            "0".repeat(64).leak()
        } else {
            hex(&Sha256::digest(std::fs::read(&tgz).unwrap())).leak()
        };
        // curl: copies the fixture to the -o path and records the URL.
        script(
            &sc.d.path().join("bin/curl"),
            &format!(
                "out=''; url=''; while [ $# -gt 0 ]; do case \"$1\" in -o) out=\"$2\"; shift;; -*) ;; *) url=\"$1\";; esac; shift; done; echo \"$url\" > '{}'; cp '{}' \"$out\"",
                sc.d.path().join("url").display(),
                tgz.display()
            ),
        );
        Pin {
            version,
            macos: Asset {
                archive: "universal-apple-darwin.tar.gz",
                sum,
                bin: "cargo-nextest",
            },
            windows: PIN.windows.clone(),
            base_url: "https://example.invalid/releases",
        }
    }

    #[test]
    fn a_missing_version_is_downloaded_verified_unpacked_and_renamed_into_place() {
        let sc = scene("macOS");
        let pin = fixture_pin(&sc, "9.9.9", false);
        let (code, out) = go(&sc, &pin);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("cargo-nextest 9.9.9 installed in"), "{out}");
        assert_eq!(
            std::fs::read_to_string(sc.d.path().join("url")).unwrap().trim(),
            "https://example.invalid/releases/cargo-nextest-9.9.9/cargo-nextest-9.9.9-universal-apple-darwin.tar.gz"
        );
        let dest = sc.d.path().join("home/.paddock-ci/tools/cargo-nextest/9.9.9");
        assert!(is_executable(&dest.join("cargo-nextest")));
        let parts: Vec<_> = std::fs::read_dir(&dest)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(parts, ["cargo-nextest"], "no part file is left");
        let tools = sc.d.path().join("home/.paddock-ci/tools");
        assert!(
            !std::fs::read_dir(&tools)
                .unwrap()
                .any(|e| e.unwrap().file_name().to_string_lossy().starts_with(".download")),
            "the work folder is removed"
        );
        // The second run finds it kept and does not call curl again.
        script(&sc.d.path().join("bin/curl"), "echo curl was called >&2; exit 7");
        let (code, out) = go(&sc, &pin);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("kept in"), "{out}");
    }

    #[test]
    fn a_download_with_the_wrong_checksum_is_refused_and_nothing_is_installed() {
        let sc = scene("macOS");
        let pin = fixture_pin(&sc, "9.9.8", true);
        let (code, out) = go(&sc, &pin);
        assert_eq!(code, 1);
        assert!(
            out.contains("::error::cargo-nextest 9.9.8 checksum mismatch for universal-apple-darwin.tar.gz (got "),
            "{out}"
        );
        assert!(!sc.d.path().join("home/.paddock-ci/tools/cargo-nextest/9.9.8").exists());
    }

    #[test]
    fn a_failed_download_is_one_error_line_with_its_next_step() {
        let sc = scene("macOS");
        script(&sc.d.path().join("bin/curl"), "echo 'curl: (6) could not resolve host' >&2; exit 6");
        let (code, out) = go(&sc, &PIN);
        assert_eq!(code, 1);
        let errors: Vec<&str> = out.lines().filter(|l| l.starts_with("::error::")).collect();
        assert_eq!(errors.len(), 1, "{out}");
        assert!(
            errors[0].contains("download failed (exit 6): curl: (6) could not resolve host"),
            "{}",
            errors[0]
        );
        assert!(out.contains("run: curl -fsSL --retry 3 -o"), "the exact command is logged: {out}");
    }

    #[test]
    fn an_unpinned_platform_is_an_error() {
        let sc = scene("Windows");
        let mut out = Vec::new();
        let code = run_with(&sc.env, false, "aarch64", &PIN, &mut out);
        assert_eq!(code, 1);
        assert!(
            String::from_utf8(out)
                .unwrap()
                .contains("::error::no pinned cargo-nextest for Windows aarch64")
        );
    }

    #[test]
    fn the_pin_is_a_release_version_with_sha256_sums() {
        assert!(regex::Regex::new(r"^\d+\.\d+\.\d+$").unwrap().is_match(PIN.version));
        for a in [&PIN.macos, &PIN.windows] {
            assert!(a.sum.len() == 64 && a.sum.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        }
    }

    #[test]
    fn the_pin_matches_the_image_and_the_shell_copy() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let action = std::fs::read_to_string(repo.join("conformance/ci/host/host-tools.sh")).unwrap();
        assert!(
            action.contains(&format!("VERSION={}", PIN.version)),
            "bump the shell copy with the pin"
        );
        for a in [&PIN.macos, &PIN.windows] {
            assert!(action.contains(a.sum), "the shell copy has the sum of {}", a.archive);
        }
    }
}
