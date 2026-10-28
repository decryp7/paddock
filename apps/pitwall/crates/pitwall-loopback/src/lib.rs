//! A small HTTP/1.1 client that can only reach this machine (ADR 0001). The address is not a parameter: a
//! [`LoopbackPort`] is a port on 127.0.0.1 and nothing else, so code holding this client cannot open an outbound
//! connection. Pitwall uses it for its engines (llama-server on a private port) and the app uses it for the server's
//! management API.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::Request;
use hyper_util::rt::TokioIo;
use serde_json::Value;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

/// Largest response body [`Client::json`] reads.
pub const MAX_JSON_BYTES: usize = 64 * 1024 * 1024;

/// A port on 127.0.0.1. There is no way to make one for another address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LoopbackPort(u16);

impl LoopbackPort {
    /// A port; 0 is refused.
    pub fn new(port: u16) -> Option<LoopbackPort> {
        (port != 0).then_some(LoopbackPort(port))
    }

    /// The port number.
    pub fn get(self) -> u16 {
        self.0
    }

    /// The socket address (always 127.0.0.1).
    pub fn addr(self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, self.0))
    }
}

/// Why a request failed.
#[derive(Debug, thiserror::Error)]
pub enum LoopbackError {
    /// Nothing listens on the port (or it refused).
    #[error("nothing answered on 127.0.0.1:{0}: {1}")]
    Connect(u16, String),
    /// The request did not finish in time.
    #[error("no answer within {0:?}")]
    Timeout(Duration),
    /// HTTP failed.
    #[error("the local HTTP exchange failed: {0}")]
    Http(String),
    /// The path or a header is not acceptable.
    #[error("invalid request: {0}")]
    Invalid(&'static str),
    /// The body is not JSON or too large.
    #[error("the answer is not usable: {0}")]
    Body(String),
}

/// A response whose body is read as it arrives.
pub struct Response {
    /// The HTTP status.
    pub status: u16,
    /// The `content-type`, if any.
    pub content_type: Option<String>,
    /// All response headers (names lowercase, values as text).
    pub headers: Vec<(String, String)>,
    body: Incoming,
}

impl Response {
    /// The next piece of the body; `None` at the end.
    pub async fn chunk(&mut self) -> Result<Option<Bytes>, LoopbackError> {
        loop {
            match self.body.frame().await {
                None => return Ok(None),
                Some(Err(e)) => return Err(LoopbackError::Http(e.to_string())),
                Some(Ok(f)) => {
                    if let Ok(data) = f.into_data() {
                        return Ok(Some(data));
                    }
                }
            }
        }
    }

    /// The whole body, at most `max` bytes.
    pub async fn bytes(mut self, max: usize) -> Result<Vec<u8>, LoopbackError> {
        let mut out = Vec::new();
        while let Some(c) = self.chunk().await? {
            if out.len() + c.len() > max {
                return Err(LoopbackError::Body(format!("larger than {max} bytes")));
            }
            out.extend_from_slice(&c);
        }
        Ok(out)
    }
}

/// A client of one local server.
#[derive(Debug, Clone)]
pub struct Client {
    port: LoopbackPort,
    bearer: Option<String>,
    connect_timeout: Duration,
}

fn header_ok(v: &str) -> bool {
    !v.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0)
}

impl Client {
    /// A client of `port`, sending `Authorization: Bearer <bearer>` when given.
    pub fn new(port: LoopbackPort, bearer: Option<String>) -> Client {
        Client {
            port,
            bearer,
            connect_timeout: Duration::from_secs(3),
        }
    }

    /// The port.
    pub fn port(&self) -> LoopbackPort {
        self.port
    }

    /// Sends a request; the head must arrive within `timeout` (the body streams afterwards).
    pub async fn send(&self, method: &str, path: &str, body: Option<Vec<u8>>, timeout: Duration) -> Result<Response, LoopbackError> {
        self.send_with(method, path, body, &[], timeout).await
    }

    /// Like [`Client::send`] with extra request headers (a `host` header replaces the default one). Header names and
    /// values with line breaks are refused.
    pub async fn send_with(
        &self,
        method: &str,
        path: &str,
        body: Option<Vec<u8>>,
        headers: &[(&str, &str)],
        timeout: Duration,
    ) -> Result<Response, LoopbackError> {
        self.send_inner(method, path, body, headers, Some(timeout)).await
    }

    /// `timeout: None` waits for the head for as long as the connection lives (a long operation); a server that goes away
    /// closes the connection, which fails the request.
    async fn send_inner(
        &self,
        method: &str,
        path: &str,
        body: Option<Vec<u8>>,
        headers: &[(&str, &str)],
        timeout: Option<Duration>,
    ) -> Result<Response, LoopbackError> {
        if headers
            .iter()
            .any(|(k, v)| !header_ok(k) || !header_ok(v) || k.contains(':') || k.contains(' '))
        {
            return Err(LoopbackError::Invalid("a header holds line breaks, spaces or colons"));
        }
        if !path.starts_with('/') || !header_ok(path) || path.contains(' ') {
            return Err(LoopbackError::Invalid(
                "the path must start with / and hold no spaces or line breaks",
            ));
        }
        if self.bearer.as_deref().is_some_and(|b| !header_ok(b)) {
            return Err(LoopbackError::Invalid("the key holds line breaks"));
        }
        let addr = self.port.addr();
        let stream = tokio::time::timeout(self.connect_timeout, tokio::net::TcpStream::connect(addr))
            .await
            .map_err(|_| LoopbackError::Timeout(self.connect_timeout))?
            .map_err(|e| LoopbackError::Connect(self.port.0, e.to_string()))?;
        let _ = stream.set_nodelay(true);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|e| LoopbackError::Http(e.to_string()))?;
        tokio::spawn(async move {
            // The connection ends with the exchange; its error (if any) surfaces through the response.
            let _ = conn.await;
        });
        let mut req = Request::builder().method(method).uri(path);
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
            req = req.header("host", format!("127.0.0.1:{}", self.port.0));
        }
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        if let Some(b) = &self.bearer {
            req = req.header("authorization", format!("Bearer {b}"));
        }
        let has_body = body.is_some();
        if has_body {
            req = req.header("content-type", "application/json");
        }
        let req = req
            .body(Full::new(Bytes::from(body.unwrap_or_default())))
            .map_err(|e| LoopbackError::Http(e.to_string()))?;
        let resp = match timeout {
            Some(t) => tokio::time::timeout(t, sender.send_request(req))
                .await
                .map_err(|_| LoopbackError::Timeout(t))?,
            None => sender.send_request(req).await,
        }
        .map_err(|e| LoopbackError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        let content_type = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).map(str::to_string);
        let headers = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string())))
            .collect();
        Ok(Response {
            status,
            content_type,
            headers,
            body: resp.into_body(),
        })
    }

    /// [`Client::json`] without a deadline, for a call that runs as long as the work needs (the app's jobs). It ends when
    /// the server answers or the connection closes.
    pub async fn json_no_deadline(&self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, Value), LoopbackError> {
        let bytes = body.map(|b| serde_json::to_vec(b).unwrap_or_default());
        let r = self.send_inner(method, path, bytes, &[], None).await?;
        let status = r.status;
        let raw = r.bytes(MAX_JSON_BYTES).await?;
        Ok((status, parse_json_body(&raw)?))
    }

    /// Sends JSON (or nothing) and reads a JSON answer (an empty body is `null`).
    pub async fn json(&self, method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<(u16, Value), LoopbackError> {
        let bytes = body.map(|b| serde_json::to_vec(b).unwrap_or_default());
        let resp = tokio::time::timeout(timeout, async {
            let r = self.send(method, path, bytes, timeout).await?;
            let status = r.status;
            let raw = r.bytes(MAX_JSON_BYTES).await?;
            Ok::<_, LoopbackError>((status, raw))
        })
        .await
        .map_err(|_| LoopbackError::Timeout(timeout))??;
        Ok((resp.0, parse_json_body(&resp.1)?))
    }
}

fn parse_json_body(raw: &[u8]) -> Result<Value, LoopbackError> {
    if raw.is_empty() {
        Ok(Value::Null)
    } else {
        serde_json::from_slice(raw).map_err(|e| LoopbackError::Body(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A one-shot server that answers with `reply` and returns what it received.
    async fn serve(reply: &'static str) -> (LoopbackPort, tokio::task::JoinHandle<String>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = LoopbackPort::new(l.local_addr().unwrap().port()).unwrap();
        let h = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = s.read(&mut buf).await.unwrap();
            s.write_all(reply.as_bytes()).await.unwrap();
            s.shutdown().await.unwrap();
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        (port, h)
    }

    #[tokio::test]
    async fn json_round_trip_with_key() {
        let (port, h) = serve("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 11\r\n\r\n{\"ok\":true}").await;
        let c = Client::new(port, Some("secret".into()));
        assert_eq!(c.port(), port);
        let (status, v) = c
            .json("POST", "/v1/x", Some(&serde_json::json!({"a": 1})), Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!((status, v["ok"].as_bool()), (200, Some(true)));
        let seen = h.await.unwrap();
        assert!(seen.starts_with("POST /v1/x HTTP/1.1"));
        assert!(seen.contains("authorization: Bearer secret"));
        assert!(seen.contains("host: 127.0.0.1:"));
        assert!(seen.contains("{\"a\":1}"));
    }

    #[tokio::test]
    async fn a_call_without_a_deadline_waits_for_the_answer_and_ends_when_the_server_goes_away() {
        // A server that reads the request, waits for the test's go and then answers.
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = LoopbackPort::new(l.local_addr().unwrap().port()).unwrap();
        let (go, goes) = tokio::sync::oneshot::channel::<()>();
        let (got, got_it) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let _ = s.read(&mut buf).await.unwrap();
            got.send(()).unwrap();
            goes.await.unwrap();
            s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 11\r\n\r\n{\"ok\":true}")
                .await
                .unwrap();
            s.shutdown().await.unwrap();
            // Then one that accepts, reads and dies without answering.
            let (mut s, _) = l.accept().await.unwrap();
            let _ = s.read(&mut buf).await.unwrap();
            drop(s);
        });
        let c = Client::new(port, None);
        let call = tokio::spawn({
            let c = c.clone();
            async move { c.json_no_deadline("POST", "/slow", None).await }
        });
        got_it.await.unwrap();
        assert!(!call.is_finished(), "still waiting: no deadline");
        go.send(()).unwrap();
        let (status, v) = call.await.unwrap().unwrap();
        assert_eq!((status, v["ok"].as_bool()), (200, Some(true)));
        // The server dies mid-call: the call fails by itself (no poll is needed to notice).
        let e = c.json_no_deadline("POST", "/dies", None).await.unwrap_err();
        assert!(matches!(e, LoopbackError::Http(_)), "{e:?}");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn streams_chunks_and_bounds() {
        let (port, _h) = serve("HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\ncontent-type: text/event-stream\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n").await;
        let c = Client::new(port, None);
        let mut r = c.send("GET", "/s", None, Duration::from_secs(5)).await.unwrap();
        assert_eq!(r.content_type.as_deref(), Some("text/event-stream"));
        let mut all = Vec::new();
        while let Some(b) = r.chunk().await.unwrap() {
            all.extend_from_slice(&b);
        }
        assert_eq!(all, b"hello world");
        let (port, _h) = serve("HTTP/1.1 200 OK\r\ncontent-length: 10\r\n\r\n0123456789").await;
        let r = Client::new(port, None)
            .send("GET", "/", None, Duration::from_secs(5))
            .await
            .unwrap();
        assert!(r.bytes(4).await.unwrap_err().to_string().contains("larger than 4"));
        let (port, _h) = serve("HTTP/1.1 204 No Content\r\n\r\n").await;
        let (s, v) = Client::new(port, None)
            .json("DELETE", "/", None, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!((s, v), (204, Value::Null));
        let (port, _h) = serve("HTTP/1.1 200 OK\r\ncontent-length: 3\r\n\r\nnot").await;
        assert!(matches!(
            Client::new(port, None).json("GET", "/", None, Duration::from_secs(5)).await,
            Err(LoopbackError::Body(_))
        ));
    }

    #[tokio::test]
    async fn refuses_bad_input_and_reports_failures() {
        assert!(LoopbackPort::new(0).is_none());
        let p = LoopbackPort::new(1).unwrap();
        assert_eq!(p.addr().ip().to_string(), "127.0.0.1");
        assert_eq!(p.get(), 1);
        let c = Client::new(p, None);
        for bad in ["no-slash", "/a b", "/a\r\nx"] {
            assert!(
                matches!(
                    c.send("GET", bad, None, Duration::from_secs(1)).await,
                    Err(LoopbackError::Invalid(_))
                ),
                "{bad}"
            );
        }
        let evil = Client::new(p, Some("a\nb".into()));
        assert!(matches!(
            evil.send("GET", "/", None, Duration::from_secs(1)).await,
            Err(LoopbackError::Invalid(_))
        ));
        // A free port: nothing listens.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let free = LoopbackPort::new(l.local_addr().unwrap().port()).unwrap();
        drop(l);
        let e = Client::new(free, None)
            .send("GET", "/", None, Duration::from_secs(1))
            .await
            .err()
            .unwrap();
        assert!(matches!(e, LoopbackError::Connect(..)), "{e}");
        // A server that accepts but never answers: timeout.
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = LoopbackPort::new(l.local_addr().unwrap().port()).unwrap();
        let _keep = tokio::spawn(async move {
            let (_s, _) = l.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let e = Client::new(port, None)
            .json("GET", "/", None, Duration::from_millis(200))
            .await
            .unwrap_err();
        assert!(matches!(e, LoopbackError::Timeout(_)), "{e}");
    }
}
