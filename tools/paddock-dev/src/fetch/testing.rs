//! Test support for `dev fetch`: a fake network (no sockets, ER-14), archives built in memory and a runner over them.

use super::runner::{LineSink, Runner};
use paddock_download::{Get, Headers, Response, Transport, TransportError, TransportErrorKind};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::sync::{Arc, Mutex};

type Hook = Arc<dyn Fn() + Send + Sync>;

/// One answer of the fake network.
#[derive(Clone)]
pub enum Route {
    /// 200 with this body.
    Body(Vec<u8>),
    /// This status and no body.
    Status(u16),
    /// 200 with a long body of zeros whose first read calls the hook (to cancel while bytes are flowing).
    Endless(Hook),
}

/// A network that answers from a table and records the URLs it was asked for.
#[derive(Default)]
pub struct FakeNet {
    routes: Mutex<HashMap<String, Route>>,
    seen: Mutex<Vec<String>>,
    headers: Mutex<Vec<(String, String, String)>>,
}

impl FakeNet {
    pub fn new() -> Arc<FakeNet> {
        Arc::new(FakeNet::default())
    }

    pub fn serve(&self, url: &str, body: impl Into<Vec<u8>>) -> &FakeNet {
        self.route(url, Route::Body(body.into()))
    }

    pub fn route(&self, url: &str, route: Route) -> &FakeNet {
        self.routes.lock().unwrap().insert(url.to_string(), route);
        self
    }

    /// Every URL asked for, in order (a download probes first, so it may appear twice).
    pub fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    /// The `(name, value)` request headers sent to `url` (names lower-case), over all requests.
    pub fn headers_to(&self, url: &str) -> Vec<(String, String)> {
        self.headers
            .lock()
            .unwrap()
            .iter()
            .filter(|(u, _, _)| u == url)
            .map(|(_, n, v)| (n.clone(), v.clone()))
            .collect()
    }

    /// How many requests went to `url`.
    pub fn asked(&self, url: &str) -> usize {
        self.seen().iter().filter(|u| *u == url).count()
    }
}

struct FirstRead<R> {
    inner: R,
    hook: Option<Hook>,
}

impl<R: Read> Read for FirstRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if let Some(h) = self.hook.take() {
            h();
        }
        self.inner.read(buf)
    }
}

fn response(status: u16, length: Option<usize>, body: impl Read + Send + 'static) -> Response {
    Response {
        status,
        headers: Headers {
            content_length: length.map(|n| n.to_string()),
            ..Headers::default()
        },
        body: Box::new(body),
    }
}

impl Transport for FakeNet {
    fn get(&self, request: &Get<'_>) -> Result<Response, TransportError> {
        self.seen.lock().unwrap().push(request.url.to_string());
        for h in request.headers {
            self.headers
                .lock()
                .unwrap()
                .push((request.url.to_string(), h.name.to_ascii_lowercase(), h.value.clone()));
        }
        let route = self.routes.lock().unwrap().get(request.url).cloned();
        match route {
            Some(Route::Body(b)) => Ok(response(200, Some(b.len()), Cursor::new(b))),
            Some(Route::Status(s)) => Ok(response(s, Some(0), Cursor::new(Vec::new()))),
            Some(Route::Endless(hook)) => Ok(response(
                200,
                None,
                FirstRead {
                    inner: std::io::repeat(0).take(512 << 20),
                    hook: Some(hook),
                },
            )),
            None => Err(TransportError::new(TransportErrorKind::Invalid, "no route")),
        }
    }
}

/// A runner over `net` whose progress lines are discarded.
pub fn runner(net: &Arc<FakeNet>, jobs: usize) -> Runner {
    Runner::new(net.clone(), Arc::new(LineSink::new(std::io::sink(), false)), jobs)
}

/// A runner over `net` that also hands back what the progress sink wrote.
pub fn runner_with_lines(net: &Arc<FakeNet>, jobs: usize) -> (Runner, Arc<Mutex<Vec<u8>>>) {
    #[derive(Clone)]
    struct Shared(Arc<Mutex<Vec<u8>>>);
    impl Write for Shared {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sink = LineSink::new(Shared(buf.clone()), true);
    (Runner::new(net.clone(), Arc::new(sink), jobs), buf)
}

/// Lower-case hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// A zip archive of `(name, bytes, unix mode)` entries.
pub fn zip_of(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes, mode) in entries {
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(*mode);
        w.start_file(*name, opts).unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// A zip of plain files `(name, bytes)` and relative symbolic links `(name, target)`, as a macOS app bundle has.
pub fn zip_with_links(files: &[(&str, &[u8])], links: &[(&str, &str)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
    for (name, bytes) in files {
        w.start_file(*name, opts).unwrap();
        w.write_all(bytes).unwrap();
    }
    for (name, target) in links {
        w.add_symlink(*name, *target, opts).unwrap();
    }
    w.finish().unwrap().into_inner()
}
