use crate::Exit;

/// `name version` on one line, the text of `--version`.
pub fn version_line(name: &str, version: &str) -> String {
    format!("{name} {version}")
}

/// The text of `--help`, built the same way in every app: usage, commands, options, exit codes, one example.
#[derive(Debug, Clone, Default)]
pub struct Help {
    name: String,
    summary: String,
    commands: Vec<(String, String)>,
    options: Vec<(String, String)>,
    examples: Vec<String>,
    exits: Vec<Exit>,
}

impl Help {
    /// Help for the program `name` with a one-line `summary`.
    pub fn new(name: &str, summary: &str) -> Help {
        Help {
            name: name.into(),
            summary: summary.into(),
            ..Help::default()
        }
    }

    /// A command and what it does.
    pub fn command(mut self, word: &str, what: &str) -> Help {
        self.commands.push((word.into(), what.into()));
        self
    }

    /// An option (`--limit <n>`) and what it does.
    pub fn option(mut self, flag: &str, what: &str) -> Help {
        self.options.push((flag.into(), what.into()));
        self
    }

    /// The options every command has ([`crate::CommonFlags`]).
    pub fn common_options(self) -> Help {
        self.option(
            "--json",
            "write one JSON object to standard output (errors too)",
        )
        .option("-q, --quiet", "no progress or notes on standard error")
        .option("-h, --help", "show this help")
        .option("-V, --version", "show the version")
    }

    /// An example command line.
    pub fn example(mut self, line: &str) -> Help {
        self.examples.push(line.into());
        self
    }

    /// Lists these exit codes (the ones this program can end with).
    pub fn exit_codes(mut self, codes: &[Exit]) -> Help {
        self.exits = codes.to_vec();
        self
    }

    /// The text, ending in a newline.
    pub fn render(&self) -> String {
        let mut out = format!(
            "{} - {}\n\nUsage: {} [options] <command>\n",
            self.name, self.summary, self.name
        );
        let section = |out: &mut String, title: &str, rows: &[(String, String)]| {
            if rows.is_empty() {
                return;
            }
            let w = rows.iter().map(|r| r.0.chars().count()).max().unwrap_or(0);
            out.push_str(&format!("\n{title}:\n"));
            for (k, v) in rows {
                out.push_str(&format!("  {k:<w$}  {v}\n"));
            }
        };
        section(&mut out, "Commands", &self.commands);
        section(&mut out, "Options", &self.options);
        let exits: Vec<(String, String)> = self
            .exits
            .iter()
            .map(|e| {
                (
                    e.code().to_string(),
                    format!("{} - {}", e.name(), e.describe()),
                )
            })
            .collect();
        section(&mut out, "Exit codes", &exits);
        if !self.examples.is_empty() {
            out.push_str("\nExamples:\n");
            for e in &self.examples {
                out.push_str(&format!("  {e}\n"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_aligned_sections_and_skips_empty_ones() {
        let h = Help::new("pit", "talks to the server")
            .command("status", "show the state")
            .command("list-models", "list models")
            .common_options()
            .exit_codes(&[Exit::Ok, Exit::NotRunning])
            .example("pit status --json")
            .render();
        assert!(h.starts_with("pit - talks to the server\n\nUsage: pit [options] <command>\n"));
        assert!(h.contains("  status       show the state\n"), "{h}");
        assert!(h.contains("  --json  ") || h.contains("--json"), "{h}");
        assert!(
            h.contains("  3  not-running - the program this talks to is not running\n"),
            "{h}"
        );
        assert!(h.ends_with("  pit status --json\n"));
        let bare = Help::new("x", "y").render();
        assert!(
            !bare.contains("Commands")
                && !bare.contains("Exit codes")
                && !bare.contains("Examples")
        );
        assert_eq!(version_line("pit", "1.2.3"), "pit 1.2.3");
    }
}
