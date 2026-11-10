//! What must hold for a flake to be visible at all (ER-33): a probe that cannot see the thing reports "not measured".
//!
//! Playwright with `retries > 0` and no `failOnFlakyTests` passes a job whose test failed once and passed on the retry; the log
//! of a passing job is never read, so the flake is invisible. The app's `playwright.config.ts` says whether that can happen.

use regex::Regex;
use std::io::ErrorKind;

/// The message for an app whose Playwright flakes are invisible in passing jobs.
pub const PLAYWRIGHT_HIDES_FLAKES: &str = "Playwright flakes in passing jobs are not visible (no failOnFlakyTests)";

/// The Playwright precondition of an app, from its config text (`None`: the config is fine, or the app has none).
/// `read` returns the file's text for a path under the repository root.
pub fn playwright(app: &str, read: &dyn Fn(&str) -> std::io::Result<String>) -> Option<String> {
    let path = format!("apps/{app}/app/playwright.config.ts");
    let text = match read(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == ErrorKind::NotFound => return None,
        Err(e) => {
            return Some(format!(
                "{path} could not be read ({e}), so whether Playwright flakes are visible is unknown"
            ));
        }
    };
    // Comments do not count.
    let code: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let value = |key: &str| -> Option<String> {
        #[allow(clippy::expect_used)]
        let r = Regex::new(&format!(r"\b{key}\s*:\s*([^,\n}}]+)")).expect("a valid pattern");
        r.captures(&code).map(|c| c[1].trim().to_string())
    };
    let retries = value("retries");
    let retries_off = retries.as_deref().is_none_or(|v| v == "0");
    let fails_on_flaky = value("failOnFlakyTests").is_some_and(|v| v != "false" && v != "0");
    (!retries_off && !fails_on_flaky).then(|| PLAYWRIGHT_HIDES_FLAKES.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(text: &'static str) -> impl Fn(&str) -> std::io::Result<String> {
        move |p| {
            assert_eq!(p, "apps/dyno/app/playwright.config.ts");
            Ok(text.to_string())
        }
    }

    #[test]
    fn retries_without_fail_on_flaky_tests_hide_flakes() {
        // The shape of Dyno's, Spotter's and Pitcrew's configs.
        let r = playwright(
            "dyno",
            &cfg("export default defineConfig({\n  retries: process.env['CI'] ? 1 : 0,\n});"),
        );
        assert_eq!(r.as_deref(), Some(PLAYWRIGHT_HIDES_FLAKES));
    }

    #[test]
    fn fail_on_flaky_tests_or_no_retries_make_flakes_visible() {
        assert!(playwright("dyno", &cfg("retries: CI ? 1 : 0,\n  failOnFlakyTests: true,")).is_none());
        assert!(
            playwright(
                "dyno",
                &cfg("  // A flake is fixed, not retried (rule 5): no retries.\n  retries: 0,")
            )
            .is_none()
        );
        // Retries commented out, or an explicit false, do not count as off.
        assert!(playwright("dyno", &cfg("// retries: 2,\nworkers: 1,")).is_none());
        assert!(playwright("dyno", &cfg("retries: 2,\nfailOnFlakyTests: false,")).is_some());
    }

    #[test]
    fn no_config_is_no_precondition_and_an_unreadable_one_is_unknown() {
        assert!(playwright("dyno", &|_| Err(std::io::Error::from(ErrorKind::NotFound))).is_none());
        let r = playwright("dyno", &|_| Err(std::io::Error::from(ErrorKind::PermissionDenied)));
        assert!(r.unwrap().contains("could not be read"));
    }
}
