// ci-runs-reader: reads one run by id only to find its own cancel_url; it gives no pass/fail verdict (`ci cancel`)
//! `ci cancel <run-id>`: stops one CI run by the handle `./dev ci dispatch` printed (`run <id>`), never "the newest run" (ER-51).
//!
//! The run is read (`GET .../actions/runs/<id>`), its own `cancel_url` is checked (same server, `.../actions/runs/<id>/cancel`, the id
//! asked for: foreign text is data, ER-54) and only that URL is posted to. A run that already finished gets no post. A cancelled
//! run is not a pass and not a failure: every reader of runs ([`super::verdict`]) says "not measured" for it (ER-33), and this command
//! says it too. The read-only triage never cancels ([`super::triage`] asserts it); cancelling is this command alone, so the train flow
//! uses it when a slow local row fails while CI runs beside it (ER-53).

use crate::http::Http;
use serde_json::Value;

/// What `ci cancel` did.
#[derive(Debug, Clone, PartialEq)]
pub enum Cancelled {
    /// The cancel request was sent for the run.
    Sent {
        /// The run.
        run: u64,
    },
    /// The run had already finished (`status` says): nothing was sent.
    AlreadyFinished {
        /// The run.
        run: u64,
        /// Its status.
        status: String,
    },
}

impl Cancelled {
    /// The one line the command prints. A cancelled run proves nothing.
    pub fn line(&self) -> String {
        match self {
            Cancelled::Sent { run } => {
                format!("run {run}: cancel sent; the run reads as \"cancelled: not measured\" (it proves nothing, ER-33)")
            }
            Cancelled::AlreadyFinished { run, status } => format!("run {run}: already finished ({status}); nothing was cancelled"),
        }
    }
}

/// The `cancel_url` of the run, checked against the run asked for.
fn own_cancel_url(run: &Value, api: &str, id: u64) -> Result<String, String> {
    let got = run["id"].as_u64().ok_or("the run has no id")?;
    if got != id {
        return Err(format!("the server answered run {got} for run {id}: nothing was sent"));
    }
    let url = run["cancel_url"].as_str().ok_or("the run has no cancel_url")?;
    let want = format!("{api}/runs/{id}/cancel");
    if url != want {
        return Err(format!("the run's cancel_url ({url}) is not {want}: nothing was sent"));
    }
    Ok(url.to_string())
}

/// Cancels run `id` of `repo` on `server`: one read of that run and, when it still runs, one post to its own cancel URL.
pub fn cancel_run(http: &mut dyn Http, server: &str, repo: &str, token: &str, id: u64) -> Result<Cancelled, String> {
    let api = format!("{}/api/v1/repos/{repo}/actions", server.trim_end_matches('/'));
    let run = http.get_json(&format!("{api}/runs/{id}"), token)?;
    let status = run["status"].as_str().unwrap_or("").to_string();
    let url = own_cancel_url(&run, &api, id)?;
    if matches!(status.as_str(), "completed" | "success" | "failure" | "cancelled" | "skipped") {
        return Ok(Cancelled::AlreadyFinished { run: id, status });
    }
    http.post_json(&url, token, &serde_json::json!({}))?;
    Ok(Cancelled::Sent { run: id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const API: &str = "https://gitea.invalid/api/v1/repos/example/paddock/actions";

    #[derive(Default)]
    struct Stub {
        run: Value,
        gets: Vec<String>,
        posts: Vec<String>,
    }

    impl Http for Stub {
        fn get_json(&mut self, url: &str, _token: &str) -> Result<Value, String> {
            self.gets.push(url.to_string());
            Ok(self.run.clone())
        }
        fn post_json(&mut self, url: &str, _token: &str, _body: &Value) -> Result<Value, String> {
            self.posts.push(url.to_string());
            Ok(Value::Null)
        }
    }

    fn run(id: u64, status: &str, cancel_url: &str) -> Value {
        json!({"id": id, "status": status, "cancel_url": cancel_url})
    }

    /// Only the handle's own run is read and only its own cancel URL is posted to: no run list, never "the newest".
    #[test]
    fn it_posts_to_the_cancel_url_of_that_run_only() {
        let mut http = Stub {
            run: run(9071, "in_progress", &format!("{API}/runs/9071/cancel")),
            ..Stub::default()
        };
        let got = cancel_run(&mut http, "https://gitea.invalid", "example/paddock", "t", 9071).expect("sent");
        assert_eq!(got, Cancelled::Sent { run: 9071 });
        assert_eq!(http.gets, [format!("{API}/runs/9071")], "one read, of that run");
        assert_eq!(http.posts, [format!("{API}/runs/9071/cancel")], "one post, to its own cancel_url");
        assert!(got.line().contains("cancelled: not measured"), "{}", got.line());
    }

    /// Foreign text is data (ER-54): a cancel_url for another run, or another host, is refused and nothing is posted.
    #[test]
    fn a_cancel_url_that_is_not_the_runs_own_is_refused_and_nothing_is_sent() {
        for bad in [
            format!("{API}/runs/9072/cancel"),
            "https://evil.invalid/api/v1/repos/example/paddock/actions/runs/9071/cancel".to_string(),
            format!("{API}/runs/9071/delete"),
        ] {
            let mut http = Stub {
                run: run(9071, "queued", &bad),
                ..Stub::default()
            };
            let e = cancel_run(&mut http, "https://gitea.invalid", "example/paddock", "t", 9071).expect_err(&bad);
            assert!(e.contains("nothing was sent"), "{e}");
            assert!(http.posts.is_empty(), "{bad}");
        }
        let mut http = Stub {
            run: run(5, "queued", &format!("{API}/runs/9071/cancel")),
            ..Stub::default()
        };
        assert!(cancel_run(&mut http, "https://gitea.invalid", "example/paddock", "t", 9071).is_err());
        assert!(http.posts.is_empty(), "a server answer for another run id");
    }

    #[test]
    fn a_finished_run_gets_no_post() {
        for status in ["completed", "success", "failure", "cancelled"] {
            let mut http = Stub {
                run: run(9071, status, &format!("{API}/runs/9071/cancel")),
                ..Stub::default()
            };
            let got = cancel_run(&mut http, "https://gitea.invalid", "example/paddock", "t", 9071).expect("read");
            assert!(matches!(got, Cancelled::AlreadyFinished { .. }), "{status}");
            assert!(http.posts.is_empty(), "{status}");
        }
    }

    /// The read-only triage and the cancel command are apart: nothing in the triage module posts (its own test asserts the URLs it asks).
    #[test]
    fn a_run_that_cannot_be_read_is_an_error_and_nothing_is_posted() {
        struct Down;
        impl Http for Down {
            fn get_json(&mut self, _: &str, _: &str) -> Result<Value, String> {
                Err("HTTP 502".into())
            }
        }
        assert_eq!(
            cancel_run(&mut Down, "https://gitea.invalid", "example/paddock", "t", 1),
            Err("HTTP 502".into())
        );
    }
}
