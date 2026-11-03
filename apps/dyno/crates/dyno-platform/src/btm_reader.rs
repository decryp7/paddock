//! The helper's `LoginItems` reader (ADR 0027 WP-7): runs `/usr/bin/sfltool dumpbtm` (fixed arguments, the runner's 15 s timeout)
//! as the helper's own account (root) and parses the dump here, so the raw text never leaves the helper and other accounts'
//! records are only counted. Pure over a [`ToolRunner`] and a [`Clock`]: tests drive both with fakes.

use crate::parse::btm::parse_view;
use crate::runner::{tool, ToolRunner};
use dyno_core::btm::{LoginItemsReader, LoginItemsReply, MAX_BTM_ITEMS};
use dyno_core::ports::PlatformError;
use paddock_jobs::time::Clock;
use std::sync::Arc;
use std::time::Duration;

const SFLTOOL: &str = "/usr/bin/sfltool";
/// How long `sfltool dumpbtm` may take (ADR 0027 section 5).
const TIMEOUT: Duration = Duration::from_secs(15);

/// Reads login items with `sfltool`.
pub struct SfltoolReader {
    runner: Box<dyn ToolRunner>,
    clock: Arc<dyn Clock>,
}

impl SfltoolReader {
    /// A reader over `runner`.
    pub fn new(runner: Box<dyn ToolRunner>, clock: Arc<dyn Clock>) -> Self {
        SfltoolReader { runner, clock }
    }
}

impl LoginItemsReader for SfltoolReader {
    fn read(&self, uid: u32) -> Result<LoginItemsReply, PlatformError> {
        let started = self.clock.monotonic();
        let ran = self.runner.run_tool(&tool(SFLTOOL, &["dumpbtm"]), TIMEOUT);
        let ms = self.clock.monotonic().saturating_sub(started).as_millis() as u64;
        let out = match ran {
            Ok(o) => o,
            Err(e) => {
                log::warn!(Tool = SFLTOOL, Ms = ms, Error = e.to_string().as_str(); "sfltool dumpbtm did not run to completion");
                return Err(e);
            }
        };
        if !out.ok {
            log::warn!(Tool = SFLTOOL, Ms = ms, Stderr = out.error.as_str(); "sfltool dumpbtm failed");
            return Err(PlatformError::Failed(format!(
                "macOS would not list login items even for the helper (sfltool said: {}). Dyno shows the login items it can see without it.",
                if out.error.is_empty() { "it failed" } else { out.error.as_str() }
            )));
        }
        let view = parse_view(&out.stdout, uid).map_err(|e| {
            let crate::parse::btm::BtmError::NoRecords { first_line } = e;
            log::warn!(Tool = SFLTOOL, Ms = ms, FirstLine = first_line.as_str(); "sfltool dumpbtm printed no records");
            PlatformError::Failed(format!(
                "Login items could not be read (sfltool dumpbtm printed no records; first line: {first_line})"
            ))
        })?;
        let mut items = view.items;
        let truncated = items.len().saturating_sub(MAX_BTM_ITEMS);
        items.truncate(MAX_BTM_ITEMS);
        log::info!(Items = items.len(), Truncated = truncated, OtherAccounts = view.other_accounts, Ms = ms; "Login items read with sfltool");
        Ok(LoginItemsReply {
            items,
            truncated: truncated as u32,
            other_accounts: view.other_accounts as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{Tool, ToolOutput};
    use paddock_jobs::time::FakeClock;
    use std::sync::Mutex;
    use std::time::Duration;

    struct Script {
        answer: Result<ToolOutput, PlatformError>,
        clock: FakeClock,
        took: Duration,
        seen: Arc<Mutex<Vec<Tool>>>,
    }
    impl ToolRunner for Script {
        fn run_tool(&self, t: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
            self.seen.lock().unwrap().push(t.clone());
            self.clock.advance(self.took);
            self.answer.clone()
        }
    }

    const DUMP: &str = "========\n Records for UID 501 : A\n========\n\n Items:\n\n #1:\n   Name: Mine\n   Type: login item (0x4)\n\
   Disposition: [enabled, allowed] (0x3)\n   Identifier: 4.com.example.mine\n\
========\n Records for UID 0 : B\n========\n\n Items:\n\n #1:\n   Name: Sys\n   Type: legacy daemon (0x10010)\n\
   Identifier: 16.com.example.sys\n   Executable Path: /usr/local/bin/sysd\n\
========\n Records for UID 502 : C\n========\n\n Items:\n\n #1:\n   Name: Someone Else\n   Type: agent (0x8)\n   Identifier: 8.x\n";

    fn reader(answer: Result<ToolOutput, PlatformError>) -> (SfltoolReader, Arc<Mutex<Vec<Tool>>>) {
        let clock = FakeClock::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let runner = Script {
            answer,
            clock: clock.clone(),
            took: Duration::from_millis(40),
            seen: seen.clone(),
        };
        (SfltoolReader::new(Box::new(runner), Arc::new(clock)), seen)
    }

    fn ok(stdout: &str) -> Result<ToolOutput, PlatformError> {
        Ok(ToolOutput {
            ok: true,
            stdout: stdout.into(),
            error: String::new(),
        })
    }

    #[test]
    fn runs_the_fixed_command_and_keeps_only_what_the_caller_may_see() {
        let (r, seen) = reader(ok(DUMP));
        let reply = r.read(501).unwrap();
        let ids: Vec<_> = reply.items.iter().map(|i| i.identifier.as_str()).collect();
        assert_eq!(ids, ["4.com.example.mine", "16.com.example.sys"]);
        assert_eq!(reply.other_accounts, 1);
        assert!(!format!("{reply:?}").contains("Someone Else"));
        let seen = seen.lock().unwrap();
        assert_eq!(
            (seen[0].program.as_str(), seen[0].args.as_slice()),
            (SFLTOOL, &["dumpbtm".to_string()][..])
        );
    }

    #[test]
    fn failures_carry_the_reason_and_the_next_step() {
        let (r, _) = reader(Ok(ToolOutput {
            ok: false,
            stdout: String::new(),
            error: "authorization required".into(),
        }));
        let e = r.read(501).unwrap_err();
        assert!(
            matches!(&e, PlatformError::Failed(m) if m.contains("authorization required") && m.contains("without it")),
            "{e:?}"
        );
        let (r, _) = reader(Err(PlatformError::Timeout("sfltool took longer than 15 s".into())));
        assert!(matches!(r.read(501), Err(PlatformError::Timeout(_))));
        let (r, _) = reader(ok("sfltool: unknown\n"));
        assert!(matches!(r.read(501), Err(PlatformError::Failed(m)) if m.contains("sfltool: unknown")));
    }
}
