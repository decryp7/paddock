//! Requests for the right-click web search (WP-09), the hash search (WP-33) and the startup filter (WP-34). All are queries:
//! the agent builds the text or address, the shell opens a browser on the person's click, and nothing is fetched or
//! uploaded by Dyno.

use crate::agent::{lock, Shared};
use dyno_core::protocol::{AgentError, ErrorCode, Reply, Request};
use dyno_core::weblinks::{search_seed, search_url, virustotal_url, HashSource};
use std::sync::Arc;

fn bad(msg: impl Into<String>) -> AgentError {
    AgentError::new(ErrorCode::BadRequest, msg)
}

/// The most characters of text a search seed is built from (it is cut to 200 anyway).
const MAX_SEED_INPUT: usize = 4096;

/// Answers a search, hash or startup-filter request.
pub fn handle(shared: &Arc<Shared>, request: Request) -> Result<Reply, AgentError> {
    match request {
        Request::SearchSeed { text } => {
            let input: String = text.chars().take(MAX_SEED_INPUT).collect();
            let redactor = crate::diagnostics::local_identity(sysinfo::System::host_name());
            Ok(Reply::SearchSeed(search_seed(&input, &redactor)))
        }
        Request::WebSearch { query } => search_url(&query).map(Reply::Url).map_err(|e| bad(e.to_string())),
        Request::VirusTotalUrl { sha256 } => virustotal_url(&sha256).map(Reply::Url).map_err(|e| bad(e.to_string())),
        Request::FileHash { source } => {
            let path = match source {
                HashSource::Path(p) => p,
                HashSource::Process(target) => {
                    let found = lock(&shared.core)
                        .latest
                        .processes
                        .iter()
                        .find(|p| p.reference() == target)
                        .map(|p| p.exe.clone());
                    match found {
                        None => return Err(AgentError::new(ErrorCode::NotFound, "That process has exited.")),
                        Some(None) => {
                            return Err(AgentError::new(
                                ErrorCode::AccessDenied,
                                "The system does not tell Dyno which file this process runs from.",
                            ))
                        }
                        Some(Some(exe)) => exe,
                    }
                }
            };
            shared
                .hasher
                .hash(&path)
                .map(Reply::FileHash)
                .map_err(|m| AgentError::new(ErrorCode::NotFound, m))
        }
        Request::StartupOverview { filter } => {
            let list = lock(&shared.ports.startup).list()?;
            Ok(Reply::StartupOverview(Box::new(dyno_core::startup::overview(
                list,
                filter,
                crate::unusual::current_os(),
            ))))
        }
        other => Err(bad(format!("{other:?} is not a search request."))),
    }
}
