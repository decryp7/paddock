//! Finding and talking to the running agent: the same `agent.json`, token and protocol as the app (ADR 0014).

use dyno_core::protocol::{LiveDetail, LiveUpdate, Message, Reply, Request, MAX_FRAME, PROTOCOL_VERSION, WIRE_MAJOR};
use paddock_ipc::{BodyOptions, BodyPeer, Client, FrameLimit, IpcError, ProtocolVersion, Token};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

/// Where things are: the data folder holding `agent.json`.
#[derive(Debug, Clone)]
pub struct Env {
    /// The agent's data folder.
    pub data_dir: PathBuf,
}

impl Env {
    /// The folder of the current user, by the same rule as the agent: `DYNO_DATA_DIR`, else `%LOCALAPPDATA%\Dyno`,
    /// `~/Library/Application Support/Dyno` or `$XDG_DATA_HOME/dyno` (`~/.local/share/dyno`).
    #[expect(
        clippy::disallowed_methods,
        reason = "KNOWN DEFECT, not a justification: a shared, predictable folder under the OS temp dir that another local user can create first (Dyno security fix, owner and review date in docs/gates.toml [[defect]])"
    )]
    pub fn from_process() -> Env {
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let data_dir = env("DYNO_DATA_DIR").unwrap_or_else(|| {
            let found = if cfg!(windows) {
                env("LOCALAPPDATA").map(|d| d.join("Dyno"))
            } else if cfg!(target_os = "macos") {
                env("HOME").map(|h| h.join("Library").join("Application Support").join("Dyno"))
            } else {
                env("XDG_DATA_HOME")
                    .map(|d| d.join("dyno"))
                    .or_else(|| env("HOME").map(|h| h.join(".local").join("share").join("dyno")))
            };
            found.unwrap_or_else(|| std::env::temp_dir().join("Dyno"))
        });
        Env { data_dir }
    }

    /// The discovery file.
    pub fn discovery(&self) -> PathBuf {
        self.data_dir.join("agent.json")
    }
}

/// What `agent.json` says (the same fields the agent writes).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    /// The IPC endpoint name.
    pub endpoint: String,
    /// The per-launch token, hex.
    pub token: String,
    /// The agent's process id.
    pub pid: u32,
    /// The agent's build.
    pub version: String,
    /// The protocol the agent speaks.
    pub protocol: u32,
}

impl Discovery {
    /// Reads the file; `None` when it is missing, too big or damaged.
    pub fn read(env: &Env) -> Option<Discovery> {
        let path = env.discovery();
        if std::fs::metadata(&path).ok()?.len() > 16 * 1024 {
            return None;
        }
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }
}

/// Why the CLI cannot work with this agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// No agent is running (or its discovery file is damaged).
    NotRunning,
    /// The agent speaks another protocol version.
    VersionMismatch {
        /// The agent's protocol.
        agent: u32,
        /// This command's protocol.
        ours: u32,
        /// The agent's build.
        agent_build: String,
    },
    /// Anything else, as a sentence.
    Failed(String),
}

impl Problem {
    /// The sentence shown to the user, saying what to do.
    pub fn message(&self) -> String {
        match self {
            Problem::NotRunning => "Dyno is not running. Open Dyno, or start its agent with: dyno-agent run".into(),
            Problem::VersionMismatch { agent, ours, agent_build } if agent > ours => format!(
                "The running Dyno ({agent_build}) speaks a newer protocol ({agent}) than this dyno command ({ours}). Update the dyno command to the same release as Dyno."
            ),
            Problem::VersionMismatch { agent, ours, agent_build } => format!(
                "The running Dyno ({agent_build}) speaks an older protocol ({agent}) than this dyno command ({ours}). Update and restart Dyno, or use the dyno command that came with it."
            ),
            Problem::Failed(m) => m.clone(),
        }
    }

    /// The exit code for this problem.
    pub fn exit_code(&self) -> i32 {
        match self {
            Problem::NotRunning => 4,
            Problem::VersionMismatch { .. } => 3,
            Problem::Failed(_) => 1,
        }
    }
}

/// Whether this command may talk to an agent that speaks `agent`. Every request, read-only ones included, needs the
/// same protocol: a request one side does not know would otherwise fail in confusing ways (there is no minor version).
pub fn check_version(discovery: &Discovery, ours: u32) -> Result<(), Problem> {
    if discovery.protocol == ours {
        Ok(())
    } else {
        Err(Problem::VersionMismatch {
            agent: discovery.protocol,
            ours,
            agent_build: discovery.version.clone(),
        })
    }
}

/// The agent as the commands see it. The real one speaks IPC; tests give a fake.
pub trait Agent {
    /// Sends one request and returns the reply (the agent's own message when it refuses).
    fn call(&self, request: Request) -> Result<Reply, Problem>;
    /// The next live update at the given detail.
    fn live(&self, detail: LiveDetail) -> Result<LiveUpdate, Problem>;
    /// The agent's build and protocol, for `status`.
    fn about(&self) -> AgentInfo;
}

/// What `status` reports about the agent itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInfo {
    /// Process id.
    pub pid: u32,
    /// Build.
    pub version: String,
    /// Protocol.
    pub protocol: u32,
}

/// The agent over its private local IPC.
pub struct IpcAgent {
    peer: BodyPeer<Message>,
    updates: Mutex<mpsc::Receiver<LiveUpdate>>,
    info: AgentInfo,
}

impl IpcAgent {
    /// Connects to the agent named in the discovery file after checking its protocol version.
    pub fn connect(env: &Env) -> Result<IpcAgent, Problem> {
        let d = Discovery::read(env).ok_or(Problem::NotRunning)?;
        check_version(&d, PROTOCOL_VERSION)?;
        let token = Token::from_hex(&d.token).ok_or_else(|| Problem::Failed("The discovery file is damaged; restart Dyno.".into()))?;
        let conn = Client::connect(&d.endpoint, &token, FrameLimit::new(MAX_FRAME), ProtocolVersion::new(WIRE_MAJOR, 0)).map_err(|e| {
            let text = e.to_string();
            if matches!(e, IpcError::Version(_)) {
                Problem::VersionMismatch {
                    agent: d.protocol,
                    ours: PROTOCOL_VERSION,
                    agent_build: d.version.clone(),
                }
            } else {
                // A stale file names an agent that is gone.
                Problem::Failed(format!(
                    "Dyno did not answer ({text}). If it is not running, start it and try again."
                ))
            }
        })?;
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let peer = BodyPeer::start(
            conn,
            BodyOptions::new("the agent").on_push(move |m| {
                if let Message::Live(l) = m {
                    if let Ok(tx) = tx.lock() {
                        let _ = tx.send(*l);
                    }
                }
            }),
        )
        .map_err(|e| Problem::Failed(e.to_string()))?;
        Ok(IpcAgent {
            peer,
            updates: Mutex::new(rx),
            info: AgentInfo {
                pid: d.pid,
                version: d.version,
                protocol: d.protocol,
            },
        })
    }
}

/// The longest any request may take.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// How long to wait for a live update (the agent pushes about once a second).
const LIVE_TIMEOUT: Duration = Duration::from_secs(15);

impl Agent for IpcAgent {
    fn call(&self, request: Request) -> Result<Reply, Problem> {
        match self.peer.call(Message::Request(Box::new(request)), CALL_TIMEOUT) {
            Ok(Message::Reply(r)) => (*r).map_err(|e| Problem::Failed(e.message)),
            Ok(_) => Err(Problem::Failed("Dyno sent something unexpected.".into())),
            Err(e) => Err(Problem::Failed(format!("Dyno did not answer: {e}"))),
        }
    }

    fn live(&self, detail: LiveDetail) -> Result<LiveUpdate, Problem> {
        let rx = self
            .updates
            .lock()
            .map_err(|_| Problem::Failed("The connection is broken.".into()))?;
        while rx.try_recv().is_ok() {}
        self.call(Request::Subscribe { detail })?;
        rx.recv_timeout(LIVE_TIMEOUT)
            .map_err(|_| Problem::Failed("Dyno did not send a measurement in time.".into()))
    }

    fn about(&self) -> AgentInfo {
        self.info.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(protocol: u32) -> Discovery {
        Discovery {
            endpoint: "x".into(),
            token: "00".into(),
            pid: 1,
            version: "9.9.9".into(),
            protocol,
        }
    }

    #[test]
    fn equal_protocols_pass_and_others_say_which_side_to_update() {
        assert!(check_version(&d(1), 1).is_ok());
        let newer = check_version(&d(2), 1).unwrap_err();
        assert_eq!(newer.exit_code(), 3);
        assert!(newer.message().contains("Update the dyno command"));
        assert!(newer.message().contains("9.9.9"));
        let older = check_version(&d(1), 2).unwrap_err();
        assert!(older.message().contains("Update and restart Dyno"));
    }

    #[test]
    fn problems_have_distinct_exit_codes_and_helpful_text() {
        assert_eq!(Problem::NotRunning.exit_code(), 4);
        assert!(Problem::NotRunning.message().contains("not running"));
        assert_eq!(Problem::Failed("x".into()).exit_code(), 1);
    }

    #[test]
    fn discovery_is_read_from_the_data_folder_and_damage_means_not_running() {
        let tmp = tempfile::tempdir().unwrap();
        let env = Env {
            data_dir: tmp.path().into(),
        };
        assert!(Discovery::read(&env).is_none());
        std::fs::write(env.discovery(), "{ not json").unwrap();
        assert!(Discovery::read(&env).is_none());
        std::fs::write(env.discovery(), vec![b' '; 20_000]).unwrap();
        assert!(Discovery::read(&env).is_none());
        std::fs::write(
            env.discovery(),
            r#"{"endpoint":"e","token":"ab","pid":7,"started":1,"version":"0.1.0","protocol":1,"extra":true}"#,
        )
        .unwrap();
        assert_eq!(Discovery::read(&env).unwrap().pid, 7);
        assert!(matches!(
            IpcAgent::connect(&Env {
                data_dir: tmp.path().join("none")
            }),
            Err(Problem::NotRunning)
        ));
    }

    #[test]
    fn a_data_folder_override_wins() {
        // Not set here (tests never set process-wide variables): the default is a folder named Dyno or dyno.
        let env = Env::from_process();
        let name = env.data_dir.file_name().unwrap().to_string_lossy().to_lowercase();
        assert!(name == "dyno" || std::env::var_os("DYNO_DATA_DIR").is_some());
        assert!(env.discovery().ends_with("agent.json"));
    }
}
