//! Report a problem (the shell's dialog, D6 of the logging ADR): paddock-diag's sections plus what only Dyno has. The dated logs of
//! both processes (`dyno.<day>.log` for the app, `dyno-agent.<day>.log` for the agent) are in the built-in Logs section, sharing
//! its window; the agent's own `agent.log` (its start-up refusals, written when the dated log cannot be) is read straight from the
//! data folder. Nothing here asks the agent: the report works with the agent down, which is when it is wanted.

use paddock_diag::window::tail_of;
use paddock_diag::{Content, Context, FileKind, Section, SectionFile, SectionOutput};
use std::path::PathBuf;

/// The most of `agent.log` a report reads.
pub const AGENT_LOG_WINDOW: u64 = 256 * 1024;

/// The agent's `agent.log`, read from the data folder.
pub struct AgentFiles {
    /// The agent's log folder (`<data folder>/logs`).
    logs: PathBuf,
}

impl AgentFiles {
    /// The section over the agent's log folder.
    pub fn new(logs: PathBuf) -> Self {
        AgentFiles { logs }
    }
}

impl Section for AgentFiles {
    fn id(&self) -> &str {
        "agent-files"
    }

    fn collect(&self, _cx: &Context<'_>) -> SectionOutput {
        let path = self.logs.join("agent.log");
        match tail_of(&path, AGENT_LOG_WINDOW) {
            Some((bytes, cut)) if !bytes.is_empty() => {
                let part = if cut { "the recent part of" } else { "all of" };
                SectionOutput::one(SectionFile {
                    name: "agent/agent.log".into(),
                    kind: FileKind::Log,
                    summary: format!("{part} the agent's own log file (read from disk; the agent was not asked)"),
                    content: Content::Log(bytes),
                })
            }
            _ => SectionOutput::note("The agent wrote no agent.log (nothing refused its start)."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_diag::AppInfo;
    use paddock_env::{AppDirs, AppId};

    fn collect(logs: &std::path::Path) -> SectionOutput {
        let app = AppInfo::new("dyno", "Dyno", "1.0.0", "");
        let dirs = AppDirs::for_app(&AppId::new("dyno", "Dyno").unwrap()).unwrap();
        let cx = Context {
            app: &app,
            dirs: &dirs,
            now: paddock_log::now_utc(&paddock_log::SystemClock),
        };
        AgentFiles::new(logs.to_path_buf()).collect(&cx)
    }

    #[test]
    fn the_agents_own_log_is_read_from_disk_without_asking_the_agent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("agent.log"), "dyno-agent: another Dyno agent is already running\n").unwrap();
        let out = collect(dir.path());
        assert_eq!(out.files.len(), 1);
        assert_eq!(out.files[0].name, "agent/agent.log");
        assert!(matches!(&out.files[0].content, Content::Log(b) if b.starts_with(b"dyno-agent: another")));
        // Nothing written: a note, not an error.
        let none = collect(&dir.path().join("missing"));
        assert!(none.files.is_empty() && !none.notes.is_empty());
    }

    #[test]
    fn a_big_agent_log_is_cut_to_its_window_at_a_line() {
        let dir = tempfile::tempdir().unwrap();
        let line = "dyno-agent: could not store an alarm change\n";
        std::fs::write(dir.path().join("agent.log"), line.repeat(20_000)).unwrap();
        let out = collect(dir.path());
        let Content::Log(bytes) = &out.files[0].content else { panic!() };
        assert!(bytes.len() as u64 <= AGENT_LOG_WINDOW && bytes.starts_with(line.as_bytes()));
    }
}
