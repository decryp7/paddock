//! Logging a failure without what it may quote (ER-27, "no user content in logs by default"). An error text can carry a
//! user's file path, a record's value or a query; so the line at warn or error level says what failed and the error's
//! *class* (`Why`), and the whole text goes to a debug line that only a user who raised the level reads.
//!
//! ```ignore
//! debrief_core::log_failure!(warn, "could not remove a partly imported source", e; Source = id);
//! ```

/// DuckDB's error classes end in " Error" ("Conversion Error", "IO Error"); the first `": "`-separated segment of the text that
/// looks like one (letters and spaces, at most 40 characters) is the class, however many contexts come before it. Anything
/// else (a path, a message, a value) gives "error".
pub fn class_of(text: &str) -> String {
    text.split(':')
        .map(str::trim)
        .find(|s| s.len() <= 40 && s.ends_with(" Error") && s.chars().all(|c| c.is_ascii_alphabetic() || c == ' '))
        .map_or_else(|| "error".to_string(), str::to_string)
}

/// What the chain of an error says about its kind without quoting it: the `io::ErrorKind` ("io:PermissionDenied") or the
/// `StoreError` variant ("StoreError::Busy") of the first such error in it; "" when it has neither.
pub fn kind_in_chain<'a>(chain: impl Iterator<Item = &'a (dyn std::error::Error + 'static)>) -> String {
    for e in chain {
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            return format!("io:{:?}", io.kind());
        }
        if let Some(se) = e.downcast_ref::<crate::ports::StoreError>() {
            return format!("StoreError::{}", store_error_variant(se));
        }
    }
    String::new()
}

/// The variant of a [`crate::ports::StoreError`], without its message.
pub fn store_error_variant(e: &crate::ports::StoreError) -> &'static str {
    use crate::ports::StoreError as E;
    match e {
        E::Unsupported { .. } => "Unsupported",
        E::Invalid(_) => "Invalid",
        E::Query(_) => "Query",
        E::NotFound(_) => "NotFound",
        E::Busy(_) => "Busy",
        E::Cancelled => "Cancelled",
        E::Io(_) => "Io",
        E::Engine(_) => "Engine",
        E::Damaged(_) => "Damaged",
        E::Locked(_) => "Locked",
    }
}

/// `Why` of a failure line: the database class when the text has one, else the kind found in the error's chain, else "error".
pub fn why(text: &str, kind: &str) -> String {
    match class_of(text) {
        c if c != "error" => c,
        _ if !kind.is_empty() => kind.to_string(),
        c => c,
    }
}

/// Finds the kind of any error type the macro gets, by autoref specialisation: a `std::error::Error` (io, StoreError, DuckDB), an
/// `anyhow::Error`, or anything printable (no kind).
pub struct Probe<'a, T: ?Sized>(pub &'a T);

/// For a `std::error::Error`.
pub trait ViaError {
    fn error_kind(&self) -> String;
}

impl<T: std::error::Error + 'static> ViaError for Probe<'_, T> {
    fn error_kind(&self) -> String {
        kind_in_chain(std::iter::successors(Some(self.0 as &(dyn std::error::Error + 'static)), |e| {
            e.source()
        }))
    }
}

/// For an `anyhow::Error`.
pub trait ViaAnyhow {
    fn error_kind(&self) -> String;
}

impl ViaAnyhow for Probe<'_, anyhow::Error> {
    fn error_kind(&self) -> String {
        kind_in_chain(self.0.chain())
    }
}

impl ViaAnyhow for Probe<'_, &anyhow::Error> {
    fn error_kind(&self) -> String {
        kind_in_chain(self.0.chain())
    }
}

/// For anything else that prints: no kind.
pub trait ViaNothing {
    fn error_kind(&self) -> String;
}

impl<T: ?Sized> ViaNothing for &Probe<'_, T> {
    fn error_kind(&self) -> String {
        String::new()
    }
}

/// Logs `$msg` at `$lvl` (warn or error) with `Why = <class of the error, else its io or StoreError kind>` and any extra
/// key/values after `;`, then the full text of `$err` (`{:#}`) at debug. The message must not contain the error.
#[macro_export]
macro_rules! log_failure {
    ($lvl:ident, $msg:expr, $err:expr $(; $($k:ident = $v:expr),+ )?) => {{
        #[allow(unused_imports)]
        use $crate::logsafe::{ViaAnyhow as _, ViaError as _, ViaNothing as _};
        let __err = &$err;
        let __text = format!("{:#}", __err);
        let __kind = (&$crate::logsafe::Probe(__err)).error_kind();
        let __msg: String = ($msg).to_string();
        ::log::$lvl!($($($k = $v,)+)? Why = $crate::logsafe::why(&__text, &__kind).as_str(); "{}", __msg);
        ::log::debug!("{}: {}", __msg, __text);
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_database_error_class_passes() {
        assert_eq!(
            class_of("Conversion Error: Could not convert string 'customer-4711' to INT32"),
            "Conversion Error"
        );
        assert_eq!(class_of("IO Error: x"), "IO Error");
        assert_eq!(class_of("/home/secret-user/secret-name.log: permission denied"), "error");
        assert_eq!(
            class_of("secret name: denied"),
            "error",
            "a first segment that is not a class is a name or a value"
        );
        assert_eq!(
            class_of("a very long message that only has letters and spaces in it Error: x"),
            "error"
        );
        assert_eq!(class_of("no colon here"), "error");
        assert_eq!(class_of(""), "error");
        assert_eq!(
            class_of("indexing seg-0001: IO Error: could not read value 5"),
            "IO Error",
            "after contexts"
        );
        assert_eq!(class_of("/home/secret-user/x.log: Permission denied"), "error");
    }

    #[test]
    fn why_names_the_database_class_or_else_the_io_kind_or_the_store_error() {
        assert_eq!(why("Conversion Error: x", "io:NotFound"), "Conversion Error");
        assert_eq!(why("plain", "io:PermissionDenied"), "io:PermissionDenied");
        assert_eq!(why("plain", ""), "error");
        let io = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!((&Probe(&io)).error_kind(), "io:PermissionDenied");
        let nested = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::NotFound)).context("reading x");
        assert_eq!((&Probe(&nested)).error_kind(), "io:NotFound");
        assert_eq!((&Probe(&&nested)).error_kind(), "io:NotFound");
        let busy = crate::ports::StoreError::Busy("an import is running on secret-folder".into());
        assert_eq!((&Probe(&busy)).error_kind(), "StoreError::Busy");
        let wrapped = anyhow::Error::new(crate::ports::StoreError::Cancelled).context("x");
        assert_eq!((&Probe(&wrapped)).error_kind(), "StoreError::Cancelled");
        assert_eq!((&Probe(&"just text")).error_kind(), "");
        for (e, v) in [
            (crate::ports::StoreError::Invalid(String::new()), "Invalid"),
            (crate::ports::StoreError::NotFound(String::new()), "NotFound"),
            (crate::ports::StoreError::Io(String::new()), "Io"),
            (crate::ports::StoreError::Engine(String::new()), "Engine"),
            (crate::ports::StoreError::Damaged(String::new()), "Damaged"),
            (crate::ports::StoreError::Locked(String::new()), "Locked"),
            (crate::ports::StoreError::unsupported("x", "1"), "Unsupported"),
        ] {
            assert_eq!(store_error_variant(&e), v);
        }
    }

    #[test]
    fn the_macro_compiles_for_every_shape_and_returns_nothing() {
        let id = 7u32;
        let e = anyhow::anyhow!("Binder Error: x");
        crate::log_failure!(warn, "it failed", e);
        crate::log_failure!(error, format!("it failed for {id}"), e; Source = id);
        crate::log_failure!(warn, "plain", std::io::Error::other("boom"); A = 1u32, B = "b");
    }
}
