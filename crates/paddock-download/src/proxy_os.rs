//! The operating system's proxy settings, read by a short-lived system tool (`scutil --proxy` on macOS, `reg query` and
//! `netsh winhttp show proxy` on Windows) and parsed by pure functions that tests feed with sample output. A tool that is missing,
//! fails or takes longer than [`TOOL_TIMEOUT`] means "no system proxy". Passwords are not part of these settings.

// The parsers compile and are tested on every platform; only one platform's are called.
#![allow(dead_code)]

use crate::proxy::{NoProxy, ProxyRules, ProxyScheme, ProxyServer};
use std::time::Duration;

/// Longest a settings tool may run.
pub(crate) const TOOL_TIMEOUT: Duration = Duration::from_secs(3);

/// The system's rules on this platform (empty where there is no system store).
pub(crate) fn system_rules() -> ProxyRules {
    #[cfg(target_os = "macos")]
    {
        run("/usr/sbin/scutil", &["--proxy"])
            .map(|t| parse_scutil(&t))
            .unwrap_or_default()
    }
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").ok();
        let reg = windows_tool_path("reg.exe", root.as_deref());
        let netsh = windows_tool_path("netsh.exe", root.as_deref());
        let ie = run(
            &reg,
            &[
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            ],
        )
        .map(|t| parse_windows_registry(&t))
        .unwrap_or_default();
        if ie.is_empty() {
            run(&netsh, &["winhttp", "show", "proxy"])
                .map(|t| parse_winhttp(&t))
                .unwrap_or_default()
        } else {
            ie
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        ProxyRules::none()
    }
}

/// Runs a tool and returns its output when it ends successfully within [`TOOL_TIMEOUT`] (killed otherwise).
#[cfg(any(target_os = "macos", windows))]
fn run(program: &str, args: &[&str]) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.by_ref().take(256 * 1024).read_to_end(&mut buf);
        buf
    });
    let until = Instant::now() + TOOL_TIMEOUT;
    let ok = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let bytes = reader.join().ok()?;
    ok.then(|| String::from_utf8_lossy(&bytes).into_owned())
}

/// `Key : Value` lines of `scutil --proxy`. `HTTPEnable`/`HTTPProxy`/`HTTPPort` and the `HTTPS` trio give the proxies,
/// `ExceptionsList` the bypass entries and `ExcludeSimpleHostnames` the `<local>` rule.
pub(crate) fn parse_scutil(text: &str) -> ProxyRules {
    let mut map = std::collections::HashMap::new();
    let mut exceptions = String::new();
    let mut in_list = false;
    for line in text.lines() {
        let line = line.trim();
        if in_list {
            if line.starts_with('}') {
                in_list = false;
            } else if let Some((_, v)) = line.split_once(':') {
                exceptions.push_str(v.trim());
                exceptions.push(',');
            }
            continue;
        }
        let Some((k, v)) = line.split_once(" : ") else {
            continue;
        };
        if k.trim() == "ExceptionsList" {
            in_list = v.trim_start().starts_with("<array>");
        } else {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let server = |prefix: &str| -> Option<ProxyServer> {
        if map.get(&format!("{prefix}Enable")).map(String::as_str) != Some("1") {
            return None;
        }
        let host = map.get(&format!("{prefix}Proxy"))?;
        let port = map
            .get(&format!("{prefix}Port"))
            .and_then(|p| p.parse().ok())
            .unwrap_or(80);
        Some(ProxyServer::new(
            ProxyScheme::Http,
            host.to_ascii_lowercase(),
            port,
        ))
    };
    let mut no_proxy = NoProxy::parse(&exceptions);
    if map.get("ExcludeSimpleHostnames").map(String::as_str) == Some("1") {
        no_proxy.extend("<local>");
    }
    ProxyRules::from_parts(server("HTTP"), server("HTTPS"), no_proxy)
}

/// `reg query` of the Internet Settings key: `ProxyEnable` (`0x1`), `ProxyServer` (`host:port`, or
/// `http=h:p;https=h:p;...` per scheme) and `ProxyOverride` (the bypass list, `<local>` included).
pub(crate) fn parse_windows_registry(text: &str) -> ProxyRules {
    let value = |name: &str| -> Option<String> {
        text.lines().find_map(|l| {
            let mut it = l.split_whitespace();
            (it.next()? == name).then(|| {
                let _kind = it.next();
                it.collect::<Vec<_>>().join(" ")
            })
        })
    };
    let enabled = value("ProxyEnable")
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "0x1" | "1"));
    if !enabled {
        return ProxyRules::none();
    }
    let Some(servers) = value("ProxyServer") else {
        return ProxyRules::none();
    };
    windows_servers(
        &servers,
        value("ProxyOverride").as_deref().unwrap_or_default(),
    )
}

/// `netsh winhttp show proxy` (machine-wide). The labels are translated with the display language, so the values are read by
/// position: the first `label : value` line after the heading is the proxy server list, the second the bypass list. "Direct
/// access" has no such line. A first value that is not a proxy address means no proxy, never a wrong one.
pub(crate) fn parse_winhttp(text: &str) -> ProxyRules {
    let mut values = text
        .lines()
        .filter_map(|l| l.split_once(" :").map(|(_, v)| v.trim().to_string()));
    let Some(servers) = values
        .next()
        .filter(|s| !s.is_empty() && looks_like_servers(s))
    else {
        return ProxyRules::none();
    };
    windows_servers(&servers, values.next().as_deref().unwrap_or_default())
}

/// Is this a proxy server list (`host:port`, `http=h:p;https=h:p`) rather than a sentence?
fn looks_like_servers(text: &str) -> bool {
    text.split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .any(|part| {
            let addr = part.split_once('=').map_or(part, |(_, a)| a);
            !addr.contains(' ') && ProxyServer::parse(addr).is_ok()
        })
}

/// The path of a Windows tool in the system folder (`%SystemRoot%\System32`), so a lookalike in the current folder or on the
/// search path is never started.
pub(crate) fn windows_tool_path(tool: &str, system_root: Option<&str>) -> String {
    format!(
        r"{}\System32\{tool}",
        system_root.unwrap_or(r"C:\Windows").trim_end_matches('\\')
    )
}

fn windows_servers(servers: &str, bypass: &str) -> ProxyRules {
    let (mut http, mut https, mut any) = (None, None, None);
    for part in servers.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (scheme, addr) = part.split_once('=').unwrap_or(("", part));
        // The address may carry a scheme of its own (`http://h:p`); a bare one is an HTTP proxy.
        let Ok(server) = ProxyServer::parse(addr) else {
            continue;
        };
        match scheme.to_ascii_lowercase().as_str() {
            "" => any = Some(server),
            "http" => http = Some(server),
            "https" => https = Some(server),
            _ => {} // ftp, socks
        }
    }
    ProxyRules::from_parts(
        http.or_else(|| any.clone()),
        https.or(any),
        NoProxy::parse(bypass),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCUTIL: &str = "<dictionary> {
  ExceptionsList : <array> {
    0 : *.local
    1 : 169.254/16
    2 : corp.test
  }
  ExcludeSimpleHostnames : 1
  FTPPassive : 1
  HTTPEnable : 1
  HTTPPort : 8080
  HTTPProxy : Proxy.Corp
  HTTPSEnable : 1
  HTTPSPort : 8443
  HTTPSProxy : secure.corp
  SOCKSEnable : 0
}
";

    #[test]
    fn scutil_output_gives_proxies_and_exceptions() {
        let r = parse_scutil(SCUTIL);
        let https = r.for_url("https://files.example/x").unwrap();
        assert_eq!((https.host(), https.port()), ("secure.corp", 8443));
        let http = r.for_url("http://files.example/x").unwrap();
        assert_eq!((http.host(), http.port()), ("proxy.corp", 8080));
        for direct in [
            "https://a.local/",
            "http://169.254.1.1/",
            "https://x.corp.test/",
            "http://intranet/",
        ] {
            assert!(r.for_url(direct).is_none(), "{direct}");
        }
    }

    #[test]
    fn a_disabled_or_empty_scutil_answer_means_no_proxy() {
        assert!(
            parse_scutil(
                &SCUTIL
                    .replace("HTTPEnable : 1", "HTTPEnable : 0")
                    .replace("HTTPSEnable : 1", "HTTPSEnable : 0")
            )
            .is_empty()
        );
        assert!(parse_scutil("").is_empty());
        assert!(parse_scutil("garbage\n : \n").is_empty());
    }

    const REG: &str = "
HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings
    ProxyEnable    REG_DWORD    0x1
    ProxyServer    REG_SZ    http=web.corp:3128;https=tls.corp:3129;ftp=ftp.corp:21
    ProxyOverride    REG_SZ    localhost;*.corp;10.*;<local>
";

    #[test]
    fn windows_internet_settings_give_per_scheme_proxies_and_the_bypass_list() {
        let r = parse_windows_registry(REG);
        assert_eq!(r.for_url("http://x.example/").unwrap().host(), "web.corp");
        assert_eq!(r.for_url("https://x.example/").unwrap().port(), 3129);
        for direct in [
            "http://a.corp/",
            "http://10.1.1.1/",
            "http://intranet/",
            "http://localhost/",
        ] {
            assert!(r.for_url(direct).is_none(), "{direct}");
        }
    }

    #[test]
    fn one_windows_proxy_serves_both_schemes_and_disabled_means_none() {
        let one =
            "    ProxyEnable    REG_DWORD    0x1\n    ProxyServer    REG_SZ    proxy.corp:8080\n";
        let r = parse_windows_registry(one);
        assert_eq!(r.for_url("https://x.example").unwrap().host(), "proxy.corp");
        assert_eq!(r.for_url("http://x.example").unwrap().port(), 8080);
        assert!(parse_windows_registry(&one.replace("0x1", "0x0")).is_empty());
        assert!(parse_windows_registry("    ProxyEnable    REG_DWORD    0x1\n").is_empty());
        assert!(parse_windows_registry("").is_empty());
    }

    #[test]
    fn winhttp_output_is_the_fallback() {
        let t = "Current WinHTTP proxy settings:\n\n    Proxy Server(s) :  wh.corp:8080\n    Bypass List     :  *.corp;<local>\n";
        let r = parse_winhttp(t);
        assert_eq!(r.for_url("https://x.example").unwrap().host(), "wh.corp");
        assert!(r.for_url("https://a.corp").is_none());
        assert!(
            parse_winhttp(
                "Current WinHTTP proxy settings:\n\n    Direct access (no proxy server).\n"
            )
            .is_empty()
        );
    }

    /// The labels of `netsh` are translated (German, French, Japanese ...): the first value after the heading is the proxy
    /// server and the second the bypass list, whatever the labels say.
    #[test]
    fn winhttp_output_in_another_language_is_read_by_position() {
        let de = "Aktuelle WinHTTP-Proxyeinstellungen:\n\n    Proxyserver     :  wh.corp:8080\n    Ausnahmeliste   :  *.corp;<local>\n";
        let r = parse_winhttp(de);
        assert_eq!(r.for_url("https://x.example").unwrap().host(), "wh.corp");
        assert!(r.for_url("https://a.corp").is_none());
        assert!(
            parse_winhttp(
                "Aktuelle WinHTTP-Proxyeinstellungen:\n\n    Direkter Zugriff (kein Proxyserver).\n"
            )
            .is_empty()
        );
        // A value that is not an address (an unexpected layout) is no proxy rather than a wrong one.
        assert!(
            parse_winhttp("Heading:\n    Something :  not an address with spaces\n").is_empty()
        );
    }

    /// The tools are started from the system folder, never found through the search path or the current folder.
    #[test]
    fn windows_tools_are_started_from_the_system_folder() {
        let p = windows_tool_path("netsh.exe", Some(r"D:\Win"));
        assert_eq!(p, r"D:\Win\System32\netsh.exe");
        assert_eq!(
            windows_tool_path("reg.exe", None),
            r"C:\Windows\System32\reg.exe"
        );
    }
}
