//! `./dev setup`: checks a developer machine for the tools of docs/dev-setup.md (the port of `scripts/setup-dev.sh`, shell-to-Rust
//! WP-7). `--install` (macOS with Homebrew) installs what is missing first, then reports again. It never edits a shell profile; it
//! prints the lines to add. `--check` is the same report and exits 1 when a tool is missing, for a script or a gate.
//!
//! The report is pure: the machine is a [`Machine`] (can this program run, which variables are set), so the tests need no tools.

use crate::ci_tool::ToolCall;
use clap::Args;
use paddock_cli::{CliError, Exit, Report};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::Duration;

/// A tool probe: the label, the program and its arguments; the tool is present when the program exits 0.
struct Tool {
    label: &'static str,
    program: &'static str,
    args: &'static [&'static str],
    /// The text of standard output the probe must contain (a target list), when exit 0 is not enough.
    contains: Option<&'static str>,
    /// Only on this OS (`std::env::consts::OS`).
    only_on: Option<&'static str>,
}

const TOOLS: &[Tool] = &[
    Tool {
        label: "rustc",
        program: "rustc",
        args: &["--version"],
        contains: None,
        only_on: None,
    },
    Tool {
        label: "rustfmt",
        program: "cargo",
        args: &["fmt", "--version"],
        contains: None,
        only_on: None,
    },
    Tool {
        label: "clippy",
        program: "cargo",
        args: &["clippy", "--version"],
        contains: None,
        only_on: None,
    },
    Tool {
        label: "windows-msvc target",
        program: "rustup",
        args: &["target", "list", "--installed"],
        contains: Some("x86_64-pc-windows-msvc"),
        only_on: None,
    },
    Tool {
        label: "node",
        program: "node",
        args: &["--version"],
        contains: None,
        only_on: None,
    },
    Tool {
        label: "npm",
        program: "npm",
        args: &["--version"],
        contains: None,
        only_on: None,
    },
    Tool {
        label: "sccache",
        program: "sccache",
        args: &["--version"],
        contains: None,
        only_on: None,
    },
    Tool {
        label: "cargo-nextest",
        program: "cargo",
        args: &["nextest", "--version"],
        contains: None,
        only_on: None,
    },
    Tool {
        label: "cargo-deny",
        program: "cargo",
        args: &["deny", "--version"],
        contains: None,
        only_on: None,
    },
    Tool {
        label: "cargo-llvm-cov",
        program: "cargo",
        args: &["llvm-cov", "--version"],
        contains: None,
        only_on: None,
    },
    Tool {
        label: "cargo-hakari",
        program: "cargo",
        args: &["hakari", "--version"],
        contains: None,
        only_on: None,
    },
    Tool {
        label: "cargo-machete",
        program: "cargo",
        args: &["machete", "--version"],
        contains: None,
        only_on: None,
    },
    Tool {
        label: "docker",
        program: "docker",
        args: &["version", "--format", "{{.Server.Version}}"],
        contains: None,
        only_on: None,
    },
    Tool {
        label: "Xcode Command Line Tools",
        program: "xcode-select",
        args: &["-p"],
        contains: None,
        only_on: Some("macos"),
    },
];

/// What `setup` asks of the machine.
pub trait Machine {
    /// True when `program args...` runs and exits 0 (and, with `contains`, prints that text).
    fn runs(&self, program: &str, args: &[&str], contains: Option<&str>) -> bool;
    /// The value of an environment variable (empty counts as not set).
    fn var(&self, name: &str) -> Option<String>;
    /// The OS name as `std::env::consts::OS` spells it.
    fn os(&self) -> &str;
}

/// The report: the lines, and the labels of the missing tools.
pub struct Checked {
    /// What the script printed.
    pub text: String,
    /// The missing tools' labels, in order.
    pub missing: Vec<&'static str>,
}

/// The tool and environment report, as `setup-dev.sh` printed it.
pub fn report(m: &dyn Machine) -> Checked {
    let mut text = String::from("Tools:\n");
    let mut missing = Vec::new();
    for t in TOOLS.iter().filter(|t| t.only_on.is_none_or(|os| os == m.os())) {
        if m.runs(t.program, t.args, t.contains) {
            let _ = writeln!(text, "  ok       {}", t.label);
        } else {
            let _ = writeln!(text, "  missing  {}", t.label);
            missing.push(t.label);
        }
    }
    text.push_str("Environment:\n");
    match m.var("SCCACHE_DIR") {
        Some(v) => drop(writeln!(text, "  ok       SCCACHE_DIR={v}")),
        None => text.push_str("  not set  SCCACHE_DIR (the cache then lands on the system disk)\n"),
    }
    if m.var("RUSTC_WRAPPER").as_deref() == Some("sccache") {
        text.push_str("  ok       RUSTC_WRAPPER=sccache\n");
    } else {
        text.push_str("  not set  RUSTC_WRAPPER=sccache\n");
    }
    match m.var("CARGO_BUILD_JOBS") {
        Some(v) => drop(writeln!(text, "  ok       CARGO_BUILD_JOBS={v}")),
        None => text.push_str("  not set  CARGO_BUILD_JOBS\n"),
    }
    Checked { text, missing }
}

/// How a missing tool is installed on macOS with Homebrew: the program and arguments, or the line to read.
pub fn install_step(label: &str) -> Result<(&'static str, Vec<String>), String> {
    let strs = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    match label {
        "sccache" => Ok(("brew", strs(&["install", "sccache"]))),
        "cargo-nextest" => Ok(("brew", strs(&["install", "cargo-nextest"]))),
        "cargo-deny" | "cargo-llvm-cov" | "cargo-hakari" | "cargo-machete" => {
            Ok(("cargo", ["install".to_string(), "--locked".to_string(), label.to_string()].to_vec()))
        }
        "windows-msvc target" => Ok(("rustup", strs(&["target", "add", "x86_64-pc-windows-msvc"]))),
        other => Err(format!("  install by hand: {other} (see docs/dev-setup.md)")),
    }
}

/// The profile lines printed when sccache or the job cap is not set up.
pub const PROFILE_HINT: &str = "
Add to your shell profile (~/.zshenv on macOS), between builds:
  export RUSTC_WRAPPER=sccache
  export SCCACHE_DIR=<dev-targets>/sccache   # the folder next to the worktrees; docs/dev-setup.md, \"Where things live\"
  export SCCACHE_CACHE_SIZE=60G
  export CARGO_BUILD_JOBS=2
";

/// The arguments of `./dev setup`.
#[derive(Args)]
pub struct SetupArgs {
    /// macOS with Homebrew: install the missing tools first, then report again.
    #[arg(long)]
    install: bool,
    /// Exit 1 when a tool is missing (the report alone exits 0).
    #[arg(long)]
    check: bool,
}

struct Real {
    env: HashMap<String, String>,
}

impl Machine for Real {
    fn runs(&self, program: &str, args: &[&str], contains: Option<&str>) -> bool {
        let call = ToolCall::new(program)
            .args(args.iter().copied())
            .pass(["RUSTUP_*", "CARGO_*", "DOCKER_*", "SCCACHE_*", "XDG_*"])
            .timeout(Duration::from_secs(30));
        match call.run(&self.env, &mut std::io::sink()) {
            Ok(r) => r.success && contains.is_none_or(|c| r.stdout.contains(c)),
            Err(_) => false,
        }
    }
    fn var(&self, name: &str) -> Option<String> {
        self.env.get(name).filter(|v| !v.is_empty()).cloned()
    }
    fn os(&self) -> &str {
        std::env::consts::OS
    }
}

/// `./dev setup`.
pub fn command(args: &SetupArgs) -> Result<Report, CliError> {
    let real = Real {
        env: std::env::vars().collect(),
    };
    let mut checked = report(&real);
    let mut text = checked.text.clone();
    if args.install && !checked.missing.is_empty() {
        if real.os() != "macos" || !real.runs("brew", &["--version"], None) {
            return Err(CliError::failure(
                "--install needs macOS with Homebrew; install the missing tools as in docs/dev-setup.md.",
            ));
        }
        for label in checked.missing.clone() {
            match install_step(label) {
                Ok((program, a)) => {
                    let ran = ToolCall::new(program)
                        .args(a)
                        .pass(["RUSTUP_*", "CARGO_*", "HOMEBREW_*"])
                        .output(crate::ci_tool::Output::Stream)
                        .timeout(Duration::from_secs(3600))
                        .run(&real.env, &mut std::io::stderr())
                        .map_err(CliError::failure)?;
                    if !ran.success {
                        return Err(CliError::failure(format!(
                            "::error::installing {label} failed: {}. Next step: run the command above by hand and read its output.",
                            ran.how
                        )));
                    }
                }
                Err(line) => {
                    text.push_str(&line);
                    text.push('\n');
                }
            }
        }
        checked = report(&real);
        text.push_str(&checked.text);
    }
    if real.var("RUSTC_WRAPPER").as_deref() != Some("sccache") || real.var("CARGO_BUILD_JOBS").is_none() {
        text.push_str(PROFILE_HINT);
    }
    let data = serde_json::json!({ "missing": checked.missing });
    if args.check && !checked.missing.is_empty() {
        return Err(CliError::new(
            Exit::Findings,
            format!(
                "{text}\n::error::missing tools: {}. Next step: install them as in docs/dev-setup.md (`./dev setup --install` on macOS).",
                checked.missing.join(", ")
            ),
        ));
    }
    Ok(Report::new(data, text.trim_end().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        present: Vec<&'static str>,
        vars: Vec<(&'static str, &'static str)>,
        os: &'static str,
    }

    impl Machine for Fake {
        fn runs(&self, program: &str, args: &[&str], _: Option<&str>) -> bool {
            let key = if program == "cargo" {
                format!("cargo-{}", args[0])
            } else {
                program.to_string()
            };
            self.present.iter().any(|p| *p == key || *p == program && program != "cargo")
        }
        fn var(&self, name: &str) -> Option<String> {
            self.vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
        }
        fn os(&self) -> &str {
            self.os
        }
    }

    #[test]
    fn the_report_lists_present_and_missing_tools_and_the_environment_as_the_script_did() {
        let m = Fake {
            present: vec!["rustc", "cargo-fmt", "node", "cargo-nextest"],
            vars: vec![("SCCACHE_DIR", "/s"), ("RUSTC_WRAPPER", "sccache")],
            os: "linux",
        };
        let c = report(&m);
        assert!(
            c.text
                .starts_with("Tools:\n  ok       rustc\n  ok       rustfmt\n  missing  clippy\n"),
            "{}",
            c.text
        );
        assert!(c.text.contains("  ok       cargo-nextest\n  missing  cargo-deny\n"));
        assert!(!c.text.contains("Xcode"), "the Xcode row is macOS only");
        assert!(
            c.text
                .ends_with("Environment:\n  ok       SCCACHE_DIR=/s\n  ok       RUSTC_WRAPPER=sccache\n  not set  CARGO_BUILD_JOBS\n")
        );
        assert!(c.missing.contains(&"docker") && !c.missing.contains(&"node"));
    }

    #[test]
    fn xcode_is_checked_on_macos_and_an_unset_wrapper_is_named() {
        let m = Fake {
            present: vec![],
            vars: vec![],
            os: "macos",
        };
        let c = report(&m);
        assert!(c.text.contains("  missing  Xcode Command Line Tools\n"));
        assert!(
            c.text
                .contains("  not set  SCCACHE_DIR (the cache then lands on the system disk)\n  not set  RUSTC_WRAPPER=sccache\n")
        );
    }

    #[test]
    fn each_missing_tool_has_its_install_step() {
        assert_eq!(
            install_step("sccache").unwrap(),
            ("brew", vec!["install".to_string(), "sccache".to_string()])
        );
        assert_eq!(install_step("cargo-deny").unwrap().1, ["install", "--locked", "cargo-deny"]);
        assert_eq!(install_step("windows-msvc target").unwrap().0, "rustup");
        assert!(install_step("docker").unwrap_err().contains("install by hand: docker"));
    }
}
