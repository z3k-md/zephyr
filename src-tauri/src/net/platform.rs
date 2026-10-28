//! What the connection monitor asks of the system: an ICMP echo without admin rights, the
//! route to the internet (the router and the interface behind it), and the DNS servers.

use std::net::Ipv4Addr;
use std::time::Duration;

/// The way to the internet right now. No route at all means offline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Route {
    /// The router; none when the internet is on the same link (or there's no route).
    pub gateway: Option<Ipv4Addr>,
    /// The interface traffic leaves by, named as the throughput counters name it ("Wi-Fi" on
    /// Windows, "en0" on macOS).
    pub interface: Option<String>,
    /// Whether there's a route to the internet at all.
    pub online: bool,
}

pub use imp::{Pinger, dns_servers, route};

#[cfg(windows)]
mod imp {
    use std::net::Ipv4Addr;
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{HANDLE, NO_ERROR};
    use windows::Win32::NetworkManagement::IpHelper::{
        FIXED_INFO_W2KSP1, GetBestRoute2, GetIfEntry2, GetNetworkParams, ICMP_ECHO_REPLY,
        IP_ADDR_STRING, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, MIB_IF_ROW2,
        MIB_IPFORWARD_ROW2,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_INET};

    use super::Route;

    /// One ICMP handle; each target gets its own, so two can wait at once.
    pub struct Pinger(HANDLE);

    // SAFETY: an ICMP handle can be used from any thread; each Pinger is used by one at a time.
    unsafe impl Send for Pinger {}

    impl Pinger {
        pub fn new() -> Option<Self> {
            // SAFETY: no arguments; the handle is closed on drop.
            unsafe { IcmpCreateFile().ok().map(Pinger) }
        }

        /// The round trip to `ip` in ms, or none when no reply came within `timeout`.
        pub fn ping(&mut self, ip: Ipv4Addr, timeout: Duration) -> Option<f32> {
            let payload = [0x5Au8; 32];
            let mut reply = [0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + 32 + 8];
            let started = Instant::now();
            // SAFETY: a live handle, the payload and a reply buffer big enough for one reply.
            let replies = unsafe {
                IcmpSendEcho(
                    self.0,
                    u32::from_ne_bytes(ip.octets()),
                    payload.as_ptr().cast(),
                    payload.len() as u16,
                    None,
                    reply.as_mut_ptr().cast(),
                    reply.len() as u32,
                    timeout.as_millis() as u32,
                )
            };
            if replies == 0 {
                return None;
            }
            // SAFETY: one reply was written at the start of the buffer.
            let reply =
                unsafe { std::ptr::read_unaligned(reply.as_ptr().cast::<ICMP_ECHO_REPLY>()) };
            // IP_SUCCESS; anything else (unreachable, TTL expired) is no answer from `ip`.
            (reply.Status == 0).then(|| started.elapsed().as_secs_f32() * 1000.0)
        }
    }

    impl Drop for Pinger {
        fn drop(&mut self) {
            // SAFETY: the handle is ours and used no more.
            unsafe {
                let _ = IcmpCloseHandle(self.0);
            }
        }
    }

    pub fn route() -> Route {
        let mut destination = SOCKADDR_INET::default();
        let mut row = MIB_IPFORWARD_ROW2::default();
        let mut source = SOCKADDR_INET::default();
        // SAFETY: plain out-structures for one route lookup, then one interface lookup.
        unsafe {
            destination.Ipv4.sin_family = AF_INET;
            destination.Ipv4.sin_addr.S_un.S_addr = u32::from_ne_bytes([1, 1, 1, 1]);
            if GetBestRoute2(None, 0, None, &destination, 0, &mut row, &mut source) != NO_ERROR {
                return Route::default();
            }
            let hop = Ipv4Addr::from(row.NextHop.Ipv4.sin_addr.S_un.S_addr.to_ne_bytes());
            let mut interface = MIB_IF_ROW2 {
                InterfaceLuid: row.InterfaceLuid,
                ..Default::default()
            };
            let name = (GetIfEntry2(&mut interface) == NO_ERROR).then(|| {
                let end = interface.Alias.iter().position(|&unit| unit == 0);
                String::from_utf16_lossy(&interface.Alias[..end.unwrap_or(interface.Alias.len())])
            });
            Route {
                gateway: (!hop.is_unspecified()).then_some(hop),
                interface: name,
                online: true,
            }
        }
    }

    pub fn dns_servers() -> Vec<Ipv4Addr> {
        let mut size = 0u32;
        // SAFETY: a size query, then a buffer of that size, aligned for the structure, walked
        // along its own list.
        unsafe {
            let _ = GetNetworkParams(None, &mut size);
            if size == 0 {
                return Vec::new();
            }
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            let info = buffer.as_mut_ptr().cast::<FIXED_INFO_W2KSP1>();
            if GetNetworkParams(Some(info), &mut size) != NO_ERROR {
                return Vec::new();
            }
            let mut servers = Vec::new();
            let mut entry: *const IP_ADDR_STRING = &(*info).DnsServerList;
            while let Some(server) = entry.as_ref() {
                let text: Vec<u8> = server
                    .IpAddress
                    .String
                    .iter()
                    .take_while(|&&byte| byte != 0)
                    .map(|&byte| byte as u8)
                    .collect();
                if let Ok(ip) = String::from_utf8_lossy(&text).parse() {
                    servers.push(ip);
                }
                entry = server.Next;
            }
            servers
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::io::Read;
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::time::{Duration, Instant};

    use socket2::{Domain, Protocol, Socket, Type};

    use super::Route;

    /// An unprivileged ICMP socket (macOS lets any user send echoes this way).
    pub struct Pinger {
        socket: Socket,
        id: u16,
        sequence: u16,
    }

    impl Pinger {
        pub fn new() -> Option<Self> {
            let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).ok()?;
            static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(1);
            let id = (std::process::id() as u16)
                .wrapping_add(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
            Some(Pinger {
                socket,
                id,
                sequence: 0,
            })
        }

        pub fn ping(&mut self, ip: Ipv4Addr, timeout: Duration) -> Option<f32> {
            self.sequence = self.sequence.wrapping_add(1);
            let mut packet = [0u8; 8 + 32];
            packet[0] = 8; // echo request
            packet[4..6].copy_from_slice(&self.id.to_be_bytes());
            packet[6..8].copy_from_slice(&self.sequence.to_be_bytes());
            packet[8..].fill(0x5A);
            let sum = checksum(&packet);
            packet[2..4].copy_from_slice(&sum.to_be_bytes());
            let started = Instant::now();
            self.socket
                .send_to(&packet, &SocketAddrV4::new(ip, 0).into())
                .ok()?;
            loop {
                let left = timeout.checked_sub(started.elapsed())?;
                self.socket
                    .set_read_timeout(Some(left.max(Duration::from_millis(1))))
                    .ok()?;
                let mut buffer = [0u8; 256];
                let read = (&self.socket).read(&mut buffer).ok()?;
                let reply = &buffer[..read];
                // The kernel hands back the IP header too; the ICMP message follows it.
                let header = usize::from(reply.first()? & 0x0F) * 4;
                let icmp = reply.get(header..)?;
                if icmp.len() >= 8 && icmp[0] == 0 && icmp[6..8] == self.sequence.to_be_bytes() {
                    return Some(started.elapsed().as_secs_f32() * 1000.0);
                }
            }
        }
    }

    fn checksum(data: &[u8]) -> u16 {
        let mut sum: u32 = data
            .chunks(2)
            .map(|pair| u32::from(u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)])))
            .sum();
        while sum >> 16 != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        !(sum as u16)
    }

    /// `route -n get default`: the gateway and interface lines.
    pub fn route() -> Route {
        let Ok(output) = std::process::Command::new("/sbin/route")
            .args(["-n", "get", "default"])
            .output()
        else {
            return Route::default();
        };
        if !output.status.success() {
            return Route::default();
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let field = |name: &str| {
            text.lines()
                .find_map(|line| line.trim().strip_prefix(name))
                .map(|value| value.trim().to_string())
        };
        Route {
            gateway: field("gateway:").and_then(|value| value.parse().ok()),
            interface: field("interface:"),
            online: true,
        }
    }

    pub fn dns_servers() -> Vec<Ipv4Addr> {
        std::fs::read_to_string("/etc/resolv.conf")
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.trim().strip_prefix("nameserver"))
            .filter_map(|value| value.trim().parse().ok())
            .collect()
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use std::net::Ipv4Addr;
    use std::time::Duration;

    use super::Route;

    pub struct Pinger;

    impl Pinger {
        pub fn new() -> Option<Self> {
            None
        }

        pub fn ping(&mut self, _ip: Ipv4Addr, _timeout: Duration) -> Option<f32> {
            None
        }
    }

    pub fn route() -> Route {
        Route::default()
    }

    pub fn dns_servers() -> Vec<Ipv4Addr> {
        Vec::new()
    }
}

/// The time to open a TCP connection to `ip:443`, for when ICMP gets no answer: the
/// connection is dropped with a reset, so nothing lingers.
pub fn tcp_rtt(ip: Ipv4Addr, timeout: Duration) -> Option<f32> {
    use socket2::{Domain, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None).ok()?;
    let started = std::time::Instant::now();
    socket
        .connect_timeout(&std::net::SocketAddrV4::new(ip, 443).into(), timeout)
        .ok()?;
    let rtt = started.elapsed().as_secs_f32() * 1000.0;
    let _ = socket.set_linger(Some(Duration::ZERO));
    Some(rtt)
}

/// The time to resolve example.com through `server`, by a hand-built A query.
pub fn dns_rtt(server: Ipv4Addr, timeout: Duration) -> Option<f32> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.set_read_timeout(Some(timeout)).ok()?;
    let id = (std::process::id() as u16) ^ (crate::util::now_ms() as u16);
    let query = super::dns::query(id, "example.com");
    let started = std::time::Instant::now();
    socket.send_to(&query, (server, 53)).ok()?;
    let mut buffer = [0u8; 512];
    loop {
        let (read, _) = socket.recv_from(&mut buffer).ok()?;
        if super::dns::answers(&buffer[..read], id) {
            return Some(started.elapsed().as_secs_f32() * 1000.0);
        }
        if started.elapsed() >= timeout {
            return None;
        }
    }
}
