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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub value: String,
    /// Where it comes from: "Wi-Fi · IPv4".
    pub detail: String,
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
            Some((
                rank,
                Address {
                    value: ip.to_string(),
                    detail: format!("{} · {family}", interface.name),
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

/// The address the internet sees, when the echo service answers within two seconds.
pub async fn public() -> Option<String> {
    static CACHE: Mutex<Option<(Instant, String)>> = Mutex::new(None);
    if let Ok(cache) = CACHE.lock()
        && let Some((at, ip)) = cache.as_ref()
        && at.elapsed() < Duration::from_secs(60)
    {
        return Some(ip.clone());
    }
    let client = reqwest::Client::builder()
        .user_agent("Zephyr")
        .timeout(Duration::from_secs(2))
        .build()
        .ok()?;
    let text = client
        .get("https://api.ipify.org")
        .send()
        .await
        .ok()?
        .text()
        .await
        .ok()?;
    let ip: IpAddr = text.trim().parse().ok()?;
    let ip = ip.to_string();
    if let Ok(mut cache) = CACHE.lock() {
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
