//! One line per unit finished, for the tools that print only at their end (the watchdog audit of 2026-11-23, cause A: the push gate's
//! conformance, gates-register and ipc-endpoints rows showed no countable progress, so their longest silence was about nine tenths of
//! their run).
//!
//! The push gate sets [`ENV`] to `1` for every row it starts (the children inherit it, ER-47); a tool that finishes a series of units
//! (a check, a phase, a file) calls [`unit`] after each, and the line goes to standard error: the gate's progress counter
//! ([`crate::pre_push::progress`]) counts every non-empty line of such a row. Without the variable nothing is printed, so CI logs and
//! the output a person reads stay as they were. The JavaScript conformance host reads the same variable
//! (conformance/src/cli.mjs).

/// The variable that asks a tool for progress lines: `1`.
pub const ENV: &str = "PADDOCK_PROGRESS_LINES";

/// Whether progress lines were asked for.
pub fn enabled() -> bool {
    enabled_in(&|k| std::env::var(k).ok())
}

/// [`enabled`] over an environment reader (a test passes its own).
pub fn enabled_in(env: &dyn Fn(&str) -> Option<String>) -> bool {
    env(ENV).is_some_and(|v| v.trim() == "1")
}

/// The line for a finished unit.
pub fn line(what: &str) -> String {
    format!("progress: {what}")
}

/// Says that `what` finished, on standard error, when progress lines were asked for.
pub fn unit(what: &str) {
    if enabled() {
        eprintln!("{}", line(what));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_value_one_asks_for_lines() {
        let with = |v: Option<&'static str>| enabled_in(&|k| (k == ENV).then(|| v.map(str::to_string)).flatten());
        assert!(with(Some("1")));
        assert!(with(Some(" 1 ")));
        assert!(!with(Some("0")));
        assert!(!with(Some("")));
        assert!(!with(None));
        assert!(
            !enabled_in(&|_| Some("1".into())) || enabled_in(&|k| (k == ENV).then(|| "1".to_string())),
            "other names are not it"
        );
        assert_eq!(line("C57 finished"), "progress: C57 finished");
    }
}
