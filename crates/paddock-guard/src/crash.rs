use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// A contained crash (a panic, or an owner process that died) as a typed error, so callers report it as a crash without
/// reading the text (`"{what} crashed: {message}"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crashed {
    /// What crashed: `Debrief`, `plugin "x" (import)`.
    pub what: String,
    /// The panic message.
    pub message: String,
}

impl Crashed {
    /// A crash of `what` with `message`.
    pub fn new(what: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Crashed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} crashed: {}", self.what, self.message)
    }
}

impl std::error::Error for Crashed {}

/// Runs `f`, turning a panic into [`Crashed`] (`"{what} crashed: ..."`).
pub fn catch<T>(what: &str, f: impl FnOnce() -> T) -> Result<T, Crashed> {
    catch_unwind(AssertUnwindSafe(f))
        .map_err(|payload| Crashed::new(what, panic_message(payload.as_ref())))
}

/// The message of a panic payload (`"unknown panic"` when it is neither a `&str` nor a `String`).
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catch_passes_results_and_reports_panics() {
        assert_eq!(catch("x", || 5), Ok(5));
        let n = std::hint::black_box(1);
        let e = catch::<()>("parser", || panic!("boom {n}")).unwrap_err();
        assert_eq!(e, Crashed::new("parser", "boom 1"));
        assert_eq!(e.to_string(), "parser crashed: boom 1");
        let e = catch::<()>("p", || panic!("static text")).unwrap_err();
        assert_eq!(e.message, "static text");
    }

    #[test]
    fn a_non_string_payload_is_an_unknown_panic() {
        let e = catch::<()>("p", || std::panic::panic_any(42u8)).unwrap_err();
        assert_eq!(e.to_string(), "p crashed: unknown panic");
        let _: &dyn std::error::Error = &e;
    }
}
