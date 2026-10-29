//! "port 3000", ":3000", "lsof 3000": what has a port open, and "ports" for everything
//! listening. Windows reads the TCP and UDP tables with their owning processes (no
//! administrator rights needed); macOS asks `lsof`, which sees the user's own processes.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

/// One open socket and the process behind it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortUse {
    pub port: u16,
    /// `TCP` or `UDP`.
    pub protocol: String,
    /// "127.0.0.1:3000", "[::]:3000", "*:5353".
    pub local: String,
    /// The other end of a connection; empty for a listener.
    pub remote: String,
    /// `LISTEN`, `ESTABLISHED`, …; empty for UDP.
    pub state: String,
    pub pid: u32,
    /// "node"; empty when the process can't be read.
    pub name: String,
    pub command: String,
    pub path: String,
    /// `app:<bundle>`, `app:exe:<path>` or a glyph.
    pub icon: String,
}

impl PortUse {
    pub fn listening(&self) -> bool {
        self.state == "LISTEN" || (self.protocol == "UDP" && self.remote.is_empty())
    }
}

/// Raw socket rows before the process is filled in.
#[derive(Debug, Clone, PartialEq)]
struct Socket {
    protocol: &'static str,
    local: IpAddr,
    port: u16,
    remote: Option<(IpAddr, u16)>,
    state: &'static str,
    pid: u32,
}

/// The typed text's port: "port 3000", ":3000", "lsof 3000", "lsof -i :3000", "what's on
/// port 3000". None when the text isn't a port question.
pub fn asked_port(text: &str) -> Option<u16> {
    let text = text.trim().to_lowercase();
    let rest = text
        .strip_prefix("lsof -i")
        .or_else(|| text.strip_prefix("lsof"))
        .or_else(|| text.strip_prefix("what's on port"))
        .or_else(|| text.strip_prefix("whats on port"))
        .or_else(|| text.strip_prefix("what is on port"))
        .or_else(|| text.strip_prefix("port"))
        .map(str::trim)
        .or_else(|| text.starts_with(':').then_some(text.as_str()))?;
    let digits = rest.strip_prefix(':').unwrap_or(rest).trim();
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u16>().ok().filter(|port| *port > 0)
}

/// "ports", "listening ports", "open ports".
pub fn asks_for_list(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "ports" | "listening ports" | "open ports" | "lsof" | "netstat"
    )
}

/// How long one read of the sockets and their processes answers for: typing ":3000" asks on
/// every keystroke, and a server that just started still shows within this.
const FRESH_FOR: Duration = Duration::from_secs(2);

static SOCKETS: Mutex<Option<(Instant, Vec<Socket>)>> = Mutex::new(None);

/// The sockets, read at most once per `FRESH_FOR`.
fn sockets_cached() -> Vec<Socket> {
    let Ok(mut cached) = SOCKETS.lock() else {
        return imp::sockets();
    };
    if let Some((at, sockets)) = cached.as_ref()
        && at.elapsed() < FRESH_FOR
    {
        return sockets.clone();
    }
    let sockets = imp::sockets();
    *cached = Some((Instant::now(), sockets.clone()));
    sockets
}

/// Everything using `port`, listeners first, one row per process and protocol.
pub fn on_port(port: u16) -> Vec<PortUse> {
    let sockets: Vec<Socket> = sockets_cached()
        .into_iter()
        .filter(|socket| socket.port == port && socket.pid != 0)
        .collect();
    // A server's own connections on its port are part of its listener.
    let listeners: Vec<(u32, &str)> = sockets
        .iter()
        .filter(|socket| socket.state == "LISTEN")
        .map(|socket| (socket.pid, socket.protocol))
        .collect();
    let sockets: Vec<Socket> = sockets
        .into_iter()
        .filter(|socket| {
            socket.state == "LISTEN" || !listeners.contains(&(socket.pid, socket.protocol))
        })
        .collect();
    let mut uses = described(dedupe(sockets, false));
    uses.sort_by_key(|item| (!item.listening(), item.protocol != "TCP", item.pid));
    uses
}

/// Every listening port, lowest first, one row per port, process and protocol.
pub fn listening() -> Vec<PortUse> {
    let sockets: Vec<Socket> = sockets_cached()
        .into_iter()
        // A UDP socket with a remote end is a client's connection (QUIC on macOS), not a
        // listener; Windows' UDP table never has one.
        .filter(|socket| {
            socket.pid != 0
                && (socket.state == "LISTEN"
                    || (socket.protocol == "UDP" && socket.remote.is_none()))
        })
        .collect();
    let mut uses = described(dedupe(sockets, true));
    uses.sort_by_key(|item| (item.protocol != "TCP", item.port, item.pid));
    uses
}

/// One row per process, protocol and port (and state): IPv4 and IPv6 listeners on the
/// same port read as one, and a server's many connections as its listener. `merge_states`
/// also folds every state together, for the list of listeners.
fn dedupe(sockets: Vec<Socket>, merge_states: bool) -> Vec<Socket> {
    let mut seen: HashMap<(u32, &'static str, u16, &'static str), Socket> = HashMap::new();
    for socket in sockets {
        let state = if merge_states { "" } else { socket.state };
        seen.entry((socket.pid, socket.protocol, socket.port, state))
            .and_modify(|kept| {
                // An address everyone can reach says more than loopback; IPv4 reads simpler.
                if kept.local.is_ipv6() && socket.local.is_ipv4() {
                    *kept = socket.clone();
                }
            })
            .or_insert(socket);
    }
    seen.into_values().collect()
}

fn address(ip: IpAddr, port: u16) -> String {
    match ip {
        IpAddr::V4(v4) if v4.is_unspecified() => format!("*:{port}"),
        IpAddr::V4(v4) => format!("{v4}:{port}"),
        IpAddr::V6(v6) if v6.is_unspecified() => format!("*:{port}"),
        IpAddr::V6(v6) => format!("[{v6}]:{port}"),
    }
}

/// A process's name, program and command line.
#[derive(Clone, Default)]
struct Described {
    name: String,
    path: String,
    command: String,
}

/// Processes already read, kept for `FRESH_FOR` like the sockets.
static PROCESSES: Mutex<Option<(Instant, HashMap<u32, Described>)>> = Mutex::new(None);

/// Each socket's process, reading only the ones not read in the last `FRESH_FOR`.
fn processes(pids: &[u32]) -> HashMap<u32, Described> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut guard = PROCESSES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard
        .as_ref()
        .is_none_or(|(at, _)| at.elapsed() >= FRESH_FOR)
    {
        *guard = Some((Instant::now(), HashMap::new()));
    }
    let Some((_, known)) = guard.as_mut() else {
        return HashMap::new();
    };
    // Each once: sysinfo loses a process that is asked for twice.
    let mut missing: Vec<Pid> = pids
        .iter()
        .filter(|pid| !known.contains_key(pid))
        .map(|pid| Pid::from_u32(*pid))
        .collect();
    missing.sort_unstable();
    missing.dedup();
    if !missing.is_empty() {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&missing),
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::Always)
                .with_cmd(UpdateKind::Always),
        );
        for pid in missing {
            let described = system
                .process(pid)
                .map(|process| {
                    let name = process.name().to_string_lossy().into_owned();
                    Described {
                        name: name
                            .strip_suffix(".exe")
                            .map(str::to_string)
                            .unwrap_or(name),
                        path: process
                            .exe()
                            .map(|path| path.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        command: process
                            .cmd()
                            .iter()
                            .map(|part| part.to_string_lossy().into_owned())
                            .collect::<Vec<_>>()
                            .join(" "),
                    }
                })
                .unwrap_or_default();
            known.insert(pid.as_u32(), described);
        }
    }
    pids.iter()
        .filter_map(|pid| known.get(pid).map(|described| (*pid, described.clone())))
        .collect()
}

/// Fills in each socket's process: its name, command line, program and icon.
fn described(sockets: Vec<Socket>) -> Vec<PortUse> {
    let pids: Vec<u32> = sockets.iter().map(|socket| socket.pid).collect();
    let processes = processes(&pids);
    sockets
        .into_iter()
        .map(|socket| {
            let process = processes.get(&socket.pid).cloned().unwrap_or_default();
            // Windows: 4 is the kernel, which owns http.sys and file sharing.
            let name = if process.name.is_empty() && socket.pid == 4 {
                "System".to_string()
            } else {
                process.name
            };
            PortUse {
                port: socket.port,
                protocol: socket.protocol.into(),
                local: address(socket.local, socket.port),
                remote: socket
                    .remote
                    .map(|(ip, port)| address(ip, port))
                    .unwrap_or_default(),
                state: socket.state.into(),
                pid: socket.pid,
                icon: icon_for(&process.path),
                name,
                command: process.command,
                path: process.path,
            }
        })
        .collect()
}

fn icon_for(path: &str) -> String {
    if path.is_empty() {
        return "glyph:terminal".into();
    }
    if cfg!(windows) {
        return format!("app:exe:{path}");
    }
    match crate::resources::app_bundle(std::path::Path::new(path)) {
        Some((bundle, _)) => format!("app:{bundle}"),
        None => "glyph:terminal".into(),
    }
}

#[cfg(windows)]
mod imp {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use windows::Win32::Foundation::NO_ERROR;
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
        MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

    use super::Socket;

    pub fn sockets() -> Vec<Socket> {
        let mut found = Vec::new();
        for row in table::<MIB_TCPROW_OWNER_PID>(|buffer, size| unsafe {
            GetExtendedTcpTable(
                buffer,
                size,
                false,
                AF_INET.0 as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        }) {
            found.push(Socket {
                protocol: "TCP",
                local: IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes())),
                port: port(row.dwLocalPort),
                remote: remote_v4(row.dwRemoteAddr, row.dwRemotePort),
                state: state(row.dwState),
                pid: row.dwOwningPid,
            });
        }
        for row in table::<MIB_TCP6ROW_OWNER_PID>(|buffer, size| unsafe {
            GetExtendedTcpTable(
                buffer,
                size,
                false,
                AF_INET6.0 as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            )
        }) {
            let remote = Ipv6Addr::from(row.ucRemoteAddr);
            found.push(Socket {
                protocol: "TCP",
                local: IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
                port: port(row.dwLocalPort),
                remote: (!remote.is_unspecified())
                    .then(|| (IpAddr::V6(remote), port(row.dwRemotePort))),
                state: state(row.dwState),
                pid: row.dwOwningPid,
            });
        }
        for row in table::<MIB_UDPROW_OWNER_PID>(|buffer, size| unsafe {
            GetExtendedUdpTable(
                buffer,
                size,
                false,
                AF_INET.0 as u32,
                UDP_TABLE_OWNER_PID,
                0,
            )
        }) {
            found.push(Socket {
                protocol: "UDP",
                local: IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes())),
                port: port(row.dwLocalPort),
                remote: None,
                state: "",
                pid: row.dwOwningPid,
            });
        }
        for row in table::<MIB_UDP6ROW_OWNER_PID>(|buffer, size| unsafe {
            GetExtendedUdpTable(
                buffer,
                size,
                false,
                AF_INET6.0 as u32,
                UDP_TABLE_OWNER_PID,
                0,
            )
        }) {
            found.push(Socket {
                protocol: "UDP",
                local: IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
                port: port(row.dwLocalPort),
                remote: None,
                state: "",
                pid: row.dwOwningPid,
            });
        }
        found
    }

    /// One of the extended tables: a count, then that many rows of `Row`. The table can grow
    /// between asking its size and reading it, so it's asked a few times.
    fn table<Row: Copy>(
        read: impl Fn(Option<*mut core::ffi::c_void>, *mut u32) -> u32,
    ) -> Vec<Row> {
        let mut size = 0u32;
        for _ in 0..4 {
            let _ = read(None, &mut size);
            if size == 0 {
                return Vec::new();
            }
            // Room for a few more rows, aligned for the structure.
            size += 16 * std::mem::size_of::<Row>() as u32;
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            let base = buffer.as_mut_ptr().cast::<u8>();
            if read(Some(base.cast()), &mut size) != NO_ERROR.0 {
                continue;
            }
            // SAFETY: the call filled the buffer with a u32 count and that many rows, which
            // start at the next offset the row's alignment allows.
            unsafe {
                let count = std::ptr::read_unaligned(base.cast::<u32>()) as usize;
                let offset = std::mem::align_of::<Row>().max(4);
                let rows = base.add(offset).cast::<Row>();
                return (0..count)
                    .map(|index| std::ptr::read_unaligned(rows.add(index)))
                    .collect();
            }
        }
        Vec::new()
    }

    /// Ports are kept in network byte order in the low 16 bits.
    fn port(raw: u32) -> u16 {
        u16::from_be(raw as u16)
    }

    fn remote_v4(address: u32, raw_port: u32) -> Option<(IpAddr, u16)> {
        let ip = Ipv4Addr::from(address.to_ne_bytes());
        (!ip.is_unspecified()).then(|| (IpAddr::V4(ip), port(raw_port)))
    }

    fn state(raw: u32) -> &'static str {
        match raw {
            1 => "CLOSED",
            2 => "LISTEN",
            3 => "SYN_SENT",
            4 => "SYN_RECEIVED",
            5 => "ESTABLISHED",
            6 => "FIN_WAIT_1",
            7 => "FIN_WAIT_2",
            8 => "CLOSE_WAIT",
            9 => "CLOSING",
            10 => "LAST_ACK",
            11 => "TIME_WAIT",
            _ => "DELETE_TCB",
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::net::IpAddr;

    use super::Socket;

    /// `lsof -nP -iTCP -iUDP -F pPtn`: per process a `p<pid>` line, then per socket its
    /// protocol (`P`), `n` name ("127.0.0.1:3000->10.0.0.2:51000" or "*:3000") and, for
    /// TCP, `TST=LISTEN`.
    pub fn sockets() -> Vec<Socket> {
        let Ok(output) = std::process::Command::new("/usr/sbin/lsof")
            .args(["-nP", "-iTCP", "-iUDP", "-FpPnT"])
            .output()
        else {
            return Vec::new();
        };
        parse(&String::from_utf8_lossy(&output.stdout))
    }

    pub(super) fn parse(text: &str) -> Vec<Socket> {
        let mut found = Vec::new();
        let mut pid = 0u32;
        let mut protocol = "TCP";
        let mut pending: Option<Socket> = None;
        for line in text.lines() {
            let (tag, value) = line.split_at(line.len().min(1));
            match tag {
                "p" => {
                    found.extend(pending.take());
                    pid = value.parse().unwrap_or(0);
                }
                "f" => found.extend(pending.take()),
                "P" => protocol = if value == "UDP" { "UDP" } else { "TCP" },
                "n" => {
                    found.extend(pending.take());
                    let (local, remote) = match value.split_once("->") {
                        Some((local, remote)) => (local, Some(remote)),
                        None => (value, None),
                    };
                    if let Some((ip, port)) = endpoint(local) {
                        pending = Some(Socket {
                            protocol,
                            local: ip,
                            port,
                            remote: remote.and_then(endpoint),
                            state: if protocol == "UDP" { "" } else { "ESTABLISHED" },
                            pid,
                        });
                    }
                }
                "T" => {
                    if let (Some(state), Some(socket)) =
                        (value.strip_prefix("ST="), pending.as_mut())
                    {
                        socket.state = match state {
                            "LISTEN" => "LISTEN",
                            "ESTABLISHED" => "ESTABLISHED",
                            "CLOSE_WAIT" => "CLOSE_WAIT",
                            "TIME_WAIT" => "TIME_WAIT",
                            "SYN_SENT" => "SYN_SENT",
                            "CLOSED" => "CLOSED",
                            _ => "OTHER",
                        };
                    }
                }
                _ => {}
            }
        }
        found.extend(pending);
        found
    }

    /// "127.0.0.1:3000", "[::1]:3000", "*:3000".
    fn endpoint(text: &str) -> Option<(IpAddr, u16)> {
        let (host, port) = text.rsplit_once(':')?;
        let port = port.parse().ok()?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let ip = if host == "*" {
            IpAddr::from([0, 0, 0, 0])
        } else {
            host.parse().ok()?
        };
        Some((ip, port))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::Socket;

    pub fn sockets() -> Vec<Socket> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_port_questions() {
        for (text, port) in [
            ("port 3000", 3000),
            (":3000", 3000),
            ("lsof 3000", 3000),
            ("lsof -i :8080", 8080),
            ("Port :5173", 5173),
            ("what's on port 443", 443),
        ] {
            assert_eq!(asked_port(text), Some(port), "{text}");
        }
        for text in [
            "port",
            "ports",
            "port 99999",
            "portal",
            ":abc",
            "3000",
            "port 0",
        ] {
            assert_eq!(asked_port(text), None, "{text}");
        }
        assert!(asks_for_list("ports"));
        assert!(!asks_for_list("port"));
    }

    #[test]
    fn one_row_per_process_and_port() {
        let socket = |local: &str, state: &'static str| Socket {
            protocol: "TCP",
            local: local.parse().unwrap(),
            port: 3000,
            remote: None,
            state,
            pid: 42,
        };
        let rows = dedupe(
            vec![socket("::", "LISTEN"), socket("0.0.0.0", "LISTEN")],
            false,
        );
        assert_eq!(rows.len(), 1);
        assert!(rows[0].local.is_ipv4());
        let rows = dedupe(
            vec![socket("::", "LISTEN"), socket("127.0.0.1", "ESTABLISHED")],
            false,
        );
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn names_addresses_as_netstat_does() {
        assert_eq!(address("0.0.0.0".parse().unwrap(), 80), "*:80");
        assert_eq!(address("::1".parse().unwrap(), 80), "[::1]:80");
        assert_eq!(address("127.0.0.1".parse().unwrap(), 80), "127.0.0.1:80");
    }

    /// Run by hand: `cargo test sockets_on_this_computer -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn sockets_on_this_computer() {
        for item in listening().iter().take(15) {
            println!("{item:?}");
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod mac_tests {
    #[test]
    fn reads_lsof_fields() {
        let text = "p501\nf23\nPTCP\nn*:3000\nTST=LISTEN\nTQR=0\nf24\nPTCP\nn127.0.0.1:3000->127.0.0.1:51000\nTST=ESTABLISHED\np77\nf5\nPUDP\nn*:5353\n";
        let found = super::imp::parse(text);
        assert_eq!(found.len(), 3);
        assert_eq!(
            (found[0].pid, found[0].port, found[0].state),
            (501, 3000, "LISTEN")
        );
        assert_eq!(found[1].remote.map(|(_, port)| port), Some(51000));
        assert_eq!((found[2].pid, found[2].protocol), (77, "UDP"));
    }
}
