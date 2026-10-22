//! Host names for the addresses on the Network page: the request is parsed once into typed addresses (bounded,
//! de-duplicated), the answer is a plain DTO. The resolver itself (the OS) and its cache live in the agent.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Most addresses resolved in one request (the page asks for what is on screen).
pub const MAX_RESOLVE: usize = 64;

/// The answer for one address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostName {
    /// The address as the page sent it.
    pub address: String,
    /// Its name, or `None` when it has none or the lookup timed out.
    pub name: Option<String>,
}

/// Parses the addresses the page wants names for: at most [`MAX_RESOLVE`], each a literal IP address, de-duplicated,
/// in order. Anything else is refused with what to fix (the webview is not trusted).
pub fn parse_addresses(addresses: &[String]) -> Result<Vec<IpAddr>, String> {
    if addresses.len() > MAX_RESOLVE {
        return Err(format!("Resolve at most {MAX_RESOLVE} addresses at a time."));
    }
    let mut out: Vec<IpAddr> = Vec::with_capacity(addresses.len());
    for a in addresses {
        let ip: IpAddr = a
            .trim()
            .parse()
            .map_err(|_| format!("\"{}\" is not an IP address.", a.chars().take(40).collect::<String>()))?;
        if !out.contains(&ip) {
            out.push(ip);
        }
    }
    Ok(out)
}

/// A resolver answer that is safe to show: control characters and absurd lengths are dropped (a name comes from the
/// network, which is untrusted), and the trailing root dot is removed.
pub fn clean_name(raw: &str) -> Option<String> {
    let name = raw.trim().trim_end_matches('.');
    if name.is_empty() || name.len() > 253 || name.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_parsed_deduplicated_and_bounded() {
        let ok = parse_addresses(&["1.1.1.1".into(), " 2001:db8::1 ".into(), "1.1.1.1".into()]).unwrap();
        assert_eq!(ok.len(), 2);
        assert!(parse_addresses(&[]).unwrap().is_empty());
        let many: Vec<String> = (0..=MAX_RESOLVE).map(|i| format!("10.0.0.{i}")).collect();
        assert!(parse_addresses(&many).unwrap_err().contains("at most"));
        let e = parse_addresses(&["not-an-ip".into()]).unwrap_err();
        assert!(e.contains("not an IP address"));
        let long = "9".repeat(500);
        assert!(
            parse_addresses(&[long]).unwrap_err().len() < 100,
            "an absurd value is not echoed back whole"
        );
    }

    #[test]
    fn a_resolver_answer_is_cleaned_or_dropped() {
        assert_eq!(clean_name("host.example.com.").as_deref(), Some("host.example.com"));
        assert_eq!(clean_name("  a.b ").as_deref(), Some("a.b"));
        assert_eq!(clean_name(""), None);
        assert_eq!(clean_name("."), None);
        assert_eq!(clean_name("bad name"), None);
        assert_eq!(clean_name("evil\u{1b}[31m"), None);
        assert_eq!(clean_name(&"a".repeat(254)), None);
    }
}
