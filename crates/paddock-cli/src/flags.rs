/// How results are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// For people.
    #[default]
    Text,
    /// One JSON object on standard output.
    Json,
}

/// The flags every command accepts, taken out of the arguments before the app parses its own.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommonFlags {
    /// Text or JSON (`--json`).
    pub format: Format,
    /// No progress and no notes on stderr (`--quiet`, `-q`).
    pub quiet: bool,
    /// `--help` or `-h` was given: print the help and exit 0.
    pub help: bool,
    /// `--version` or `-V` was given: print the version line and exit 0.
    pub version: bool,
}

impl CommonFlags {
    /// Takes the common flags out of `args` (program name not included) and returns them with the arguments that are
    /// left, in order. Everything after a bare `--` is kept untouched (it belongs to the command).
    pub fn extract<S: AsRef<str>>(args: &[S]) -> (CommonFlags, Vec<String>) {
        let mut flags = CommonFlags::default();
        let mut rest = Vec::with_capacity(args.len());
        let mut literal = false;
        for a in args {
            let a = a.as_ref();
            if literal {
                rest.push(a.to_string());
                continue;
            }
            match a {
                "--" => {
                    literal = true;
                    rest.push(a.to_string());
                }
                "--json" => flags.format = Format::Json,
                "--quiet" | "-q" => flags.quiet = true,
                "--help" | "-h" => flags.help = true,
                "--version" | "-V" => flags.version = true,
                _ => rest.push(a.to_string()),
            }
        }
        (flags, rest)
    }

    /// Whether progress lines should be written (not `--quiet`, and not JSON mode, which keeps stderr for errors only
    /// when the caller says so; progress is wanted on stderr in both modes unless `--quiet`).
    pub fn show_progress(&self) -> bool {
        !self.quiet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_common_flags_anywhere_and_keeps_the_rest_in_order() {
        let (f, rest) = CommonFlags::extract(&["list", "--json", "-q", "--limit", "5", "-V"]);
        assert_eq!(f.format, Format::Json);
        assert!(f.quiet && f.version && !f.help);
        assert_eq!(rest, ["list", "--limit", "5"]);
        assert!(!f.show_progress());
        let (f, rest) = CommonFlags::extract::<&str>(&[]);
        assert_eq!(f, CommonFlags::default());
        assert!(rest.is_empty() && f.show_progress());
    }

    #[test]
    fn after_a_double_dash_nothing_is_taken() {
        let (f, rest) = CommonFlags::extract(&["run", "--", "--json", "-h"]);
        assert_eq!(f, CommonFlags::default());
        assert_eq!(rest, ["run", "--", "--json", "-h"]);
        let (f, _) = CommonFlags::extract(&["--help"]);
        assert!(f.help);
    }
}
