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

pub use imp::{Pinger, RouteWatch, dns_servers, route};

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

    /// Reading the route is a cheap call here, so it's read every time.
    pub struct RouteWatch;

    impl RouteWatch {
        pub fn new() -> Self {
            RouteWatch
        }

        pub fn changed(&mut self) -> bool {
            true
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
                // Every ICMP socket in the process sees every echo reply, and the router and
                // internet pingers send the same sequence numbers in step, so a reply counts
                // only when it's from the address asked, with this pinger's id: the router's
                // quick answer used to be taken for the internet's.
                let from = reply.get(12..16)?;
                if icmp.len() >= 8
                    && icmp[0] == 0
                    && from == ip.octets()
                    && icmp[4..6] == self.id.to_be_bytes()
                    && icmp[6..8] == self.sequence.to_be_bytes()
                {
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
        super::parse_route(&String::from_utf8_lossy(&output.stdout))
    }

    pub fn dns_servers() -> Vec<Ipv4Addr> {
        super::parse_resolv_conf(&std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default())
    }

    /// Listens on a routing socket, so `route` (a process each time) only runs again when the
    /// kernel says a route or address changed, and every few minutes besides.
    pub struct RouteWatch {
        socket: Option<Socket>,
        checked: Option<Instant>,
    }

    impl RouteWatch {
        const ANYWAY: Duration = Duration::from_secs(5 * 60);

        pub fn new() -> Self {
            let socket = Socket::new_raw(
                Domain::from(libc::AF_ROUTE),
                Type::from(libc::SOCK_RAW),
                None,
            )
            .and_then(|socket| socket.set_nonblocking(true).map(|()| socket));
            if let Err(err) = &socket {
                log::warn!(
                    "network monitor: no routing socket, reading the route each time: {err}"
                );
            }
            RouteWatch {
                socket: socket.ok(),
                checked: None,
            }
        }

        /// Whether the route may have changed since last asked.
        pub fn changed(&mut self) -> bool {
            let mut changed = self.checked.is_none_or(|at| at.elapsed() >= Self::ANYWAY);
            match &self.socket {
                Some(socket) => {
                    let mut socket: &Socket = socket;
                    let mut buffer = [0u8; 2048];
                    // Drained every time, so messages don't pile up between checks.
                    while let Ok(read) = socket.read(&mut buffer) {
                        if read == 0 {
                            break;
                        }
                        changed |= super::route_message_matters(&buffer[..read]);
                    }
                }
                None => changed = true,
            }
            if changed {
                self.checked = Some(Instant::now());
            }
            changed
        }
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

    pub struct RouteWatch;

    impl RouteWatch {
        pub fn new() -> Self {
            RouteWatch
        }

        pub fn changed(&mut self) -> bool {
            true
        }
    }
}

/// `route -n get default`'s output: the gateway and interface lines.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_route(text: &str) -> Route {
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

/// The IPv4 `nameserver` lines of a resolv.conf.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_resolv_conf(text: &str) -> Vec<Ipv4Addr> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("nameserver"))
        .filter_map(|value| value.trim().parse().ok())
        .collect()
}

/// Whether a BSD routing-socket message could move the default route: route additions,
/// removals and changes (but not the per-host entries ARP and neighbour discovery churn
/// through), interfaces going up or down, and addresses coming and going.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn route_message_matters(message: &[u8]) -> bool {
    const RTM_ADD: u8 = 0x1;
    const RTM_DELETE: u8 = 0x2;
    const RTM_CHANGE: u8 = 0x3;
    const RTM_NEWADDR: u8 = 0xc;
    const RTM_DELADDR: u8 = 0xd;
    const RTM_IFINFO: u8 = 0xe;
    const RTF_HOST: i32 = 0x4;
    // rt_msghdr: u16 length, u8 version, u8 type, u16 index, then i32 flags at offset 8.
    let Some(&kind) = message.get(3) else {
        return false;
    };
    match kind {
        RTM_ADD | RTM_DELETE | RTM_CHANGE => message
            .get(8..12)
            .is_some_and(|flags| i32::from_ne_bytes(flags.try_into().unwrap()) & RTF_HOST == 0),
        RTM_NEWADDR | RTM_DELADDR | RTM_IFINFO => true,
        _ => false,
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
    let id = (std::process::id() as u16) ^ (crate::util::now_ms() as u16);
    let query = super::dns::query(id, "example.com");
    let started = std::time::Instant::now();
    socket.send_to(&query, (server, 53)).ok()?;
    let mut buffer = [0u8; 512];
    loop {
        // One deadline for the whole lookup, however many stray packets arrive meanwhile.
        let left = timeout.checked_sub(started.elapsed())?;
        socket
            .set_read_timeout(Some(left.max(Duration::from_millis(1))))
            .ok()?;
        let (read, from) = socket.recv_from(&mut buffer).ok()?;
        if from == std::net::SocketAddr::from((server, 53))
            && super::dns::answers(&buffer[..read], id)
        {
            return Some(started.elapsed().as_secs_f32() * 1000.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_get_default_gives_the_gateway_and_interface() {
        let text = "   route to: default
destination: default
       mask: default
    gateway: 192.168.1.1
  interface: en0
      flags: <UP,GATEWAY,DONE,STATIC,PRCLONING,GLOBAL>
 recvpipe  sendpipe  ssthresh  rtt,msec    rttvar  hopcount      mtu     expire
       0         0         0         0         0         0      1500         0
";
        assert_eq!(
            parse_route(text),
            Route {
                gateway: Some(Ipv4Addr::new(192, 168, 1, 1)),
                interface: Some("en0".into()),
                online: true,
            }
        );
        // A VPN's route names a link, not an address: no router, but still online.
        let tunnel = parse_route("    gateway: utun3\n  interface: utun3\n");
        assert_eq!(tunnel.gateway, None);
        assert_eq!(tunnel.interface.as_deref(), Some("utun3"));
    }

    #[test]
    fn resolv_conf_gives_the_ipv4_nameservers() {
        let text = "#\n# macOS Notice\n#\nsearch lan\nnameserver 192.168.1.1\nnameserver fe80::1%en0\n  nameserver\t1.1.1.1\n";
        assert_eq!(
            parse_resolv_conf(text),
            [Ipv4Addr::new(192, 168, 1, 1), Ipv4Addr::new(1, 1, 1, 1)]
        );
        assert!(parse_resolv_conf("").is_empty());
    }

    #[test]
    fn only_route_messages_that_could_move_the_default_count() {
        let message = |kind: u8, flags: i32| {
            let mut bytes = vec![0u8; 92];
            bytes[3] = kind;
            bytes[8..12].copy_from_slice(&flags.to_ne_bytes());
            bytes
        };
        assert!(route_message_matters(&message(0x1, 0x1 | 0x2)));
        assert!(route_message_matters(&message(0x2, 0x2)));
        assert!(route_message_matters(&message(0xe, 0)));
        // An ARP entry for some host on the network.
        assert!(!route_message_matters(&message(0x1, 0x4 | 0x400)));
        // RTM_MISS, and a message cut short.
        assert!(!route_message_matters(&message(0x7, 0)));
        assert!(!route_message_matters(&[0, 0]));
    }
}
