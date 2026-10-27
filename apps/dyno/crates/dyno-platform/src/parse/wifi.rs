//! Output of the tools that name the Wi-Fi adapter and the network it joined: macOS `networksetup`, Linux `iwgetid` and
//! `nmcli`, Windows `netsh wlan`. Pure, so every OS's parser is tested everywhere.

/// The Wi-Fi devices in `networksetup -listallhardwareports` output (`Hardware Port: Wi-Fi` / `Device: en0`).
pub fn mac_wifi_devices(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut is_wifi = false;
    for line in text.lines() {
        let line = line.trim();
        if let Some(port) = line.strip_prefix("Hardware Port:") {
            let p = port.trim().to_lowercase();
            is_wifi = p == "wi-fi" || p == "airport";
        } else if let Some(dev) = line.strip_prefix("Device:") {
            let dev = dev.trim();
            if is_wifi && !dev.is_empty() {
                out.push(dev.to_string());
            }
            is_wifi = false;
        }
    }
    out
}

/// The network in `networksetup -getairportnetwork <device>` output. macOS also answers "not associated" when it hides
/// the name from programs without Location access, so `None` means "none or hidden".
pub fn mac_ssid(text: &str) -> Option<String> {
    let name = text.lines().find_map(|l| l.trim().strip_prefix("Current Wi-Fi Network:"))?.trim();
    (!name.is_empty() && !name.eq_ignore_ascii_case("<redacted>")).then(|| name.to_string())
}

/// The network in `iwgetid -r` output (one line, the SSID).
pub fn iwgetid_ssid(text: &str) -> Option<String> {
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// The active network in `nmcli -t -f active,ssid dev wifi` output (`yes:Home`; colons in a name are escaped `\:`).
pub fn nmcli_ssid(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let rest = l.strip_prefix("yes:")?;
        let name = rest.replace("\\:", ":");
        (!name.trim().is_empty()).then(|| name.trim().to_string())
    })
}

/// What `netsh wlan show interfaces` shows for the first connected adapter: its name and the network.
pub fn netsh_wifi(text: &str) -> Option<(String, Option<String>)> {
    let mut name: Option<String> = None;
    let mut ssid: Option<String> = None;
    let mut connected = false;
    let mut found: Option<(String, Option<String>)> = None;
    let flush =
        |name: &mut Option<String>, ssid: &mut Option<String>, connected: &mut bool, found: &mut Option<(String, Option<String>)>| {
            if let (Some(n), true, None) = (name.take(), *connected, found.as_ref()) {
                *found = Some((n, ssid.take()));
            }
            *ssid = None;
            *connected = false;
        };
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "Name" => {
                flush(&mut name, &mut ssid, &mut connected, &mut found);
                name = Some(v.to_string());
            }
            "State" => connected = v.eq_ignore_ascii_case("connected"),
            "SSID" => ssid = (!v.is_empty()).then(|| v.to_string()),
            _ => {}
        }
    }
    flush(&mut name, &mut ssid, &mut connected, &mut found);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_hardware_ports_name_the_wifi_device() {
        let text = "Hardware Port: Ethernet Adapter (en4)\nDevice: en4\nEthernet Address: aa\n\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: bb\n\nHardware Port: Thunderbolt Bridge\nDevice: bridge0\n";
        assert_eq!(mac_wifi_devices(text), vec!["en0"]);
        assert!(mac_wifi_devices("").is_empty());
        assert!(mac_wifi_devices("Hardware Port: Wi-Fi\n").is_empty());
    }

    #[test]
    fn mac_ssid_is_none_when_not_associated_or_hidden() {
        assert_eq!(mac_ssid("Current Wi-Fi Network: Home Net\n").as_deref(), Some("Home Net"));
        assert_eq!(mac_ssid("You are not associated with an AirPort network.\n"), None);
        assert_eq!(mac_ssid("Current Wi-Fi Network: <redacted>\n"), None);
        assert_eq!(mac_ssid("Current Wi-Fi Network: \n"), None);
    }

    #[test]
    fn linux_tools_name_the_network() {
        assert_eq!(iwgetid_ssid("Home\n").as_deref(), Some("Home"));
        assert_eq!(iwgetid_ssid("\n"), None);
        assert_eq!(iwgetid_ssid(""), None);
        assert_eq!(nmcli_ssid("no:Other\nyes:Cafe\\: Free\n").as_deref(), Some("Cafe: Free"));
        assert_eq!(nmcli_ssid("no:Other\n"), None);
        assert_eq!(nmcli_ssid("yes:\n"), None);
    }

    #[test]
    fn netsh_names_the_first_connected_adapter() {
        let text = "\nThere is 2 interfaces on the system:\n\n    Name                   : Wi-Fi 2\n    State                  : disconnected\n\n    Name                   : Wi-Fi\n    State                  : connected\n    SSID                   : Home\n    BSSID                  : aa:bb\n";
        assert_eq!(netsh_wifi(text), Some(("Wi-Fi".into(), Some("Home".into()))));
        assert_eq!(netsh_wifi("    Name : Wi-Fi\n    State : disconnected\n"), None);
        assert_eq!(
            netsh_wifi("    Name : Wi-Fi\n    State : connected\n"),
            Some(("Wi-Fi".into(), None))
        );
        assert_eq!(netsh_wifi("The Wireless AutoConfig Service (wlansvc) is not running."), None);
    }
}
