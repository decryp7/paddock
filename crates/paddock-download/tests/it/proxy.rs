//! Proxy support over the real transport against an in-process CONNECT proxy and server (loopback only): an explicit proxy, a
//! login that is sent and never shown, `NO_PROXY` matching, and the environment's variables.
#![cfg(feature = "http")]

use crate::support;

use paddock_download::{
    Config, DownloadError, Downloader, Fetch, HttpConfig, HttpTransport, Integrity, ProxyMode,
    ProxyRules, ProxyServer, Request, Sha256Digest,
};
use std::time::Duration;
use support::{ConnectProxy, Server, blob, job, partial, sha256_hex};

fn fast() -> Config {
    Config {
        attempts: 2,
        backoff: Duration::from_millis(5),
        ..Config::default()
    }
}

fn downloader(rules: ProxyRules) -> Downloader {
    Downloader::http(HttpConfig {
        proxy: ProxyMode::Rules(rules),
        ..HttpConfig::default()
    })
    .unwrap()
    .with_config(fast())
}

fn file_server(data: &[u8]) -> Server {
    let d = data.to_vec();
    Server::start(move |r, _| partial(r, &d))
}

fn request(server: &Server, dir: &tempfile::TempDir, data: &[u8]) -> Request {
    Request::new(
        &server.url("/f.bin"),
        dir.path().join("f.bin"),
        Integrity::Sha256(Sha256Digest::from_hex(&sha256_hex(data)).unwrap()),
    )
    .unwrap()
}

#[test]
fn an_explicit_proxy_carries_the_download_through_a_connect_tunnel() {
    let data = blob(300 * 1024);
    let (server, proxy) = (file_server(&data), ConnectProxy::start(None));
    let dir = tempfile::tempdir().unwrap();
    let d = downloader(ProxyRules::all(ProxyServer::parse(&proxy.url()).unwrap()));
    let (ctx, _) = job();
    let got = d.download(&ctx, &request(&server, &dir, &data)).unwrap();
    assert_eq!(std::fs::read(got.path).unwrap(), data);
    let tunnels = proxy.log();
    assert!(!tunnels.is_empty());
    assert!(
        tunnels
            .iter()
            .all(|t| t.target == server.host() && t.authorization.is_none()),
        "{tunnels:?}"
    );
}

#[test]
fn an_authenticated_proxy_gets_basic_credentials_that_never_show_in_output() {
    let data = blob(64 * 1024);
    let (server, proxy) = (
        file_server(&data),
        ConnectProxy::start(Some(("al@ice", "p:ss/w0rd"))),
    );
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    // Through the credentials API (special characters are encoded for the tunnel request, not mangled).
    let server_rules = ProxyRules::all(
        ProxyServer::parse(&proxy.url())
            .unwrap()
            .with_credentials("al@ice", "p:ss/w0rd"),
    );
    let d = downloader(server_rules);
    d.download(&ctx, &request(&server, &dir, &data)).unwrap();
    let t = proxy.log();
    assert!(!t[0].refused);
    // base64("al@ice:p:ss/w0rd")
    assert_eq!(
        t[0].authorization.as_deref(),
        Some("Basic YWxAaWNlOnA6c3MvdzByZA==")
    );

    // The same login inside the address (percent-encoded).
    let url = format!("http://al%40ice:p%3Ass%2Fw0rd@127.0.0.1:{}", proxy.port);
    let dir2 = tempfile::tempdir().unwrap();
    let d = Downloader::http(HttpConfig {
        proxy: ProxyMode::explicit(&url).unwrap(),
        ..HttpConfig::default()
    })
    .unwrap()
    .with_config(fast());
    d.download(&ctx, &request(&server, &dir2, &data)).unwrap();

    // Without it the proxy answers 407: a proxy error, not retried, and nothing prints the password.
    let wrong = downloader(ProxyRules::all(
        ProxyServer::parse(&proxy.url())
            .unwrap()
            .with_credentials("al@ice", "hunter2"),
    ));
    let dir3 = tempfile::tempdir().unwrap();
    let e = wrong
        .download(&ctx, &request(&server, &dir3, &data))
        .unwrap_err();
    let DownloadError::Network { detail, .. } = &e else {
        panic!("{e:?}")
    };
    assert!(detail.contains("proxy failed"), "{detail}");
    assert!(!format!("{e} {e:?}").contains("hunter2"));
    assert_eq!(
        proxy.log().iter().filter(|t| t.refused).count(),
        1,
        "a 407 is not retried"
    );
    let rules = ProxyRules::all(
        ProxyServer::parse(&proxy.url())
            .unwrap()
            .with_credentials("u", "hunter2"),
    );
    assert!(
        !format!(
            "{rules:?} {:?}",
            HttpConfig {
                proxy: ProxyMode::Rules(rules.clone()),
                ..HttpConfig::default()
            }
        )
        .contains("hunter2")
    );
}

#[test]
fn hosts_on_the_bypass_list_connect_directly() {
    let data = blob(16 * 1024);
    let (server, proxy) = (file_server(&data), ConnectProxy::start(None));
    let via = ProxyRules::all(ProxyServer::parse(&proxy.url()).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let (ctx, _) = job();
    // 127.0.0.0/8 matches the server: no tunnel is opened.
    downloader(via.clone().with_no_proxy("example.test, 127.0.0.0/8"))
        .download(&ctx, &request(&server, &dir, &data))
        .unwrap();
    assert!(proxy.log().is_empty(), "{:?}", proxy.log());
    // A list that does not match it still uses the proxy.
    let dir = tempfile::tempdir().unwrap();
    downloader(via.with_no_proxy("example.test,10.0.0.0/8"))
        .download(&ctx, &request(&server, &dir, &data))
        .unwrap();
    assert!(!proxy.log().is_empty());
}

#[test]
fn the_environments_variables_choose_the_proxy_and_no_proxy_wins() {
    let data = blob(16 * 1024);
    let (server, proxy) = (file_server(&data), ConnectProxy::start(None));
    let url = proxy.url();
    let host = server.host();
    let env = move |extra_no_proxy: Option<&'static str>| {
        let url = url.clone();
        move |n: &str| match n {
            "HTTP_PROXY" => Some(url.clone()),
            "no_proxy" => extra_no_proxy.map(str::to_string),
            _ => None,
        }
    };
    let (ctx, _) = job();
    let dir = tempfile::tempdir().unwrap();
    let rules = ProxyRules::from_env_with(&env(None)).unwrap();
    downloader(rules)
        .download(&ctx, &request(&server, &dir, &data))
        .unwrap();
    assert_eq!(proxy.log().last().unwrap().target, host);
    let before = proxy.log().len();
    let dir = tempfile::tempdir().unwrap();
    let rules = ProxyRules::from_env_with(&env(Some("127.0.0.1"))).unwrap();
    downloader(rules)
        .download(&ctx, &request(&server, &dir, &data))
        .unwrap();
    assert_eq!(proxy.log().len(), before, "NO_PROXY sent it direct");
}

#[test]
fn direct_mode_ignores_proxies_and_reload_changes_nothing_for_fixed_modes() {
    let t = HttpTransport::new(HttpConfig {
        proxy: ProxyMode::Direct,
        ..HttpConfig::default()
    })
    .unwrap();
    assert!(t.proxy_rules().is_empty());
    t.reload_proxy().unwrap();
    assert!(t.proxy_rules().is_empty());
    // A system-mode transport builds and keeps loopback direct, whatever this machine is set up with.
    let sys = HttpTransport::new(HttpConfig::default()).unwrap();
    sys.reload_proxy().unwrap();
    assert!(sys.proxy_rules().for_url("http://127.0.0.1:1/").is_none());
}

#[test]
fn proxy_rules_per_scheme_and_the_no_proxy_forms_that_are_rare_or_malformed() {
    let http = ProxyServer::parse("http://proxy.example:3128").unwrap();
    let rules = ProxyRules::per_scheme(Some(http), None);
    assert!(
        rules.for_url("http://a.example/").is_some()
            && rules.for_url("https://a.example/").is_none()
    );
    assert!(rules.no_proxy().is_empty());
    // IPv6 literals with and without a port, short IPv4 forms, CIDR of both families and malformed entries.
    let list = "[::1]:8080, fd00::/8, 169.254/16, 10.0.0.0/8, 10.1.0.0/40, fe80::/200, [bad, :80, .., *., a*b.example, 300.1.1.1/8";
    let no = paddock_download::NoProxy::parse(list);
    assert!(no.matches("::1", 8080) && !no.matches("::1", 80));
    assert!(no.matches("fd00::5", 1) && no.matches("169.254.9.9", 1) && no.matches("10.9.9.9", 1));
    assert!(no.matches("axb.example", 1));
    // Mixed families never match each other's networks, and an all-zero prefix matches everything of its family.
    assert!(!no.matches("203.0.113.5", 1));
    let all_v4 = paddock_download::NoProxy::parse("0.0.0.0/0");
    assert!(all_v4.matches("203.0.113.5", 1) && !all_v4.matches("::1", 1));
    assert!(paddock_download::NoProxy::parse("::/0").matches("2001:db8::1", 1));
    // Host and port forms of a proxy server: IPv6 with a port, junk after a bracket, too many colons.
    assert!(ProxyServer::parse("http://[::1]:3128").is_ok());
    for bad in [
        "http://[::1]x",
        "http://[::1]:abc",
        "http://a:b:c",
        "http://a:99999",
    ] {
        assert!(ProxyServer::parse(bad).is_err(), "{bad}");
    }
    let shown = format!(
        "{:?}",
        ProxyServer::parse("http://u:secret@proxy.example:3128").unwrap()
    );
    assert!(shown.contains("password hidden") && !shown.contains("secret"));
    // The process environment is read by `from_env` (the result depends on the machine; only that it answers).
    let _ = ProxyRules::from_env();
}

#[test]
fn an_https_proxy_that_refuses_connections_is_a_network_error_naming_the_target_host() {
    // Nothing listens on the discard port of the loopback: the proxy is the part that fails, and it is not retried forever.
    let proxy = ProxyServer::parse("https://127.0.0.1:9").unwrap();
    let e = downloader(ProxyRules::all(proxy))
        .get_bytes("http://files.example.test/a.bin", &Fetch::new())
        .unwrap_err();
    assert!(
        matches!(&e, DownloadError::Network { host, .. } if host == "files.example.test"),
        "{e:?}"
    );
}
