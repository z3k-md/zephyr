//! "ip", "my ip", "ip address", "local ip": this computer's addresses as inline answers. The
//! local ones come from the network interfaces; the public one is asked of an echo service,
//! only while such a query is showing, and kept for a minute.

use std::net::{IpAddr, UdpSocket};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Whether the text asks for this computer's IP address.
pub fn is_query(text: &str) -> bool {
    let text = text.trim().to_lowercase();
    let text = text.trim_end_matches('?');
    matches!(
        text,
        "ip" | "ip address"
            | "ip addr"
            | "my ip"
            | "my ip address"
            | "local ip"
            | "local ip address"
            | "what is my ip"
            | "whats my ip"
            | "what's my ip"
            | "public ip"
            | "external ip"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Wifi,
    Ethernet,
    Other,
}

impl Link {
    /// The row icon for an address on this kind of link.
    pub fn glyph(self) -> &'static str {
        match self {
            Link::Wifi => "glyph:wifi",
            Link::Ethernet => "glyph:ethernet",
            Link::Other => "glyph:lan",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub value: String,
    /// Where it comes from: "Wi-Fi · IPv4".
    pub detail: String,
    pub link: Link,
}

/// The kind of link behind an interface, and the name people know it by ("Wi-Fi" for en0).
pub fn describe(interface: &str) -> (Link, String) {
    let port = hardware_ports()
        .iter()
        .find(|(device, _)| device == interface)
        .map(|(_, port)| port.clone());
    let name = port.clone().unwrap_or_else(|| interface.to_string());
    let lower = name.to_lowercase();
    let link = if lower.contains("wi-fi")
        || lower.contains("wifi")
        || lower.contains("wireless")
        || lower.contains("wlan")
    {
        Link::Wifi
    } else if lower.contains("ethernet")
        || lower.contains("thunderbolt")
        || lower.starts_with("eth")
    {
        Link::Ethernet
    } else {
        Link::Other
    };
    (link, name)
}

/// macOS: each interface's hardware port ("en0" is "Wi-Fi"), read once. Windows already
/// names interfaces by what they are ("Wi-Fi", "Ethernet").
fn hardware_ports() -> &'static [(String, String)] {
    static PORTS: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
    PORTS.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            let Ok(output) = std::process::Command::new("networksetup")
                .arg("-listallhardwareports")
                .output()
            else {
                return Vec::new();
            };
            parse_ports(&String::from_utf8_lossy(&output.stdout))
        }
        #[cfg(not(target_os = "macos"))]
        Vec::new()
    })
}

/// `networksetup -listallhardwareports` output as (device, port) pairs.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_ports(text: &str) -> Vec<(String, String)> {
    let mut ports = Vec::new();
    let mut port: Option<&str> = None;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("Hardware Port: ") {
            port = Some(name.trim());
        } else if let (Some(device), Some(name)) = (line.strip_prefix("Device: "), port.take()) {
            ports.push((device.trim().to_string(), name.to_string()));
        }
    }
    ports
}

/// This computer's addresses, the one it reaches the internet through first, then the
/// other IPv4 ones, then IPv6. Loopback and link-local addresses are left out.
pub fn local() -> Vec<Address> {
    let primary = route_address();
    let interfaces = if_addrs::get_if_addrs().unwrap_or_default();
    let mut found: Vec<(u8, Address)> = interfaces
        .iter()
        .filter(|interface| !interface.is_loopback())
        .filter_map(|interface| {
            let ip = interface.ip();
            let usable = match ip {
                IpAddr::V4(v4) => !v4.is_link_local() && !v4.is_unspecified(),
                IpAddr::V6(v6) => !v6.is_unicast_link_local() && !v6.is_unspecified(),
            };
            if !usable {
                return None;
            }
            let rank = match ip {
                _ if Some(ip) == primary => 0,
                IpAddr::V4(_) => 1,
                IpAddr::V6(_) => 2,
            };
            let family = if ip.is_ipv4() { "IPv4" } else { "IPv6" };
            let (link, name) = describe(&interface.name);
            Some((
                rank,
                Address {
                    value: ip.to_string(),
                    detail: format!("{name} · {family}"),
                    link,
                },
            ))
        })
        .collect();
    found.sort_by_key(|(rank, _)| *rank);
    found.dedup_by(|a, b| a.1.value == b.1.value);
    found.into_iter().map(|(_, address)| address).collect()
}

/// The address the system routes internet traffic from. Connecting a UDP socket only picks
/// the route; nothing is sent.
fn route_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:53").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

static PUBLIC: Mutex<Option<(Instant, String)>> = Mutex::new(None);

/// Forgets the public address, after the network changed.
pub fn forget_public() {
    if let Ok(mut cache) = PUBLIC.lock() {
        *cache = None;
    }
}

/// The address the internet sees, when the echo service answers within two seconds; kept
/// for a minute.
pub async fn public() -> Option<String> {
    if let Ok(cache) = PUBLIC.lock()
        && let Some((at, ip)) = cache.as_ref()
        && at.elapsed() < Duration::from_secs(60)
    {
        return Some(ip.clone());
    }
    let text = crate::util::http()
        .get("https://api.ipify.org")
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    let ip: IpAddr = text.trim().parse().ok()?;
    let ip = ip.to_string();
    if let Ok(mut cache) = PUBLIC.lock() {
        *cache = Some((Instant::now(), ip.clone()));
    }
    Some(ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_ip_questions_only() {
        for asked in [
            "ip",
            "IP Address",
            "my ip",
            "local ip",
            "what's my ip?",
            "public ip",
        ] {
            assert!(is_query(asked), "{asked}");
        }
        for search in ["ipad", "ip man", "ipa recipes", "zip", "192.168.0.1"] {
            assert!(!is_query(search), "{search}");
        }
    }

    #[test]
    fn hardware_ports_name_their_devices() {
        let text = "Hardware Port: Ethernet Adapter (en3)\nDevice: en3\nEthernet Address: 02\n\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: 2c\n";
        assert_eq!(
            parse_ports(text),
            [
                ("en3".to_string(), "Ethernet Adapter (en3)".to_string()),
                ("en0".to_string(), "Wi-Fi".to_string()),
            ]
        );
    }

    /// Shows this computer's addresses: `cargo test lists_this_computers_addresses -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn lists_this_computers_addresses() {
        for address in local() {
            println!("{} ({})", address.value, address.detail);
        }
        println!("public: {:?}", tauri::async_runtime::block_on(public()));
    }
}
