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

/// The path a hash job reads: the one the person gave, or the program a running process was started from.
pub fn hash_path(shared: &Arc<Shared>, source: HashSource) -> Result<String, AgentError> {
    match source {
        HashSource::Path(p) => Ok(p),
        HashSource::Process(target) => {
            let found = lock(&shared.core)
                .latest
                .processes
                .iter()
                .find(|p| p.reference() == target)
                .map(|p| p.exe.clone());
            match found {
                None => Err(AgentError::new(ErrorCode::NotFound, "That process has exited.")),
                Some(None) => Err(AgentError::new(
                    ErrorCode::AccessDenied,
                    "The system does not tell Dyno which file this process runs from.",
                )),
                Some(Some(exe)) => Ok(exe),
            }
        }
    }
}

/// The shared redactor as the search seed wants it: the text with private details replaced and what was removed, by kind.
fn seed_redact(redactor: &paddock_diag::Redactor) -> impl Fn(&str) -> (String, dyno_core::weblinks::Removed) + '_ {
    move |text| {
        let (out, c) = redactor.text(text);
        (
            out,
            dyno_core::weblinks::Removed {
                home_paths: c.home_paths + c.profile_paths,
                user_names: c.user_names,
                machine_names: c.machine_names,
                emails: c.emails,
                ip_addresses: c.ip_addresses,
            },
        )
    }
}

/// Answers a search, hash or startup-filter request.
pub fn handle(shared: &Arc<Shared>, request: Request) -> Result<Reply, AgentError> {
    match request {
        Request::SearchSeed { text } => {
            let input: String = text.chars().take(MAX_SEED_INPUT).collect();
            let host = sysinfo::System::host_name().ok();
            let redactor = paddock_diag::Redactor::for_machine_named(&paddock_env::ProcessVars, host.as_deref());
            let redact = seed_redact(&redactor);
            Ok(Reply::SearchSeed(search_seed(&input, &redact)))
        }
        Request::WebSearch { query } => search_url(&query).map(Reply::Url).map_err(|e| bad(e.to_string())),
        Request::VirusTotalUrl { sha256 } => virustotal_url(&sha256).map(Reply::Url).map_err(|e| bad(e.to_string())),
        Request::StartupOverview { filter } => {
            // The list and what it could not read come from one lock, so they describe the same read.
            let (list, unavailable, btm_via) = {
                let mut startup = lock(&shared.ports.startup);
                let list = startup.list()?;
                (list, startup.unavailable(), startup.btm_via())
            };
            let mut overview = dyno_core::startup::overview(list, filter, crate::unusual::current_os()).with_btm_via(btm_via);
            overview.unavailable = unavailable;
            Ok(Reply::StartupOverview(Box::new(overview)))
        }
        other => Err(bad(format!("{other:?} is not a search request."))),
    }
}

#[cfg(test)]
mod seed_tests {
    use super::*;

    #[test]
    fn a_seed_loses_someone_elses_profile_and_the_machine_name_with_the_shared_redactor() {
        let r = paddock_diag::Redactor::for_machine_named(&paddock_env::MapVars::new(), Some("Dana-PC.local"));
        let s = search_seed("crash in C:\\Users\\Dana Lee\\AppData\\x.exe on Dana-PC", &seed_redact(&r));
        assert!(!s.text.contains("Dana") && !s.text.contains("Lee"), "{}", s.text);
        assert_eq!(s.removed, vec!["1 home folder", "1 computer name"]);
    }
}
