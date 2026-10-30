//! HTTPS for release discovery and checksum files (ADR 0002): HTTPS only, timeouts, size caps, the OS certificate store.
//! Downloads of runner binaries are `paddock-download`'s (segmented, resumable, hashed while writing; `work::download`).

use std::io::Read;
use std::time::Duration;

/// Largest runner binary Pitcrew downloads.
pub const MAX_BINARY: u64 = 256 * 1024 * 1024;

/// An HTTP client. Only `https://` URLs are fetched, except loopback `http://` when built for tests.
#[derive(Clone)]
pub struct Http {
    agent: ureq::Agent,
    allow_loopback_http: bool,
}

impl Http {
    /// The client the service uses.
    pub fn new() -> Self {
        Self::build(false)
    }

    /// A client that also fetches `http://127.0.0.1` (tests with a local server).
    pub fn for_tests() -> Self {
        Self::build(true)
    }

    fn build(allow_loopback_http: bool) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_global(Some(Duration::from_secs(15 * 60)))
            .timeout_recv_body(Some(Duration::from_secs(120)))
            .user_agent(concat!("Pitcrew/", env!("CARGO_PKG_VERSION")))
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .http_status_as_error(false)
            .build();
        Http {
            agent: config.into(),
            allow_loopback_http,
        }
    }

    /// Whether `url` may be fetched: HTTPS only (loopback `http://` in tests).
    pub fn check(&self, url: &str) -> Result<(), String> {
        if url.starts_with("https://") || (self.allow_loopback_http && url.starts_with("http://127.0.0.1:")) {
            Ok(())
        } else {
            Err(format!("{url} is not an HTTPS address"))
        }
    }

    fn get(&self, url: &str, bearer: Option<&str>) -> Result<ureq::http::Response<ureq::Body>, String> {
        self.check(url)?;
        let mut req = self.agent.get(url);
        if let Some(t) = bearer {
            req = req.header("Authorization", &format!("token {t}"));
        }
        let resp = req.call().map_err(|e| format!("{} could not be reached: {e}", host(url)))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(format!("{} answered HTTP {status} for {}", host(url), path_of(url)));
        }
        Ok(resp)
    }

    /// The body of `url` as text, at most `max` bytes (more is an error).
    pub fn text(&self, url: &str, max: usize, bearer: Option<&str>) -> Result<String, String> {
        let resp = self.get(url, bearer)?;
        let mut buf = Vec::new();
        resp.into_body()
            .into_reader()
            .take(max as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| format!("reading from {} failed: {e}", host(url)))?;
        if buf.len() > max {
            return Err(format!("{} sent more than {max} bytes", host(url)));
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

fn host(url: &str) -> &str {
    url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url)
}

fn path_of(url: &str) -> &str {
    url.split("://").nth(1).and_then(|r| r.find('/').map(|i| &r[i..])).unwrap_or("/")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    /// A tiny HTTP server for tests: `routes` maps a path to (status, body). Returns the base URL.
    pub(crate) fn serve(routes: Vec<(String, u16, Vec<u8>)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let routes = routes.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut first = String::new();
                    let _ = reader.read_line(&mut first);
                    loop {
                        let mut l = String::new();
                        if reader.read_line(&mut l).unwrap_or(0) == 0 || l == "\r\n" {
                            break;
                        }
                    }
                    let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let (status, body) = routes
                        .iter()
                        .find(|(p, _, _)| *p == path)
                        .map(|(_, s, b)| (*s, b.clone()))
                        .unwrap_or((404, b"not found".to_vec()));
                    let mut s = stream;
                    let _ = write!(
                        s,
                        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = s.write_all(&body);
                });
            }
        });
        base
    }

    #[test]
    fn reads_text_within_limits_and_refuses_plain_http() {
        let base = serve(vec![
            ("/v.json".into(), 200, br#"{"latest":{"version":"5.2.0"}}"#.to_vec()),
            ("/gone".into(), 500, vec![]),
        ]);
        let http = Http::for_tests();
        assert!(http.text(&format!("{base}/v.json"), 1024, Some("t")).unwrap().contains("5.2.0"));
        assert!(http.text(&format!("{base}/v.json"), 5, None).unwrap_err().contains("more than"));
        assert!(http.text(&format!("{base}/gone"), 5, None).unwrap_err().contains("HTTP 500"));
        assert!(Http::new()
            .text("http://example.com", 10, None)
            .unwrap_err()
            .contains("not an HTTPS"));
        assert_eq!(host("https://a.b/c"), "a.b");
        assert_eq!(path_of("https://a.b"), "/");
    }
}
