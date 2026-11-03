//! "Look up this IP": Dyno builds an https URL from the user's template and the browser opens it. Dyno never fetches
//! anything itself, and nothing but the address (and, for the ASN template, the AS number) is ever put into the URL.

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Longest template accepted, in bytes.
pub const MAX_TEMPLATE: usize = 300;
/// The address the Test button looks up (Google's public resolver: public, well known, harmless).
pub const TEST_ADDRESS: &str = "8.8.8.8";
/// The template used until the user chooses another.
pub const DEFAULT_TEMPLATE: &str = "https://ipinfo.io/{ip}";

/// A ready-made template: a name for the dropdown and the URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    /// Shown in the dropdown.
    pub name: &'static str,
    /// What it tells you.
    pub about: &'static str,
    /// The IP template.
    pub ip: &'static str,
    /// The ASN template, when the site has one.
    pub asn: Option<&'static str>,
}

/// The built-in presets (each accepts the address in the path). "Custom" is the UI's own entry.
pub const PRESETS: [Preset; 3] = [
    Preset {
        name: "ipinfo.io",
        about: "Location and network owner",
        ip: "https://ipinfo.io/{ip}",
        asn: Some("https://ipinfo.io/{asn}"),
    },
    Preset {
        name: "bgp.he.net",
        about: "Network owner and BGP routing",
        ip: "https://bgp.he.net/ip/{ip}",
        asn: Some("https://bgp.he.net/{asn}"),
    },
    Preset {
        name: "AbuseIPDB",
        about: "Abuse reports about the address",
        ip: "https://www.abuseipdb.com/check/{ip}",
        asn: None,
    },
];

/// Why a template or an address cannot be used (the text says what to change).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LookupError {
    /// The template is empty or too long.
    #[error("The lookup address must be 1 to {MAX_TEMPLATE} characters.")]
    Length,
    /// Not https.
    #[error("The lookup address must start with https://.")]
    NotHttps,
    /// No `{ip}`.
    #[error("The lookup address must contain {{ip}} where the address goes, for example https://ipinfo.io/{{ip}}.")]
    NoPlaceholder,
    /// Another `{...}` placeholder, or control characters or spaces.
    #[error("The lookup address may only contain {{ip}} (and {{asn}} in the ASN template), no spaces or other placeholders.")]
    Unsupported,
    /// The host part is empty or has credentials.
    #[error("The lookup address needs a plain host name (no user name or password).")]
    Host,
    /// The text is not an IP address.
    #[error("That is not an IP address.")]
    NotAnIp,
    /// Private, loopback and the like.
    #[error("Private address, nothing to look up.")]
    NotPublic,
}

/// A validated template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    text: String,
}

impl Template {
    /// Parses `text`, which must hold `placeholder` (`ip` or `asn`) at least once and no other placeholder.
    pub fn parse(text: &str, placeholder: &str) -> Result<Template, LookupError> {
        let t = text.trim();
        if t.is_empty() || t.len() > MAX_TEMPLATE {
            return Err(LookupError::Length);
        }
        let Some(rest) = t.strip_prefix("https://") else {
            return Err(LookupError::NotHttps);
        };
        if t.chars().any(|c| c.is_control() || c.is_whitespace() || !c.is_ascii()) {
            return Err(LookupError::Unsupported);
        }
        let marker = format!("{{{placeholder}}}");
        if !t.contains(&marker) {
            return Err(LookupError::NoPlaceholder);
        }
        let stripped = t.replace(&marker, "");
        if stripped.contains('{') || stripped.contains('}') {
            return Err(LookupError::Unsupported);
        }
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        if authority.is_empty() || authority.contains('@') || authority.contains('{') {
            return Err(LookupError::Host);
        }
        Ok(Template { text: t.to_string() })
    }

    fn fill(&self, placeholder: &str, value: &str) -> String {
        self.text.replace(&format!("{{{placeholder}}}"), value)
    }
}

/// Percent-encodes everything but unreserved characters and `:` (which an IPv6 address needs and a path allows).
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b':') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Parses an address as the page shows it: strips a zone id (`fe80::1%en0`) and brackets, accepts IPv4-mapped IPv6.
pub fn parse_ip(text: &str) -> Result<IpAddr, LookupError> {
    let t = text.trim().trim_start_matches('[').trim_end_matches(']');
    let t = t.split('%').next().unwrap_or(t);
    match t.parse::<IpAddr>() {
        Ok(IpAddr::V6(v6)) => Ok(v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4)),
        Ok(v4) => Ok(v4),
        Err(_) => Err(LookupError::NotAnIp),
    }
}

/// What kind of address this is. Only [`AddressClass::Public`] is looked up, located on the map or matched against a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AddressClass {
    /// Known to the public internet.
    Public,
    /// This computer (loopback, unspecified).
    Loopback,
    /// RFC 1918 and unique-local addresses.
    Private,
    /// Link-local.
    LinkLocal,
    /// Carrier-grade NAT (`100.64.0.0/10`).
    Cgnat,
    /// Multicast and broadcast.
    Multicast,
    /// Documentation, benchmarking (`198.18.0.0/15`, also the fake-IP range of proxy tools), IETF protocol assignments
    /// (`192.0.0.0/24`), 6to4 (`2002::/16`), Teredo (`2001::/32`), discard-only (`100::/64`).
    Special,
    /// Reserved (`240.0.0.0/4`, `0.0.0.0/8`).
    Reserved,
}

fn classify_v4(a: Ipv4Addr) -> AddressClass {
    let o = a.octets();
    if a.is_loopback() || a.is_unspecified() {
        AddressClass::Loopback
    } else if a.is_private() {
        AddressClass::Private
    } else if a.is_link_local() {
        AddressClass::LinkLocal
    } else if o[0] == 100 && (o[1] & 0xC0) == 64 {
        AddressClass::Cgnat
    } else if a.is_multicast() || a.is_broadcast() {
        AddressClass::Multicast
    } else if a.is_documentation() || (o[0] == 198 && (o[1] & 0xFE) == 18) || (o[0] == 192 && o[1] == 0 && o[2] == 0) {
        AddressClass::Special
    } else if o[0] == 0 || o[0] >= 240 {
        AddressClass::Reserved
    } else {
        AddressClass::Public
    }
}

fn classify_v6(a: Ipv6Addr) -> AddressClass {
    let s = a.segments();
    if a.is_loopback() || a.is_unspecified() {
        AddressClass::Loopback
    } else if (s[0] & 0xFE00) == 0xFC00 {
        AddressClass::Private
    } else if (s[0] & 0xFFC0) == 0xFE80 {
        AddressClass::LinkLocal
    } else if a.is_multicast() {
        AddressClass::Multicast
    } else if (s[0] == 0x2001 && s[1] == 0x0DB8) // documentation
        || s[0] == 0x2002 // 6to4
        || (s[0] == 0x2001 && s[1] == 0) // Teredo
        || (s[0] == 0x0100 && s[1] == 0 && s[2] == 0 && s[3] == 0)
    // discard-only
    {
        AddressClass::Special
    } else {
        AddressClass::Public
    }
}

/// The IPv4 address inside a NAT64 address (`64:ff9b::/96`), which is matched and located as that address.
pub fn nat64_embedded(a: &Ipv6Addr) -> Option<Ipv4Addr> {
    let s = a.segments();
    if s[..6] == [0x64, 0xFF9B, 0, 0, 0, 0] {
        let o = a.octets();
        Some(Ipv4Addr::new(o[12], o[13], o[14], o[15]))
    } else {
        None
    }
}

/// Unwraps an IPv4-mapped (`::ffff:a.b.c.d`) or NAT64 IPv6 address to the IPv4 address inside; any other address is returned
/// as it is.
pub fn unwrapped(ip: &IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(a) => a.to_ipv4_mapped().or_else(|| nat64_embedded(a)).map_or(*ip, IpAddr::V4),
        v4 => *v4,
    }
}

/// The class of `ip` after unwrapping IPv4-mapped and NAT64 forms.
pub fn classify(ip: &IpAddr) -> AddressClass {
    match unwrapped(ip) {
        IpAddr::V4(a) => classify_v4(a),
        IpAddr::V6(a) => classify_v6(a),
    }
}

/// Whether an address is one the public internet knows: private, loopback, link-local, CGNAT, multicast, reserved, benchmarking,
/// documentation and the other special-purpose ranges are not. A NAT64 address is judged by the IPv4 address inside it.
pub fn is_public(ip: &IpAddr) -> bool {
    classify(ip) == AddressClass::Public
}

/// The lookup URL for `ip` from `template`: the canonical address (IPv6 compressed, no zone) substituted, encoded.
pub fn url_for_ip(template: &Template, ip: &IpAddr) -> Result<String, LookupError> {
    if !is_public(ip) {
        return Err(LookupError::NotPublic);
    }
    Ok(template.fill("ip", &encode(&ip.to_string())))
}

/// The lookup URL for an AS number (digits only; `AS13335` is accepted and normalised).
pub fn url_for_asn(template: &Template, asn: &str) -> Result<String, LookupError> {
    let digits = asn.trim().trim_start_matches(['A', 'a']).trim_start_matches(['S', 's']);
    if digits.is_empty() || digits.len() > 10 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(LookupError::Unsupported);
    }
    Ok(template.fill("asn", &format!("AS{digits}")))
}

/// The user's lookup settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IpLookupSettings {
    /// The IP template (https, with `{ip}`).
    pub ip_template: String,
    /// The optional ASN template (https, with `{asn}`); empty when unused.
    pub asn_template: String,
}

impl Default for IpLookupSettings {
    fn default() -> Self {
        IpLookupSettings {
            ip_template: DEFAULT_TEMPLATE.into(),
            asn_template: String::new(),
        }
    }
}

impl IpLookupSettings {
    /// Settings that always hold valid templates: an invalid IP template falls back to the default, an invalid ASN
    /// template to none (the editor validates too, so this only catches hand-edited files).
    pub fn normalised(mut self) -> IpLookupSettings {
        match Template::parse(&self.ip_template, "ip") {
            Ok(t) => self.ip_template = t.text,
            Err(_) => self.ip_template = DEFAULT_TEMPLATE.into(),
        }
        self.asn_template = match Template::parse(&self.asn_template, "asn") {
            Ok(t) => t.text,
            Err(_) => String::new(),
        };
        self
    }

    /// The URL to open for `ip`.
    pub fn url(&self, ip: &str) -> Result<String, LookupError> {
        let template = Template::parse(&self.ip_template, "ip")?;
        url_for_ip(&template, &parse_ip(ip)?)
    }

    /// The URL the Test button opens, for the settings as typed (not yet saved).
    pub fn test_url(&self) -> Result<String, LookupError> {
        let template = Template::parse(&self.ip_template, "ip")?;
        url_for_ip(&template, &parse_ip(TEST_ADDRESS)?)
    }

    /// The ASN URL, when a template is set.
    pub fn asn_url(&self, asn: &str) -> Result<Option<String>, LookupError> {
        if self.asn_template.is_empty() {
            return Ok(None);
        }
        url_for_asn(&Template::parse(&self.asn_template, "asn")?, asn).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_purpose_ranges_are_never_public() {
        for ip in [
            "198.18.0.1",
            "198.19.255.254",
            "192.0.0.8",
            "2002::1",
            "2002:c000:204::1",
            "2001::1",
            "100::1",
            "100::ffff",
            "64:ff9b::a00:1",
            "64:ff9b::c612:1",
        ] {
            let ip: IpAddr = ip.parse().unwrap();
            assert!(!is_public(&ip), "{ip}");
        }
        for ip in [
            "198.17.255.255",
            "198.20.0.0",
            "192.0.1.1",
            "2001:1::1",
            "2003::1",
            "100:0:0:1::1",
            "8.8.8.8",
        ] {
            let ip: IpAddr = ip.parse().unwrap();
            assert!(is_public(&ip), "{ip}");
        }
    }

    #[test]
    fn nat64_is_judged_by_the_address_inside() {
        let a: IpAddr = "64:ff9b::cb00:7107".parse().unwrap(); // 203.0.113.7, documentation
        assert_eq!(unwrapped(&a), "203.0.113.7".parse::<IpAddr>().unwrap());
        assert_eq!(classify(&a), AddressClass::Special);
        let b: IpAddr = "64:ff9b::808:808".parse().unwrap();
        assert_eq!(unwrapped(&b), "8.8.8.8".parse::<IpAddr>().unwrap());
        assert!(is_public(&b));
        let mapped: IpAddr = "::ffff:10.0.0.1".parse().unwrap();
        assert_eq!(classify(&mapped), AddressClass::Private);
    }

    #[test]
    fn look_up_refuses_special_ranges() {
        let s = IpLookupSettings::default();
        assert_eq!(s.url("198.18.0.5"), Err(LookupError::NotPublic));
        assert_eq!(s.url("2002::1"), Err(LookupError::NotPublic));
    }

    fn t(s: &str) -> Result<Template, LookupError> {
        Template::parse(s, "ip")
    }

    #[test]
    fn templates_must_be_https_with_the_placeholder_and_nothing_else() {
        assert!(t("https://ipinfo.io/{ip}").is_ok());
        assert!(t("https://example.com/lookup?query={ip}&x=1").is_ok());
        assert_eq!(t(""), Err(LookupError::Length));
        assert_eq!(
            t(&format!("https://a/{{ip}}{}", "x".repeat(MAX_TEMPLATE))),
            Err(LookupError::Length)
        );
        assert_eq!(t("http://ipinfo.io/{ip}"), Err(LookupError::NotHttps));
        assert_eq!(t("javascript:alert({ip})"), Err(LookupError::NotHttps));
        assert_eq!(t("https://ipinfo.io/"), Err(LookupError::NoPlaceholder));
        assert_eq!(t("https://ipinfo.io/{ip}/{asn}"), Err(LookupError::Unsupported));
        assert_eq!(t("https://ipinfo.io/{ip} x"), Err(LookupError::Unsupported));
        assert_eq!(t("https://user:pw@ipinfo.io/{ip}"), Err(LookupError::Host));
        assert_eq!(t("https:///{ip}"), Err(LookupError::Host));
        assert_eq!(t("https://{ip}.example.com/"), Err(LookupError::Host));
        assert_eq!(t("https://exämple.com/{ip}"), Err(LookupError::Unsupported));
    }

    #[test]
    fn the_canonical_address_is_substituted() {
        let tpl = t("https://ipinfo.io/{ip}").unwrap();
        let ip = |s| parse_ip(s).unwrap();
        assert_eq!(url_for_ip(&tpl, &ip("8.8.8.8")).unwrap(), "https://ipinfo.io/8.8.8.8");
        assert_eq!(
            url_for_ip(&tpl, &ip("2606:4700:4700:0000:0000:0000:0000:1111")).unwrap(),
            "https://ipinfo.io/2606:4700:4700::1111"
        );
        assert_eq!(
            url_for_ip(&tpl, &ip("[2606:4700:4700::1111]")).unwrap(),
            "https://ipinfo.io/2606:4700:4700::1111"
        );
        assert_eq!(
            url_for_ip(&tpl, &ip("::ffff:8.8.4.4")).unwrap(),
            "https://ipinfo.io/8.8.4.4",
            "a mapped address is the IPv4 one"
        );
        assert_eq!(encode("a b/c?d"), "a%20b%2Fc%3Fd");
    }

    #[test]
    fn zone_ids_and_junk_never_reach_the_url() {
        assert_eq!(parse_ip("fe80::1%en0").unwrap(), "fe80::1".parse::<IpAddr>().unwrap());
        for bad in ["", "not an ip", "8.8.8.8/evil", "8.8.8.8?x=1", "8.8.8.8 ", "1.2.3"] {
            // Trailing spaces are trimmed; everything else must fail to parse.
            if bad.trim() == "8.8.8.8" {
                assert!(parse_ip(bad).is_ok());
            } else {
                assert_eq!(parse_ip(bad), Err(LookupError::NotAnIp), "{bad:?}");
            }
        }
    }

    #[test]
    fn private_loopback_link_local_and_cgnat_addresses_have_nothing_to_look_up() {
        let tpl = t("https://ipinfo.io/{ip}").unwrap();
        for private in [
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.9.9",
            "100.64.0.1",
            "100.127.255.254",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "192.0.2.1",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12:3456::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:192.168.0.1",
        ] {
            let ip = parse_ip(private).unwrap();
            assert!(!is_public(&ip), "{private}");
            assert_eq!(url_for_ip(&tpl, &ip), Err(LookupError::NotPublic), "{private}");
        }
        for public in [
            "8.8.8.8",
            "1.1.1.1",
            "100.63.255.255",
            "100.128.0.1",
            "172.32.0.1",
            "2606:4700:4700::1111",
            "2a00:1450:4001::1",
        ] {
            assert!(is_public(&parse_ip(public).unwrap()), "{public}");
        }
    }

    #[test]
    fn the_asn_template_takes_only_digits() {
        let tpl = Template::parse("https://bgp.he.net/{asn}", "asn").unwrap();
        assert_eq!(url_for_asn(&tpl, "AS13335").unwrap(), "https://bgp.he.net/AS13335");
        assert_eq!(url_for_asn(&tpl, " 15169 ").unwrap(), "https://bgp.he.net/AS15169");
        for bad in ["", "AS", "13335/../x", "AS1a", "12345678901"] {
            assert!(url_for_asn(&tpl, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn settings_are_always_valid_and_the_presets_are_valid_templates() {
        let s = IpLookupSettings {
            ip_template: "http://nope/{ip}".into(),
            asn_template: "https://x/{ip}".into(),
        }
        .normalised();
        assert_eq!(s, IpLookupSettings::default());
        assert_eq!(s.url("8.8.8.8").unwrap(), "https://ipinfo.io/8.8.8.8");
        assert_eq!(s.url("10.0.0.1"), Err(LookupError::NotPublic));
        assert_eq!(s.asn_url("AS1").unwrap(), None);
        assert_eq!(s.test_url().unwrap(), "https://ipinfo.io/8.8.8.8");
        for p in PRESETS {
            Template::parse(p.ip, "ip").unwrap();
            if let Some(a) = p.asn {
                Template::parse(a, "asn").unwrap();
            }
        }
        let with_asn = IpLookupSettings {
            ip_template: PRESETS[1].ip.into(),
            asn_template: PRESETS[1].asn.unwrap().into(),
        }
        .normalised();
        assert_eq!(with_asn.asn_url("AS13335").unwrap().unwrap(), "https://bgp.he.net/AS13335");
        assert!(LookupError::NotPublic.to_string().contains("nothing to look up"));
    }
}
