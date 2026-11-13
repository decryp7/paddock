//! The network interfaces and the Wi-Fi network joined (WP-35), for the timeline's network change events. Interfaces come
//! from sysinfo (one cheap call); the Wi-Fi adapter and network name from a small tool per OS, run at most once a minute
//! with a timeout. Observe-only.

use crate::parse::wifi;
use dyno_core::netchange::{classify, usable, Link, NetState};
use dyno_core::ports::{NetworkLinks, PlatformError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a Wi-Fi tool's answer is reused.
const WIFI_TTL: Duration = Duration::from_secs(60);
/// Longest a Wi-Fi tool may run.
const TOOL_TIMEOUT: Duration = Duration::from_secs(5);

/// What the OS says about Wi-Fi: its adapters and the network joined.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WifiInfo {
    /// Interface names the OS lists as Wi-Fi adapters.
    pub devices: Vec<String>,
    /// The network joined, when the OS tells.
    pub ssid: Option<String>,
}

/// Reads [`WifiInfo`]; a seam so the interface logic is tested without any tool.
pub trait WifiProbe: Send + Sync {
    /// One read. A tool that is missing or fails gives an empty answer (no Wi-Fi events), never an error.
    fn read(&self) -> WifiInfo;
}

/// The real tools: `networksetup` (macOS), `iwgetid` then `nmcli` and `/sys/class/net/*/wireless` (Linux), `netsh wlan` (Windows).
#[derive(Debug, Default)]
pub struct OsWifi;

fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = crate::process::command(program);
    cmd.args(args);
    crate::process::output_text(cmd, TOOL_TIMEOUT).ok()
}

impl WifiProbe for OsWifi {
    fn read(&self) -> WifiInfo {
        if cfg!(target_os = "macos") {
            let devices = run("/usr/sbin/networksetup", &["-listallhardwareports"])
                .map(|t| wifi::mac_wifi_devices(&t))
                .unwrap_or_default();
            let ssid = devices
                .first()
                .and_then(|d| run("/usr/sbin/networksetup", &["-getairportnetwork", d]))
                .and_then(|t| wifi::mac_ssid(&t));
            WifiInfo { devices, ssid }
        } else if cfg!(windows) {
            match run("netsh", &["wlan", "show", "interfaces"]).and_then(|t| wifi::netsh_wifi(&t)) {
                Some((name, ssid)) => WifiInfo { devices: vec![name], ssid },
                None => WifiInfo::default(),
            }
        } else {
            let devices = linux_wireless_devices(std::path::Path::new("/sys/class/net"));
            let ssid = if devices.is_empty() {
                None
            } else {
                run("iwgetid", &["-r"])
                    .and_then(|t| wifi::iwgetid_ssid(&t))
                    .or_else(|| run("nmcli", &["-t", "-f", "active,ssid", "dev", "wifi"]).and_then(|t| wifi::nmcli_ssid(&t)))
            };
            WifiInfo { devices, ssid }
        }
    }
}

/// Interfaces under `root` (`/sys/class/net`) that have a `wireless` folder.
pub fn linux_wireless_devices(root: &std::path::Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut v: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().join("wireless").is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// Where the interface addresses come from; a seam for tests.
pub trait Interfaces: Send + Sync {
    /// Every interface with all its addresses.
    fn all(&self) -> Vec<(String, Vec<std::net::IpAddr>)>;
}

/// sysinfo's interface list.
#[derive(Debug, Default)]
pub struct SysinfoInterfaces;

impl Interfaces for SysinfoInterfaces {
    fn all(&self) -> Vec<(String, Vec<std::net::IpAddr>)> {
        let Ok(nets) = sysinfo::Networks::new_with_refreshed_list()
            .inspect_err(|why| log::warn!("the network interface list is unavailable (sysinfo): {why}; no interfaces are listed"))
        else {
            return Vec::new();
        };
        nets.iter()
            .map(|(name, data)| {
                (
                    name.to_string_lossy().into_owned(),
                    data.ip_networks().iter().map(|n| n.addr).collect(),
                )
            })
            .collect()
    }
}

/// The links of this machine.
pub struct OsLinks {
    interfaces: Box<dyn Interfaces>,
    wifi: Box<dyn WifiProbe>,
    cache: Mutex<Option<(Instant, WifiInfo)>>,
}

impl OsLinks {
    /// The real interfaces and tools.
    pub fn new() -> OsLinks {
        OsLinks::with(Box::new(SysinfoInterfaces), Box::new(OsWifi))
    }

    /// Links over the given sources (tests).
    pub fn with(interfaces: Box<dyn Interfaces>, wifi: Box<dyn WifiProbe>) -> OsLinks {
        OsLinks {
            interfaces,
            wifi,
            cache: Mutex::new(None),
        }
    }

    fn wifi_info(&self) -> WifiInfo {
        let mut c = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((at, info)) = c.as_ref() {
            if at.elapsed() < WIFI_TTL {
                return info.clone();
            }
        }
        let info = self.wifi.read();
        *c = Some((Instant::now(), info.clone()));
        info
    }
}

impl Default for OsLinks {
    fn default() -> Self {
        OsLinks::new()
    }
}

impl NetworkLinks for OsLinks {
    fn links(&self) -> Result<NetState, PlatformError> {
        let wifi = self.wifi_info();
        let mut links: Vec<Link> = self
            .interfaces
            .all()
            .into_iter()
            .map(|(name, addrs)| {
                let is_wifi = wifi.devices.iter().any(|d| d == &name);
                let mut addresses: Vec<std::net::IpAddr> = addrs.into_iter().filter(usable).collect();
                addresses.sort();
                addresses.dedup();
                Link {
                    kind: classify(&name, is_wifi),
                    name,
                    addresses,
                }
            })
            .collect();
        links.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(NetState { links, ssid: wifi.ssid })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::netchange::LinkKind;
    use std::sync::Arc;

    struct Fixed(Vec<(String, Vec<std::net::IpAddr>)>);
    impl Interfaces for Fixed {
        fn all(&self) -> Vec<(String, Vec<std::net::IpAddr>)> {
            self.0.clone()
        }
    }

    struct Counting(Arc<Mutex<u32>>, WifiInfo);
    impl WifiProbe for Counting {
        fn read(&self) -> WifiInfo {
            *self.0.lock().unwrap() += 1;
            self.1.clone()
        }
    }

    fn ip(s: &str) -> std::net::IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn interfaces_are_classified_filtered_and_sorted() {
        let calls = Arc::new(Mutex::new(0));
        let links = OsLinks::with(
            Box::new(Fixed(vec![
                ("utun4".into(), vec![ip("10.8.0.2"), ip("fe80::1")]),
                ("en0".into(), vec![ip("192.168.1.5"), ip("fe80::2"), ip("192.168.1.5")]),
                ("lo0".into(), vec![ip("127.0.0.1")]),
            ])),
            Box::new(Counting(
                calls.clone(),
                WifiInfo {
                    devices: vec!["en0".into()],
                    ssid: Some("Home".into()),
                },
            )),
        );
        let s = links.links().unwrap();
        assert_eq!(s.ssid.as_deref(), Some("Home"));
        let by = |n: &str| s.links.iter().find(|l| l.name == n).unwrap().clone();
        assert_eq!(by("en0").kind, LinkKind::Wifi);
        assert_eq!(by("en0").addresses, vec![ip("192.168.1.5")]);
        assert_eq!(by("utun4").kind, LinkKind::Vpn);
        assert_eq!(by("utun4").addresses, vec![ip("10.8.0.2")]);
        assert_eq!(by("lo0").kind, LinkKind::Ignored);
        assert!(by("lo0").addresses.is_empty());
        assert_eq!(s.links.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["en0", "lo0", "utun4"]);
        // The tool is not run again within its time to live.
        links.links().unwrap();
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[test]
    fn wireless_devices_are_found_by_their_folder() {
        let t = paddock_testkit::private_tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("wlan0/wireless")).unwrap();
        std::fs::create_dir_all(t.path().join("eth0/device")).unwrap();
        assert_eq!(linux_wireless_devices(t.path()), vec!["wlan0"]);
        assert!(linux_wireless_devices(&t.path().join("missing")).is_empty());
    }

    #[test]
    fn the_real_interfaces_can_be_read() {
        // Only that it does not fail or panic: what is there depends on the machine.
        assert!(SysinfoInterfaces.all().iter().all(|(n, _)| !n.is_empty()));
    }
}
