//! `dev ci node-caches`: npm's download cache and the Angular build cache kept between runs (the port of the `node-caches`
//! action). On a host runner they live in the CI home's kept folders (job-isolation: `npm` shared per slot, `angular/<name>-<slot>`
//! per job slot); in a Linux container on the runner's `/sccache` mount (`npm/` and `angular/<name>`), and a container without the
//! mount keeps both in the job (a `::notice::` says so).
//!
//! `npm_config_cache` goes to `$GITHUB_ENV` (a native path on Windows). The Angular cache folder of the workspace
//! (`<workspace>/<angular-dir>/.angular/cache`, turned on in CI by `angular.json`'s `cli.cache.environment: all`) becomes a link
//! (a junction on Windows) to the kept one, so two builds never write one cache at once. The workspace is `PADDOCK_WORKSPACE` (the
//! kept clone of a host job), else `GITHUB_WORKSPACE`.

use super::links::{Links, OsLinks};
use super::safe_remove;
use super::{Step, folder_name};
use crate::ci_isolation::home;
use clap::Args as ClapArgs;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// `dev ci node-caches`.
#[derive(ClapArgs, Debug)]
pub struct NodeCachesArgs {
    /// The Angular workspace whose `.angular/cache` is kept (relative to the checkout); empty keeps only the npm cache.
    #[arg(long, default_value = "")]
    pub angular_dir: String,
    /// The Angular cache's folder: one per job (and per matrix shard); on a host the job's slot is appended.
    #[arg(long)]
    pub name: String,
}

/// The short name the CLI uses.
pub type Args = NodeCachesArgs;

/// The command over the real machine.
pub fn run(env: &HashMap<String, String>, windows: bool, args: &Args, out: &mut dyn Write) -> i32 {
    run_with(env, windows, args, &OsLinks { env, windows }, out)
}

/// The command over an explicit link maker (tests).
pub fn run_with(env: &HashMap<String, String>, windows: bool, args: &Args, links: &dyn Links, out: &mut dyn Write) -> i32 {
    let mut step = Step {
        env,
        out,
        windows,
        name: "node-caches",
    };
    let Some(os) = step.var("RUNNER_OS").map(str::to_string) else {
        return step.fail("RUNNER_OS is not set.", "run this step in a runner job.");
    };
    let folder = folder_name(&args.name);
    let want_angular = !args.angular_dir.is_empty();
    let (npm_cache, ng_cache): (String, String);
    if os == "Linux" {
        let mount = PathBuf::from(step.var("PADDOCK_CI_MOUNT").unwrap_or("/sccache"));
        let npm = mount.join("npm");
        if !mount.is_dir() || std::fs::create_dir_all(&npm).is_err() {
            step.say("::notice::no /sccache mount in this container; the npm and Angular caches live and die with the job.");
            return 0;
        }
        npm_cache = npm.display().to_string();
        ng_cache = mount.join("angular").join(&folder).display().to_string();
    } else {
        // The host's kept folders (job-isolation): npm's cache and the Angular cache are the job slot's own.
        npm_cache = match step.isolation(&["path", "npm"]) {
            Ok(p) => p,
            Err(e) => return isolation_failed(&mut step, &e),
        };
        ng_cache = if want_angular {
            match step.isolation(&["path", "angular", &folder]) {
                Ok(p) => p,
                Err(e) => return isolation_failed(&mut step, &e),
            }
        } else {
            String::new()
        };
    }
    let shown = if windows { home::native(&npm_cache) } else { npm_cache.clone() };
    if let Err(e) = step.append_to("GITHUB_ENV", &format!("npm_config_cache={shown}")) {
        return step.fail(e, "run this step in a runner job.");
    }
    step.say(format!("node-caches: npm {npm_cache}"));
    if !want_angular {
        return 0;
    }
    // The tree of a host job is the kept clone (PADDOCK_WORKSPACE); on Windows the work folder is only the bootstrap checkout.
    let Some(ws) = step
        .var("PADDOCK_WORKSPACE")
        .or_else(|| step.var("GITHUB_WORKSPACE"))
        .map(PathBuf::from)
    else {
        return step.fail(
            "neither PADDOCK_WORKSPACE nor GITHUB_WORKSPACE is set.",
            "run this step in a runner job.",
        );
    };
    let angular = ws.join(&args.angular_dir).join(".angular");
    if let Err(e) = std::fs::create_dir_all(&ng_cache).and_then(|()| std::fs::create_dir_all(&angular)) {
        return step.fail(
            format!("cannot create the Angular cache folders: {e}."),
            "check the CI home's permissions and free space.",
        );
    }
    // angular.json turns the cache on in CI too; its folder is a link to the kept one.
    let link = angular.join("cache");
    let home_dir = step.var("HOME").or_else(|| step.var("USERPROFILE")).map(PathBuf::from);
    if let Err(e) = safe_remove::remove_under(&angular, &link, home_dir.as_deref()) {
        return step.fail(
            format!("cannot clear {}: {e}.", link.display()),
            "remove that folder by hand, then re-run the job.",
        );
    }
    if let Err(e) = links.link_dir(Path::new(&ng_cache), &link) {
        return step.fail(
            format!("cannot link {} to {ng_cache}: {e}.", link.display()),
            "check that the runner may create links there.",
        );
    }
    step.say(format!("node-caches: Angular {} -> {ng_cache}", link.display()));
    0
}

/// The isolation's own message is the cause; it already carries its next step.
fn isolation_failed(step: &mut Step<'_>, text: &str) -> i32 {
    step.say(text);
    1
}

#[cfg(test)]
mod tests {
    use super::super::links::fake::RecordingLinks;
    use super::*;

    fn env_in(dir: &Path, os: &str) -> HashMap<String, String> {
        [
            ("RUNNER_OS", os.to_string()),
            ("PADDOCK_CI_SLOT", "s1".into()),
            ("HOME", dir.join("home").display().to_string()),
            ("GITHUB_WORKSPACE", dir.join("ws").display().to_string()),
            ("GITHUB_ENV", dir.join("github-env").display().to_string()),
            ("PADDOCK_CI_MOUNT", dir.join("no-mount").display().to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
    }

    fn go(env: &HashMap<String, String>, win: bool, angular: &str, links: &dyn Links) -> (i32, String) {
        let mut out = Vec::new();
        let args = Args {
            angular_dir: angular.into(),
            name: "e2e/1".into(),
        };
        let code = run_with(env, win, &args, links, &mut out);
        (code, String::from_utf8(out).unwrap())
    }

    /// `rel` (written with `/`) below `base`, spelled as the code under test joins it: component by component, so the separators
    /// are the OS's own (`format!("{}/..", base.display())` mixes `\` and `/` on Windows).
    fn under(base: &Path, rel: &str) -> String {
        rel.split('/')
            .fold(base.to_path_buf(), |p, part| p.join(part))
            .display()
            .to_string()
    }

    fn setup() -> (tempfile::TempDir, HashMap<String, String>) {
        let d = crate::ci_host::canonical_tempdir();
        std::fs::create_dir_all(d.path().join("home")).unwrap();
        std::fs::create_dir_all(d.path().join("ws/ui/.angular/cache")).unwrap();
        std::fs::write(d.path().join("ws/ui/.angular/cache/stale"), "").unwrap();
        let env = env_in(d.path(), "macOS");
        (d, env)
    }

    #[test]
    fn a_host_keeps_npm_and_the_angular_cache_in_its_slots_folders_and_links_the_workspace_cache() {
        let (d, env) = setup();
        let links = RecordingLinks::default();
        let (code, out) = go(&env, false, "ui", &links);
        assert_eq!(code, 0, "{out}");
        let kept = d.path().join("home/.paddock-ci");
        let written = std::fs::read_to_string(d.path().join("github-env")).unwrap();
        let kept_text = paddock_testkit::path::text(&kept);
        assert_eq!(written, format!("npm_config_cache={kept_text}/npm-s1\n"));
        let ng = format!("{kept_text}/angular/e2e_1-s1");
        assert!(Path::new(&ng).is_dir(), "the kept Angular folder exists");
        assert!(
            !d.path().join("ws/ui/.angular/cache").exists(),
            "the stale cache of the workspace is gone"
        );
        assert_eq!(
            links.made.lock().unwrap().as_slice(),
            [(ng.clone(), under(d.path(), "ws/ui/.angular/cache"))]
        );
        assert!(out.contains(&format!("node-caches: npm {kept_text}/npm-s1")), "{out}");
        assert!(
            out.contains(&format!("node-caches: Angular {} -> {ng}", under(d.path(), "ws/ui/.angular/cache"))),
            "{out}"
        );
    }

    #[test]
    fn without_an_angular_dir_only_npm_is_kept() {
        let (d, env) = setup();
        let links = RecordingLinks::default();
        let (code, _) = go(&env, false, "", &links);
        assert_eq!(code, 0);
        assert!(links.made.lock().unwrap().is_empty());
        assert!(d.path().join("ws/ui/.angular/cache/stale").exists(), "the workspace is not touched");
        assert!(!d.path().join("home/.paddock-ci/angular").exists());
    }

    #[test]
    fn the_kept_clone_of_a_host_job_is_the_workspace_when_there_is_one() {
        let (d, mut env) = setup();
        std::fs::create_dir_all(d.path().join("clone/ui")).unwrap();
        env.insert("PADDOCK_WORKSPACE".into(), d.path().join("clone").display().to_string());
        let links = RecordingLinks::default();
        let (code, _) = go(&env, false, "ui", &links);
        assert_eq!(code, 0);
        assert_eq!(links.made.lock().unwrap()[0].1, under(d.path(), "clone/ui/.angular/cache"));
    }

    #[test]
    fn a_container_uses_the_mount_and_one_without_says_so() {
        let (d, mut env) = setup();
        env.insert("RUNNER_OS".into(), "Linux".into());
        let links = RecordingLinks::default();
        let (code, out) = go(&env, false, "ui", &links);
        assert_eq!(code, 0);
        assert!(out.starts_with("::notice::no /sccache mount in this container"), "{out}");
        assert!(!d.path().join("github-env").exists(), "nothing exported without the mount");
        let mount = d.path().join("no-mount");
        std::fs::create_dir(&mount).unwrap();
        let (code, out) = go(&env, false, "ui", &links);
        assert_eq!(code, 0, "{out}");
        assert_eq!(
            std::fs::read_to_string(d.path().join("github-env")).unwrap(),
            format!("npm_config_cache={}\n", mount.join("npm").display())
        );
        assert_eq!(links.made.lock().unwrap()[0].0, under(&mount, "angular/e2e_1"));
    }

    #[test]
    fn windows_writes_native_paths_and_a_missing_slot_fails_with_the_isolations_message() {
        let (d, mut env) = setup();
        let links = RecordingLinks::default();
        let (code, _) = go(&env, true, "", &links);
        assert_eq!(code, 0);
        let written = std::fs::read_to_string(d.path().join("github-env")).unwrap();
        #[cfg(windows)]
        assert!(written.contains('\\'), "{written}");
        #[cfg(not(windows))]
        assert!(written.starts_with("npm_config_cache="), "{written}");
        env.remove("PADDOCK_CI_SLOT");
        let (code, out) = go(&env, false, "", &links);
        assert_eq!(code, 1);
        assert!(out.contains("::error::job-isolation:"), "{out}");
    }
}
