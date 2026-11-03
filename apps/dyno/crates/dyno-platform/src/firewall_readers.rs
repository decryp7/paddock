//! The firewall readers (ADR 0027 WP-9), pure over a [`ToolRunner`] and a [`Clock`]: the macOS Application Firewall (the agent, no
//! rights), `pf` (the helper, root) and nftables (the helper, root). Fixed arguments only. Logs carry counts and durations, never
//! rule addresses, programs or ports.

use crate::parse::firewall::{alf_view, parse_nft, pf_view, FirewallParseError};
use crate::runner::{tool, ToolOutput, ToolRunner};
use dyno_core::firewall::{FirewallReader, FirewallSource};
use dyno_core::ports::PlatformError;
use paddock_jobs::time::Clock;
use std::sync::Arc;
use std::time::Duration;

const SOCKETFILTERFW: &str = "/usr/libexec/ApplicationFirewall/socketfilterfw";
const PFCTL: &str = "/sbin/pfctl";

/// How long one firewall tool may take (ADR 0027 section 5: 15 s).
const TIMEOUT: Duration = Duration::from_secs(15);

struct Runs {
    runner: Box<dyn ToolRunner>,
    clock: Arc<dyn Clock>,
}

impl Runs {
    fn new(runner: Box<dyn ToolRunner>, clock: Arc<dyn Clock>) -> Self {
        Runs {
            runner,
            clock,
        }
    }

    fn run(&self, program: &str, args: &[&str]) -> Result<ToolOutput, PlatformError> {
        let started = self.clock.monotonic();
        let out = self.runner.run_tool(&tool(program, args), TIMEOUT);
        let ms = self.clock.monotonic().saturating_sub(started).as_millis() as u64;
        match &out {
            Ok(o) => log::debug!(Tool = program, Ok = o.ok, Ms = ms; "Firewall tool ran"),
            Err(e) => log::warn!(Tool = program, Ms = ms, Error = e.to_string().as_str(); "Firewall tool did not run to completion"),
        }
        out
    }

    /// Runs the tool and insists on a clean exit; the error names the tool and its first stderr line.
    fn run_ok(&self, program: &str, args: &[&str], hint: &str) -> Result<String, PlatformError> {
        let out = self.run(program, args)?;
        if out.ok {
            Ok(out.stdout)
        } else {
            Err(PlatformError::Failed(format!(
                "{} failed ({}). {hint}",
                program.rsplit('/').next().unwrap_or(program),
                if out.error.is_empty() { "no message" } else { out.error.as_str() }
            )))
        }
    }
}

fn parse_err(e: FirewallParseError) -> PlatformError {
    PlatformError::Failed(e.to_string())
}

fn log_view(what: &str, v: &FirewallSource) {
    log::info!(Rules = v.rule_count, Truncated = v.truncated; "{what} read");
}

/// The macOS Application Firewall, read by the agent as the user (WP-0: `socketfilterfw --get*` and `--listapps` exit 0).
pub struct AlfReader(Runs);

impl AlfReader {
    /// A reader over `runner`.
    pub fn new(runner: Box<dyn ToolRunner>, clock: Arc<dyn Clock>) -> Self {
        AlfReader(Runs::new(runner, clock))
    }
}

impl FirewallReader for AlfReader {
    fn read(&self) -> Result<FirewallSource, PlatformError> {
        let hint = "The Application Firewall could not be read.";
        let global = self.0.run_ok(SOCKETFILTERFW, &["--getglobalstate"], hint)?;
        let block = self
            .0
            .run_ok(SOCKETFILTERFW, &["--getblockall"], hint)
            .unwrap_or_default();
        let stealth = self
            .0
            .run_ok(SOCKETFILTERFW, &["--getstealthmode"], hint)
            .unwrap_or_default();
        let apps = self.0.run_ok(SOCKETFILTERFW, &["--listapps"], hint).unwrap_or_default();
        let v = alf_view(&global, &block, &stealth, &apps).map_err(parse_err)?;
        log_view("Application Firewall", &v);
        Ok(v)
    }
}

/// macOS `pf`, read by the helper as root.
pub struct PfReader(Runs);

impl PfReader {
    /// A reader over `runner`.
    pub fn new(runner: Box<dyn ToolRunner>, clock: Arc<dyn Clock>) -> Self {
        PfReader(Runs::new(runner, clock))
    }
}

impl FirewallReader for PfReader {
    fn read(&self) -> Result<FirewallSource, PlatformError> {
        let hint = "pf can only be read with administrator rights; turn on administrator readings.";
        let info = self.0.run_ok(PFCTL, &["-s", "info"], hint)?;
        let rules = self.0.run_ok(PFCTL, &["-s", "rules"], hint)?;
        let v = pf_view(&info, &rules).map_err(parse_err)?;
        log_view("pf", &v);
        Ok(v)
    }
}

/// Linux nftables, read by the helper as root. `nft` lives at a protected path chosen by the helper.
pub struct NftReader {
    nft: String,
    runs: Runs,
}

impl NftReader {
    /// A reader that runs `nft` (an absolute path) through `runner`.
    pub fn new(nft: impl Into<String>, runner: Box<dyn ToolRunner>, clock: Arc<dyn Clock>) -> Self {
        NftReader {
            nft: nft.into(),
            runs: Runs::new(runner, clock),
        }
    }
}

impl FirewallReader for NftReader {
    fn read(&self) -> Result<FirewallSource, PlatformError> {
        let json = self.runs.run_ok(
            &self.nft,
            &["-j", "list", "ruleset"],
            "nft needs administrator rights, and may not be installed.",
        )?;
        let v = parse_nft(&json).map_err(parse_err)?;
        log_view("nftables", &v);
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::Tool;
    use std::sync::Mutex;
    use dyno_core::firewall::{FirewallSwitch, RuleSource};
    use paddock_jobs::time::FakeClock;

    /// Answers by the arguments, so the test says what each command prints.
    struct Table(Vec<(&'static str, Result<ToolOutput, PlatformError>)>, Arc<Mutex<Vec<String>>>);
    impl ToolRunner for Table {
        fn run_tool(&self, t: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
            let line = format!("{} {}", t.program, t.args.join(" "));
            self.1.lock().unwrap().push(line.clone());
            self.0
                .iter()
                .find(|(k, _)| line.ends_with(k))
                .map(|(_, v)| v.clone())
                .expect("an unscripted command")
        }
    }
    fn say(s: &str) -> Result<ToolOutput, PlatformError> {
        Ok(ToolOutput {
            ok: true,
            stdout: s.into(),
            error: String::new(),
        })
    }
    fn clock() -> Arc<dyn Clock> {
        Arc::new(FakeClock::new())
    }

    #[test]
    fn the_application_firewall_is_four_fixed_commands() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let runner = Table(
            vec![
                ("--getglobalstate", say("Firewall is enabled. (State = 1)")),
                ("--getblockall", say("Block all DISABLED!")),
                ("--getstealthmode", say("Stealth mode disabled")),
                (
                    "--listapps",
                    say("Total number of apps = 1\n\n1 :  /Applications/X.app\n\t ( Allow incoming connections )\n"),
                ),
            ],
            seen.clone(),
        );
        let v = AlfReader::new(Box::new(runner), clock()).read().unwrap();
        assert_eq!((v.source, v.rule_count, v.switches.len()), (RuleSource::ApplicationFirewall, 1, 2));
        assert_eq!(v.switches[1], (FirewallSwitch::Stealth, false));
        assert_eq!(seen.lock().unwrap().len(), 4);
    }

    #[test]
    fn pf_and_nft_failures_say_what_to_do() {
        let denied = Ok(ToolOutput {
            ok: false,
            stdout: String::new(),
            error: "pfctl: /dev/pf: Permission denied".into(),
        });
        let r = PfReader::new(Box::new(Table(vec![("-s info", denied)], Arc::default())), clock());
        assert!(matches!(r.read(), Err(PlatformError::Failed(m)) if m.contains("Permission denied") && m.contains("administrator")));
        let r = NftReader::new(
            "/usr/sbin/nft",
            Box::new(Table(
                vec![("list ruleset", Err(PlatformError::Timeout("15 s".into())))],
                Arc::default(),
            )),
            clock(),
        );
        assert!(matches!(r.read(), Err(PlatformError::Timeout(_))));
        let r = NftReader::new(
            "/usr/sbin/nft",
            Box::new(Table(vec![("list ruleset", say(r#"{"nftables":[]}"#))], Arc::default())),
            clock(),
        );
        let v = r.read().unwrap();
        assert_eq!((v.source, v.rule_count, v.states[0].enabled), (RuleSource::Nftables, 0, false));
        let r = PfReader::new(
            Box::new(Table(
                vec![("-s info", say("Status: Disabled")), ("-s rules", say(""))],
                Arc::default(),
            )),
            clock(),
        );
        assert!(!r.read().unwrap().states[0].enabled);
    }
}
