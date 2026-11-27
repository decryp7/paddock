//! The one writer of a curl `-K` config (user decision of the train aa round, ER-21 and ER-22). Every value this tool writes into a curl
//! config, a path, a URL or a secret, goes through [`line`], so the escape rules of curl's config format are applied in one place.
//!
//! The rule, from the curl manual (`man curl`, option `-K, --config <file>`, "Specify how options are written in the config file"): a value
//! with spaces or a leading equals sign must be in double quotes, and "within double quotes the following escape sequences are available:
//! `\\`, `\"`, `\t`, `\n`, `\r` and `\v`. A backslash preceding any other letter is ignored." So a Windows path (`D:\a\_temp\x`) written
//! unescaped reaches curl as another path (run 9265: the Windows leg of the test cache recorded nothing), and a `"` in a value ends it early.
//! Two incidents of this class (run 9135 wrote `http://http://`, run 9265 the backslashes) are the reason for the one writer.

/// `v` as the inside of a double-quoted curl config value.
pub fn value(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 2);
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{b}' => out.push_str("\\v"),
            c => out.push(c),
        }
    }
    out
}

/// One config line, `key = "value"` and a newline, the value escaped.
pub fn line(key: &str, v: &str) -> String {
    format!("{key} = \"{}\"\n", value(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_path_keeps_its_backslashes_and_a_quote_cannot_end_the_value() {
        assert_eq!(line("output", r"D:\a\_temp\t\r"), "output = \"D:\\\\a\\\\_temp\\\\t\\\\r\"\n");
        assert_eq!(line("header", "x\"y"), "header = \"x\\\"y\"\n");
        assert_eq!(value("a\tb\nc\r\u{b}"), "a\\tb\\nc\\r\\v");
        assert_eq!(value("plain /path:1"), "plain /path:1");
    }

    /// The positive control: the real curl reads the escaped value back as the original, for every escape the manual lists.
    #[test]
    fn curl_reads_back_what_the_writer_wrote() {
        use paddock_proc::{Limits, run_bounded_with_input};
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src.txt");
        std::fs::write(&src, "hello").unwrap();
        let out = d.path().join("s\\t\\r\\v\"q").join("o\\n");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        let cfg = format!(
            "{}{}silent\nshow-error\nwrite-out = \"%{{filename_effective}}\"\n",
            line("url", &format!("file://{}", src.display())),
            line("output", &out.display().to_string())
        );
        let mut c = paddock_proc::command("curl");
        c.args(["-q", "-K", "-"]);
        let r = run_bounded_with_input(c, cfg.into_bytes(), Limits::new(std::time::Duration::from_secs(20))).unwrap();
        assert!(r.success(), "{}", r.stderr_text());
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "hello");
        assert_eq!(r.stdout_text().trim(), out.display().to_string());
    }
}
