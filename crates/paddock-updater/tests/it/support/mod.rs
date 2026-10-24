//! A fake update server (an in-memory `Transport`: no network), the signed fixtures and a job kit.
#![allow(dead_code)]

use paddock_download::{Config, Downloader, Get, Headers, Response, Transport, TransportError};
use paddock_jobs::{CancelToken, EventSink, JobContext, JobRegistry, JobSnapshot};
use paddock_updater::UpdateKey;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const MANIFEST_URL: &str = "https://updates.test/stable/latest.json";
pub const BETA_URL: &str = "https://updates.test/beta/latest.json";
pub const PACKAGE_URL: &str = "https://updates.test/r/App_9.9.9.tar.gz";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The test key (`tauri signer generate`, no password; the private key is not kept).
pub fn key_text() -> String {
    std::fs::read_to_string(fixture("key.pub")).unwrap()
}

pub fn key() -> UpdateKey {
    UpdateKey::parse(&key_text()).unwrap()
}

/// The signed package (48 KiB of random bytes).
pub fn package() -> Vec<u8> {
    std::fs::read(fixture("update.bin")).unwrap()
}

/// `update.bin.sig`: signed for 9.9.9 with the test key. Others: `other-version.sig` (9.9.8), `no-version.sig`,
/// `other-key.sig` (another key, 9.9.9).
pub fn signature(name: &str) -> String {
    std::fs::read_to_string(fixture(name))
        .unwrap()
        .trim()
        .to_string()
}

/// `latest.json` announcing `version` for this system with `sig` (and the package's size when `sized`).
pub fn manifest(version: &str, sig: &str, sized: bool) -> String {
    let target = paddock_updater::Target::current(None).keys().pop().unwrap();
    let size = if sized {
        format!(", \"size\": {}", package().len())
    } else {
        String::new()
    };
    format!(
        r#"{{"version":"{version}","notes":"Faster.\u0007","pub_date":"2026-10-24T08:00:00Z",
           "platforms":{{"{target}":{{"url":"{PACKAGE_URL}","signature":"{sig}"{size}}}}}}}"#
    )
}

/// What the fake server answers for one address.
#[derive(Clone)]
pub enum Route {
    Body(u16, Vec<u8>),
    Fail(TransportError),
    /// Does not answer until [`FakeServer::release`] (a cancel must not wait for it).
    Hang,
    /// The transport panics (a bug in an app's own transport).
    Panic,
}

#[derive(Default)]
pub struct FakeServer {
    routes: Mutex<HashMap<String, Route>>,
    pub hits: Mutex<Vec<String>>,
    released: (Mutex<bool>, std::sync::Condvar),
}

impl FakeServer {
    pub fn new() -> Arc<FakeServer> {
        Arc::new(FakeServer::default())
    }

    pub fn route(&self, url: &str, route: Route) {
        self.routes.lock().unwrap().insert(url.into(), route);
    }

    pub fn json(&self, url: &str, text: &str) {
        self.route(url, Route::Body(200, text.as_bytes().to_vec()));
    }

    /// Lets every hung request end (with a timeout error).
    pub fn release(&self) {
        *self.released.0.lock().unwrap() = true;
        self.released.1.notify_all();
    }

    pub fn hits(&self, url: &str) -> usize {
        self.hits
            .lock()
            .unwrap()
            .iter()
            .filter(|u| *u == url)
            .count()
    }
}

impl Transport for FakeServer {
    fn get(&self, g: &Get<'_>) -> Result<Response, TransportError> {
        self.hits.lock().unwrap().push(g.url.to_string());
        let route = self.routes.lock().unwrap().get(g.url).cloned();
        match route {
            Some(Route::Body(status, body)) => Ok(Response {
                status,
                headers: Headers {
                    content_length: Some(body.len().to_string()),
                    ..Headers::default()
                },
                body: Box::new(Cursor::new(body)),
            }),
            Some(Route::Fail(e)) => Err(e),
            Some(Route::Panic) => panic!("transport bug"),
            Some(Route::Hang) => {
                let g = self.released.0.lock().unwrap();
                drop(self.released.1.wait_while(g, |r| !*r).unwrap());
                Err(TransportError::new(
                    paddock_download::TransportErrorKind::Timeout,
                    "hung",
                ))
            }
            None => Ok(Response {
                status: 404,
                headers: Headers::default(),
                body: Box::new(Cursor::new(Vec::new())),
            }),
        }
    }
}

/// A downloader over `server` with short waits.
pub fn downloader(server: &Arc<FakeServer>) -> Arc<Downloader> {
    Arc::new(Downloader::new(server.clone()).with_config(Config {
        attempts: 1,
        backoff: Duration::from_millis(1),
        disk_reserve: 0,
        ..Config::default()
    }))
}

/// Every snapshot; cancels the job when a phase with id `cancel_at` begins.
#[derive(Default)]
pub struct Sink {
    pub seen: Mutex<Vec<JobSnapshot>>,
    pub cancel_at: Mutex<Option<(String, CancelToken)>>,
}

impl EventSink for Sink {
    fn emit(&self, s: &JobSnapshot) {
        self.seen.lock().unwrap().push(s.clone());
        if let Some((phase, token)) = self.cancel_at.lock().unwrap().as_ref() {
            let json = serde_json::to_value(s).unwrap();
            if json["phase"]["id"].as_str() == Some(phase.as_str()) {
                token.cancel();
            }
        }
    }
}

impl Sink {
    pub fn phases(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in self.seen.lock().unwrap().iter() {
            let json = serde_json::to_value(s).unwrap();
            if let Some(id) = json["phase"]["id"].as_str()
                && out.last().map(String::as_str) != Some(id)
            {
                out.push(id.to_string());
            }
        }
        out
    }
}

pub fn jobs() -> (Arc<JobRegistry>, Arc<Sink>) {
    let sink = Arc::new(Sink::default());
    (Arc::new(JobRegistry::new(sink.clone())), sink)
}

pub fn job(reg: &JobRegistry) -> JobContext {
    reg.register("test", "test", None)
}
