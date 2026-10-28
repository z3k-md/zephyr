//! CPU and GPU temperatures in °C, for System Resources and the footer. macOS reads them from
//! the System Management Controller through IOKit, which any app may do: no entitlement, no
//! administrator rights. Elsewhere there's no reading yet (Windows needs a driver, opt-in).

/// The hottest CPU and GPU sensors right now; none for one that can't be read.
pub fn read() -> (Option<f32>, Option<f32>) {
    imp::read()
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{c_char, c_void};
    use std::sync::{Mutex, OnceLock};

    /// Which sensors count: Apple silicon names its CPU cores' `Tp…` (performance) and
    /// `Te…` (efficiency) and its GPU's `Tg…`; Intel Macs use `TC…` and `TG…`.
    const CPU: [&[u8; 2]; 3] = [b"Tp", b"Te", b"TC"];
    const GPU: [&[u8; 2]; 2] = [b"Tg", b"TG"];
    /// Readings outside this are sensors that aren't there (zeros) or not temperatures.
    const PLAUSIBLE: std::ops::Range<f32> = 5.0..130.0;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Version {
        major: u8,
        minor: u8,
        build: u8,
        reserved: u8,
        release: u16,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct PowerLimits {
        version: u16,
        length: u16,
        cpu: u32,
        gpu: u32,
        memory: u32,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct KeyInfo {
        size: u32,
        kind: u32,
        attributes: u8,
    }

    /// The SMC's request and reply, as AppleSMC's user client lays it out (80 bytes).
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Message {
        key: u32,
        version: Version,
        limits: PowerLimits,
        info: KeyInfo,
        result: u8,
        status: u8,
        command: u8,
        index: u32,
        bytes: [u8; 32],
    }

    const HANDLE_EVENT: u32 = 2;
    const READ_KEY: u8 = 5;
    const KEY_AT_INDEX: u8 = 8;
    const KEY_INFO: u8 = 9;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *const c_void;
        fn IOServiceGetMatchingService(main_port: u32, matching: *const c_void) -> u32;
        fn IOServiceOpen(service: u32, task: u32, kind: u32, connect: *mut u32) -> i32;
        fn IOObjectRelease(object: u32) -> i32;
        fn IOConnectCallStructMethod(
            connection: u32,
            selector: u32,
            input: *const c_void,
            input_size: usize,
            output: *mut c_void,
            output_size: *mut usize,
        ) -> i32;
    }

    struct Smc {
        connection: u32,
        /// The temperature keys found once, with each one's size and type.
        cpu: Vec<(u32, KeyInfo)>,
        gpu: Vec<(u32, KeyInfo)>,
    }

    impl Smc {
        fn open() -> Option<Smc> {
            // SAFETY: plain IOKit calls with valid arguments; the matching dictionary is
            // consumed by IOServiceGetMatchingService.
            unsafe {
                let service =
                    IOServiceGetMatchingService(0, IOServiceMatching(c"AppleSMC".as_ptr()));
                if service == 0 {
                    return None;
                }
                let mut connection = 0;
                #[allow(deprecated)]
                let task = libc::mach_task_self();
                let opened = IOServiceOpen(service, task, 0, &mut connection);
                IOObjectRelease(service);
                if opened != 0 {
                    return None;
                }
                let mut smc = Smc {
                    connection,
                    cpu: Vec::new(),
                    gpu: Vec::new(),
                };
                smc.find_sensors();
                Some(smc)
            }
        }

        fn call(&self, request: &Message) -> Option<Message> {
            let mut reply = Message::default();
            let mut size = std::mem::size_of::<Message>();
            // SAFETY: both buffers are Messages, the layout the user client expects.
            let result = unsafe {
                IOConnectCallStructMethod(
                    self.connection,
                    HANDLE_EVENT,
                    (request as *const Message).cast(),
                    std::mem::size_of::<Message>(),
                    (&mut reply as *mut Message).cast(),
                    &mut size,
                )
            };
            (result == 0 && reply.result == 0).then_some(reply)
        }

        fn info(&self, key: u32) -> Option<KeyInfo> {
            let request = Message {
                key,
                command: KEY_INFO,
                ..Default::default()
            };
            self.call(&request).map(|reply| reply.info)
        }

        fn value(&self, key: u32, info: KeyInfo) -> Option<f32> {
            let request = Message {
                key,
                info,
                command: READ_KEY,
                ..Default::default()
            };
            let reply = self.call(&request)?;
            let bytes = &reply.bytes;
            let value = match &info.kind.to_be_bytes() {
                b"flt " => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
                // Signed fixed point with 8 fraction bits, big-endian.
                b"sp78" => f32::from(i16::from_be_bytes([bytes[0], bytes[1]])) / 256.0,
                _ => return None,
            };
            PLAUSIBLE.contains(&value).then_some(value)
        }

        /// Walks every key the SMC has once and keeps the temperature sensors that answer.
        fn find_sensors(&mut self) {
            let count_key = u32::from_be_bytes(*b"#KEY");
            let Some(info) = self.info(count_key) else {
                return;
            };
            let request = Message {
                key: count_key,
                info,
                command: READ_KEY,
                ..Default::default()
            };
            let Some(reply) = self.call(&request) else {
                return;
            };
            let count = u32::from_be_bytes([
                reply.bytes[0],
                reply.bytes[1],
                reply.bytes[2],
                reply.bytes[3],
            ]);
            for index in 0..count.min(4096) {
                let request = Message {
                    command: KEY_AT_INDEX,
                    index,
                    ..Default::default()
                };
                let Some(reply) = self.call(&request) else {
                    continue;
                };
                let name = reply.key.to_be_bytes();
                let prefix = [name[0], name[1]];
                let cpu = CPU.iter().any(|wanted| **wanted == prefix);
                let gpu = GPU.iter().any(|wanted| **wanted == prefix);
                if !cpu && !gpu {
                    continue;
                }
                let Some(info) = self.info(reply.key) else {
                    continue;
                };
                if self.value(reply.key, info).is_none() {
                    continue;
                }
                if cpu {
                    self.cpu.push((reply.key, info));
                } else {
                    self.gpu.push((reply.key, info));
                }
            }
        }

        fn hottest(&self, sensors: &[(u32, KeyInfo)]) -> Option<f32> {
            sensors
                .iter()
                .filter_map(|(key, info)| self.value(*key, *info))
                .reduce(f32::max)
        }
    }

    static SMC: OnceLock<Option<Mutex<Smc>>> = OnceLock::new();

    pub fn read() -> (Option<f32>, Option<f32>) {
        let Some(smc) = SMC.get_or_init(|| Smc::open().map(Mutex::new)) else {
            return (None, None);
        };
        let Ok(smc) = smc.lock() else {
            return (None, None);
        };
        (smc.hottest(&smc.cpu), smc.hottest(&smc.gpu))
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn the_message_is_laid_out_as_the_smc_expects() {
            assert_eq!(std::mem::size_of::<super::Message>(), 80);
        }

        #[test]
        #[ignore]
        fn temperatures_here() {
            let started = std::time::Instant::now();
            let first = super::read();
            println!("first read {first:?} in {:?}", started.elapsed());
            let started = std::time::Instant::now();
            let second = super::read();
            println!("then {second:?} in {:?}", started.elapsed());
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn read() -> (Option<f32>, Option<f32>) {
        (None, None)
    }
}
