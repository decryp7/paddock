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

/// Logs `$msg` at `$lvl` (warn or error) with `Why = <class of the error>` and any extra key/values after `;`, then the full
/// text of `$err` (`{:#}`) at debug. The message must not contain the error.
#[macro_export]
macro_rules! log_failure {
    ($lvl:ident, $msg:expr, $err:expr $(; $($k:ident = $v:expr),+ )?) => {{
        let __text = format!("{:#}", $err);
        let __msg: String = ($msg).to_string();
        ::log::$lvl!($($($k = $v,)+)? Why = $crate::logsafe::class_of(&__text).as_str(); "{}", __msg);
        ::log::debug!("{}: {}", __msg, __text);
    }};
}

#[cfg(test)]
mod tests {
    use super::class_of;

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
    fn the_macro_compiles_for_every_shape_and_returns_nothing() {
        let id = 7u32;
        let e = anyhow::anyhow!("Binder Error: x");
        crate::log_failure!(warn, "it failed", e);
        crate::log_failure!(error, format!("it failed for {id}"), e; Source = id);
        crate::log_failure!(warn, "plain", std::io::Error::other("boom"); A = 1u32, B = "b");
    }
}
