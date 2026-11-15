//! `verify-bundle`: the bundle checks of `scripts/macos-verify.sh`, read-only on the system.

use paddock_proc::{command, run_bounded, Limits};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The launchd label of the bundled agent.
pub const LABEL: &str = "net.decryptology.pitwall.service";
const TOOL_LIMIT: Duration = Duration::from_secs(60);

/// The checklist the script printed: `ok`, `fail` (a problem) and `..` (a note) lines.
#[derive(Default)]
pub struct Lines {
    pub text: String,
    pub problems: usize,
}

impl Lines {
    pub fn ok(&mut self, s: &str) {
        let _ = writeln!(self.text, "  ok    {s}");
    }
    pub fn bad(&mut self, what: &str, next: &str) {
        let _ = writeln!(self.text, "  fail  {what}\n        {next}");
        self.problems += 1;
    }
    pub fn note(&mut self, s: &str) {
        let _ = writeln!(self.text, "  ..    {s}");
    }
}

/// What `plutil -extract <key> raw` printed for a key, trimmed; `None` when the tool failed.
pub fn raw_value(output: Option<&str>) -> Option<String> {
    output.map(|s| s.trim().to_string())
}

/// The plist checks given the values `plutil -extract` returned: the program the agent runs and KeepAlive.
pub fn check_plist(l: &mut Lines, lint_ok: bool, bundle_program: Option<&str>, keep_alive: Option<&str>) {
    if lint_ok {
        l.ok("plist is valid");
    } else {
        l.bad("plist", "plutil -lint failed");
    }
    if bundle_program == Some("Contents/MacOS/pitwall-server") {
        l.ok("BundleProgram points at the bundled server");
    } else {
        l.bad("BundleProgram", "must be Contents/MacOS/pitwall-server");
    }
    if keep_alive == Some("true") {
        l.ok("KeepAlive restarts the server");
    } else {
        l.bad("KeepAlive", "must be true");
    }
}

/// The `Signature=` value of `codesign -dv` output (it prints on standard error).
pub fn signature_kind(codesign_dv: &str) -> Option<String> {
    codesign_dv
        .lines()
        .find_map(|l| l.strip_prefix("Signature="))
        .map(|s| s.trim().to_string())
}

/// True when `otool -l` output has a load command `LC_RPATH`.
pub fn has_rpath(otool_l: &str) -> bool {
    otool_l.contains("LC_RPATH")
}

fn tool(program: &str, args: &[&str]) -> Option<(bool, String)> {
    let mut cmd = command(program);
    cmd.args(args);
    let out = run_bounded(cmd, Limits::new(TOOL_LIMIT)).ok()?;
    Some((out.success(), format!("{}{}", out.stdout_text(), out.stderr_text())))
}

fn executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// The bundle checks; `Err` only for a bundle that does not exist (the script stopped there).
pub fn verify(app: &Path) -> Result<Lines, Lines> {
    let mut l = Lines::default();
    let _ = writeln!(l.text, "Pitwall bundle: {}", app.display());
    if app.is_dir() {
        l.ok("app bundle");
    } else {
        l.bad(
            "app bundle",
            &format!(
                "not found at {} (build with cargo tauri build, or install from the .dmg)",
                app.display()
            ),
        );
        return Err(l);
    }
    let server = app.join("Contents/MacOS/pitwall-server");
    let main = app.join("Contents/MacOS/pitwall-app");
    let plist = app.join(format!("Contents/Library/LaunchAgents/{LABEL}.plist"));
    if executable(&main) {
        l.ok("app binary");
    } else {
        l.bad("app binary", &format!("{} is missing", main.display()));
    }
    if executable(&server) && std::fs::metadata(&server).is_ok_and(|m| m.len() > 0) {
        l.ok("server binary in the bundle");
    } else {
        l.bad(
            "server binary",
            &format!("{} is missing or empty; rebuild (scripts/sidecar.mjs)", server.display()),
        );
    }
    if plist.is_file() {
        l.ok("launch agent plist in the bundle");
        let p = plist.to_string_lossy().into_owned();
        let lint = tool("plutil", &["-lint", &p]).is_some_and(|(ok, _)| ok);
        let get = |key: &str| tool("plutil", &["-extract", key, "raw", &p]).filter(|(ok, _)| *ok).map(|(_, o)| o);
        check_plist(
            &mut l,
            lint,
            raw_value(get("BundleProgram").as_deref()).as_deref(),
            raw_value(get("KeepAlive").as_deref()).as_deref(),
        );
    } else {
        l.bad(
            "launch agent plist",
            &format!("Contents/Library/LaunchAgents/{LABEL}.plist is missing; rebuild"),
        );
    }
    // SMAppService refuses a bundle that is only linker-signed (-67056): it needs a sealed (ad-hoc or Developer ID) signature.
    let a = app.to_string_lossy().into_owned();
    if tool("codesign", &["--verify", "--deep", "--strict", &a]).is_some_and(|(ok, _)| ok) {
        let kind = tool("codesign", &["-dv", &a]).and_then(|(_, o)| signature_kind(&o));
        l.ok(&format!("code signature is sealed ({})", kind.as_deref().unwrap_or("valid")));
    } else {
        l.bad(
            "code signature",
            "codesign --verify failed; build with signingIdentity \"-\" (docs/macos.md)",
        );
    }
    if executable(&main) {
        if let Some((_, o)) = tool("otool", &["-l", &main.to_string_lossy()]) {
            if has_rpath(&o) {
                l.ok("rpath present on the app binary");
            } else {
                l.note("no rpath on the app binary (fine unless native libs are bundled)");
            }
        }
    }
    let ver = tool(
        "plutil",
        &[
            "-extract",
            "LSMinimumSystemVersion",
            "raw",
            &app.join("Contents/Info.plist").to_string_lossy(),
        ],
    )
    .filter(|(ok, _)| *ok)
    .map(|(_, o)| o.trim().to_string())
    .filter(|v| !v.is_empty());
    match ver {
        Some(v) => l.ok(&format!("minimum macOS {v} (SMAppService needs 13)")),
        None => l.note("no LSMinimumSystemVersion"),
    }
    Ok(l)
}

/// `cargo xtask verify-bundle [Pitwall.app] [--smoke]`.
pub fn run_task(args: &[String]) -> Result<(), String> {
    let mut app: Option<PathBuf> = None;
    let mut smoke = false;
    for a in args {
        match a.as_str() {
            "--smoke" => smoke = true,
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => app = Some(PathBuf::from(other)),
        }
    }
    let app = app.unwrap_or_else(|| PathBuf::from("/Applications/Pitwall.app"));
    let mut l = match verify(&app) {
        Ok(l) => l,
        Err(l) => {
            return Err(format!(
                "{}\n{} problem(s) found; docs/macos.md explains each step.",
                l.text, l.problems
            ))
        }
    };
    if smoke {
        let server = app.join("Contents/MacOS/pitwall-server");
        let main = app.join("Contents/MacOS/pitwall-app");
        if executable(&server) && executable(&main) {
            crate::smoke::run(&server, &main, &mut l);
        }
    }
    print!("{}", l.text);
    if l.problems == 0 {
        println!("\nEverything checked looks right.");
        Ok(())
    } else {
        Err(format!("\n{} problem(s) found; docs/macos.md explains each step.", l.problems))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_good_plist_gives_three_ok_lines_and_each_wrong_value_one_problem() {
        let mut l = Lines::default();
        check_plist(&mut l, true, Some("Contents/MacOS/pitwall-server"), Some("true"));
        assert_eq!(l.problems, 0);
        assert!(
            l.text.contains("ok    BundleProgram points at the bundled server") && l.text.contains("ok    KeepAlive restarts the server")
        );
        let mut bad = Lines::default();
        check_plist(&mut bad, false, Some("Contents/MacOS/other"), Some("false"));
        assert_eq!(bad.problems, 3);
        assert!(
            bad.text.contains("fail  plist\n        plutil -lint failed") && bad.text.contains("must be Contents/MacOS/pitwall-server")
        );
        let mut missing = Lines::default();
        check_plist(&mut missing, true, None, None);
        assert_eq!(missing.problems, 2);
    }

    #[test]
    fn the_signature_kind_and_the_rpath_are_read_from_the_tools_output() {
        assert_eq!(
            signature_kind("Identifier=x\nSignature=adhoc\nTeamIdentifier=not set\n").as_deref(),
            Some("adhoc")
        );
        assert_eq!(signature_kind("code object is not signed at all"), None);
        assert!(has_rpath("Load command 9\n      cmd LC_RPATH\n") && !has_rpath("cmd LC_MAIN"));
    }

    #[test]
    fn a_missing_bundle_ends_the_check_with_one_problem() {
        let d = tempfile::tempdir().unwrap();
        let missing = d.path().join("no-such-pitwall-bundle.app");
        let l = verify(&missing).err().unwrap();
        assert_eq!(l.problems, 1);
        assert!(l.text.contains("fail  app bundle") && l.text.contains("not found at"));
    }

    #[test]
    fn an_empty_bundle_lists_each_missing_piece() {
        let d = tempfile::tempdir().unwrap();
        let app = d.path().join("Pitwall.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        let l = verify(&app).ok().unwrap();
        assert!(
            l.text.contains("fail  app binary") && l.text.contains("fail  server binary") && l.text.contains("fail  launch agent plist"),
            "{}",
            l.text
        );
        assert!(l.problems >= 4, "{}", l.text);
    }
}
