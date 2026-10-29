//! GPU load and memory for the footer and System Resources, read without admin rights or a
//! vendor's tools. Windows reads the counters Task Manager shows, which every vendor's driver
//! fills in; macOS asks IOKit for each GPU's own performance statistics.

use serde::Serialize;

pub use platform::Reader;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gpu {
    /// The GPU's name ("NVIDIA GeForce RTX 4070"); empty where it isn't known.
    pub name: String,
    /// The busiest engine on any GPU, 0–100, as Task Manager counts it.
    pub usage: f32,
    /// Memory the main GPU is using and has of its own. The total is 0 where the GPU shares
    /// the computer's memory (Apple silicon) or it can't be read.
    pub memory_used: u64,
    pub memory_total: u64,
    /// The main GPU's temperature in °C, where its driver reports one.
    #[serde(skip)]
    pub temperature: Option<f32>,
}

/// Splits a `\GPU Engine(*)` instance name, `pid_1234_luid_0x…_0x…_phys_0_eng_3_engtype_3D`,
/// into the process it counts for and the engine (`0x…_0x…_phys_0_eng_3_engtype_3D`).
#[cfg_attr(not(windows), allow(dead_code))]
fn engine_instance(name: &str) -> (Option<u32>, &str) {
    let (owner, engine) = name.split_once("_luid_").unwrap_or(("", name));
    let pid = owner
        .strip_prefix("pid_")
        .and_then(|pid| pid.split('_').next())
        .and_then(|pid| pid.parse().ok());
    (pid, engine)
}

#[cfg(windows)]
mod platform {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    use windows::Wdk::Graphics::Direct3D::{
        D3DKMT_ADAPTER_PERFDATA, D3DKMT_CLOSEADAPTER, D3DKMT_OPENADAPTERFROMLUID,
        D3DKMT_QUERYADAPTERINFO, D3DKMTCloseAdapter, D3DKMTOpenAdapterFromLuid,
        D3DKMTQueryAdapterInfo, KMTQAITYPE_ADAPTERPERFDATA,
    };
    use windows::Win32::Foundation::LUID;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
    };
    use windows::Win32::System::Performance::{
        PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW,
    };
    use windows::core::w;

    use super::Gpu;

    pub struct Reader {
        query: PDH_HQUERY,
        /// Per process, per engine: `pid_…_luid_…_phys_0_eng_3_engtype_3D`.
        engines: PDH_HCOUNTER,
        /// Per GPU: `luid_…_phys_0`.
        memory: PDH_HCOUNTER,
        /// The GPU with the most memory of its own, whose memory the reading shows.
        main: Option<Adapter>,
        main_checked: Instant,
        /// Each process's load from the last reading: its busiest engine, as Task Manager
        /// shows it.
        by_process: HashMap<u32, f32>,
    }

    const MAIN_RECHECK: Duration = Duration::from_secs(10 * 60);
    const MAIN_RECHECK_MISSING: Duration = Duration::from_secs(30);

    struct Adapter {
        name: String,
        memory: u64,
        /// How the counters name it, lowercased: `luid_0x00000000_0x0000cd88`.
        luid: String,
        /// The kernel graphics handle its temperature is read through, as Task Manager's is;
        /// none where it couldn't be opened.
        kmt: Option<u32>,
    }

    impl Drop for Adapter {
        fn drop(&mut self) {
            if let Some(handle) = self.kmt {
                let close = D3DKMT_CLOSEADAPTER { hAdapter: handle };
                // SAFETY: a handle this adapter opened and nothing else uses.
                unsafe {
                    let _ = D3DKMTCloseAdapter(&close);
                }
            }
        }
    }

    /// The GPU's temperature as the driver reports it to Windows (in tenths of a degree),
    /// for vendors that do; 0 means it doesn't.
    fn temperature(handle: u32) -> Option<f32> {
        let mut data = D3DKMT_ADAPTER_PERFDATA::default();
        let mut query = D3DKMT_QUERYADAPTERINFO {
            hAdapter: handle,
            Type: KMTQAITYPE_ADAPTERPERFDATA,
            pPrivateDriverData: (&mut data as *mut D3DKMT_ADAPTER_PERFDATA).cast(),
            PrivateDriverDataSize: std::mem::size_of::<D3DKMT_ADAPTER_PERFDATA>() as u32,
        };
        // SAFETY: the query points at a perf-data structure of the size it says.
        let status = unsafe { D3DKMTQueryAdapterInfo(&mut query) };
        (status.is_ok() && data.Temperature > 0).then(|| data.Temperature as f32 / 10.0)
    }

    // SAFETY: PDH handles can be used from any thread, and the reader is only ever used
    // behind the stats sampler's lock.
    unsafe impl Send for Reader {}

    impl Reader {
        /// None on a computer without GPU counters (no GPU, or a driver too old to report).
        pub fn new() -> Option<Self> {
            let mut query = PDH_HQUERY::default();
            let mut engines = PDH_HCOUNTER::default();
            let mut memory = PDH_HCOUNTER::default();
            // SAFETY: out-pointers to locals, and counter paths as wide string literals.
            unsafe {
                if PdhOpenQueryW(None, 0, &mut query) != 0 {
                    return None;
                }
                if PdhAddEnglishCounterW(
                    query,
                    w!("\\GPU Engine(*)\\Utilization Percentage"),
                    0,
                    &mut engines,
                ) != 0
                {
                    PdhCloseQuery(query);
                    return None;
                }
                let _ = PdhAddEnglishCounterW(
                    query,
                    w!("\\GPU Adapter Memory(*)\\Dedicated Usage"),
                    0,
                    &mut memory,
                );
                // Utilization is a rate, so the first collection is only its starting point.
                PdhCollectQueryData(query);
            }
            Some(Self {
                query,
                engines,
                memory,
                main: main_adapter(),
                main_checked: Instant::now(),
                by_process: HashMap::new(),
            })
        }

        /// Each process's GPU load from the last reading that asked for it, 0–100, handed
        /// over rather than copied.
        pub fn take_by_process(&mut self) -> HashMap<u32, f32> {
            std::mem::take(&mut self.by_process)
        }

        /// `per_process` also works out each process's load, for Resources.
        pub fn read(&mut self, per_process: bool) -> Option<Gpu> {
            // SAFETY: the query stays open for the reader's life.
            if unsafe { PdhCollectQueryData(self.query) } != 0 {
                return None;
            }
            // An engine's load is the sum of what each process uses of it; the GPU's is its
            // busiest engine's.
            let mut engines: HashMap<&str, f64> = HashMap::new();
            let mut by_engine: HashMap<(u32, &str), f64> = HashMap::new();
            let instances = values(self.engines);
            for (name, value) in &instances {
                let (pid, engine) = super::engine_instance(name);
                *engines.entry(engine).or_default() += value;
                if per_process && let Some(pid) = pid {
                    *by_engine.entry((pid, engine)).or_default() += value;
                }
            }
            if per_process {
                self.by_process.clear();
                for ((pid, _), value) in by_engine {
                    let busiest = self.by_process.entry(pid).or_default();
                    *busiest = busiest.max(value.min(100.0) as f32);
                }
            }
            let usage = engines.values().copied().fold(0.0, f64::max).min(100.0) as f32;
            let mut gpu = Gpu {
                usage,
                ..Gpu::default()
            };
            // The main GPU is looked for again now and then, and sooner when it's missing or
            // its counters are gone (an eGPU unplugged, a driver reinstalled).
            let memory = values(self.memory);
            let found = self.main.as_ref().is_some_and(|main| {
                memory
                    .iter()
                    .any(|(name, _)| name.to_ascii_lowercase().starts_with(&main.luid))
            });
            let recheck = if found {
                MAIN_RECHECK
            } else {
                MAIN_RECHECK_MISSING
            };
            if self.main_checked.elapsed() >= recheck {
                self.main = main_adapter();
                self.main_checked = Instant::now();
            }
            if let Some(main) = &self.main {
                gpu.name = main.name.clone();
                gpu.memory_total = main.memory;
                gpu.memory_used = memory
                    .into_iter()
                    .find(|(name, _)| name.to_ascii_lowercase().starts_with(&main.luid))
                    .map_or(0, |(_, used)| used as u64);
                gpu.temperature = main.kmt.and_then(temperature);
            }
            Some(gpu)
        }
    }

    impl Drop for Reader {
        fn drop(&mut self) {
            // SAFETY: the query is open, and nothing uses it after this.
            unsafe {
                PdhCloseQuery(self.query);
            }
        }
    }

    /// Every instance of a wildcard counter, by name, from the last collection.
    fn values(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
        // A process starting between asking the size and reading can grow the list, so the
        // size is asked again then.
        for _ in 0..3 {
            let mut size = 0u32;
            let mut count = 0u32;
            // SAFETY: first a size query without a buffer, then a buffer of that size, aligned
            // for the items, read only as far as the count PDH filled in.
            unsafe {
                if PdhGetFormattedCounterArrayW(
                    counter,
                    PDH_FMT_DOUBLE,
                    &mut size,
                    &mut count,
                    None,
                ) != PDH_MORE_DATA
                {
                    return Vec::new();
                }
                let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
                let items = buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
                match PdhGetFormattedCounterArrayW(
                    counter,
                    PDH_FMT_DOUBLE,
                    &mut size,
                    &mut count,
                    Some(items),
                ) {
                    0 => {}
                    PDH_MORE_DATA => continue,
                    _ => return Vec::new(),
                }
                return std::slice::from_raw_parts(items, count as usize)
                    .iter()
                    // Valid data, or new data (an instance seen for the first time).
                    .filter(|item| item.FmtValue.CStatus <= 1)
                    .map(|item| {
                        (
                            item.szName.to_string().unwrap_or_default(),
                            item.FmtValue.Anonymous.doubleValue,
                        )
                    })
                    .collect();
            }
        }
        Vec::new()
    }

    /// The hardware GPU with the most memory of its own: the discrete one, when there are two.
    fn main_adapter() -> Option<Adapter> {
        // SAFETY: plain DXGI enumeration; each adapter is released as it goes out of scope.
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            let mut best: Option<Adapter> = None;
            let mut best_luid = LUID::default();
            for index in 0.. {
                let Ok(adapter) = factory.EnumAdapters1(index) else {
                    break;
                };
                let Ok(desc) = adapter.GetDesc1() else {
                    continue;
                };
                let memory = desc.DedicatedVideoMemory as u64;
                if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0
                    || best.as_ref().is_some_and(|best| best.memory >= memory)
                {
                    continue;
                }
                let end = desc.Description.iter().position(|&unit| unit == 0);
                let name = String::from_utf16_lossy(&desc.Description[..end.unwrap_or(128)]);
                best = Some(Adapter {
                    name: name.trim().to_string(),
                    memory,
                    luid: format!(
                        "luid_0x{:08x}_0x{:08x}",
                        desc.AdapterLuid.HighPart as u32, desc.AdapterLuid.LowPart
                    ),
                    kmt: None,
                });
                best_luid = desc.AdapterLuid;
            }
            if let Some(adapter) = best.as_mut() {
                let mut open = D3DKMT_OPENADAPTERFROMLUID {
                    AdapterLuid: best_luid,
                    hAdapter: 0,
                };
                if D3DKMTOpenAdapterFromLuid(&mut open).is_ok() {
                    adapter.kmt = Some(open.hAdapter);
                }
            }
            best
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::{CStr, c_char, c_void};

    use super::Gpu;

    type CFTypeRef = *const c_void;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOServiceMatching(name: *const c_char) -> CFTypeRef;
        fn IOServiceGetMatchingServices(
            main_port: u32,
            matching: CFTypeRef,
            iterator: *mut u32,
        ) -> i32;
        fn IOIteratorNext(iterator: u32) -> u32;
        fn IOObjectRelease(object: u32) -> i32;
        fn IORegistryEntryCreateCFProperty(
            entry: u32,
            key: CFTypeRef,
            allocator: CFTypeRef,
            options: u32,
        ) -> CFTypeRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            allocator: CFTypeRef,
            text: *const c_char,
            encoding: u32,
        ) -> CFTypeRef;
        fn CFDictionaryGetValue(dictionary: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
        fn CFNumberGetValue(number: CFTypeRef, kind: isize, value: *mut c_void) -> u8;
        fn CFGetTypeID(object: CFTypeRef) -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFDictionaryGetTypeID() -> usize;
        fn CFRelease(object: CFTypeRef);
    }

    const UTF8: u32 = 0x0800_0100;
    const SINT64: isize = 4;

    /// Nothing to keep between reads: each walks the GPUs IOKit lists, a handful at most.
    pub struct Reader;

    impl Reader {
        pub fn new() -> Option<Self> {
            Some(Reader)
        }

        /// No per-process GPU load here.
        pub fn take_by_process(&mut self) -> std::collections::HashMap<u32, f32> {
            std::collections::HashMap::new()
        }

        /// The busiest GPU, as Activity Monitor's GPU History shows it.
        pub fn read(&mut self, _per_process: bool) -> Option<Gpu> {
            let mut busiest: Option<Gpu> = None;
            // SAFETY: IOKit and CoreFoundation calls on objects this function creates and
            // releases; the matching dictionary is consumed by IOServiceGetMatchingServices.
            unsafe {
                let matching = IOServiceMatching(c"IOAccelerator".as_ptr());
                if matching.is_null() {
                    return None;
                }
                let mut iterator = 0;
                if IOServiceGetMatchingServices(0, matching, &mut iterator) != 0 {
                    return None;
                }
                loop {
                    let service = IOIteratorNext(iterator);
                    if service == 0 {
                        break;
                    }
                    if let Some(gpu) = read_gpu(service)
                        && busiest.as_ref().is_none_or(|best| gpu.usage > best.usage)
                    {
                        busiest = Some(gpu);
                    }
                    IOObjectRelease(service);
                }
                IOObjectRelease(iterator);
            }
            busiest
        }
    }

    /// One GPU's load and memory from its performance statistics. Apple silicon and AMD say
    /// "Device Utilization %", Intel "GPU Activity(%)".
    unsafe fn read_gpu(service: u32) -> Option<Gpu> {
        // SAFETY: the caller's live service; every object created here is released here.
        unsafe {
            let stats = property(service, c"PerformanceStatistics");
            if stats.is_null() {
                return None;
            }
            let found = if CFGetTypeID(stats) == CFDictionaryGetTypeID() {
                let usage = number_in(stats, c"Device Utilization %")
                    .or_else(|| number_in(stats, c"GPU Activity(%)"));
                usage.map(|usage| Gpu {
                    name: String::new(),
                    usage: usage.clamp(0, 100) as f32,
                    memory_used: number_in(stats, c"In use system memory")
                        .or_else(|| number_in(stats, c"vramUsedBytes"))
                        .unwrap_or(0)
                        .max(0) as u64,
                    memory_total: vram_total(service),
                    temperature: None,
                })
            } else {
                None
            };
            CFRelease(stats);
            found
        }
    }

    /// A discrete GPU's own memory; 0 for Apple silicon, which shares the computer's.
    unsafe fn vram_total(service: u32) -> u64 {
        // SAFETY: the caller's live service; the property is released here.
        unsafe {
            let total = property(service, c"VRAM,totalMB");
            if total.is_null() {
                return 0;
            }
            let megabytes = number(total).unwrap_or(0).max(0) as u64;
            CFRelease(total);
            megabytes * 1024 * 1024
        }
    }

    unsafe fn property(service: u32, key: &CStr) -> CFTypeRef {
        // SAFETY: a CFString made and released here, and the caller's live service.
        unsafe {
            let key = CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), UTF8);
            if key.is_null() {
                return std::ptr::null();
            }
            let value = IORegistryEntryCreateCFProperty(service, key, std::ptr::null(), 0);
            CFRelease(key);
            value
        }
    }

    unsafe fn number_in(dictionary: CFTypeRef, key: &CStr) -> Option<i64> {
        // SAFETY: the caller's live dictionary; the value it returns isn't owned, so it isn't
        // released.
        unsafe {
            let key = CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), UTF8);
            if key.is_null() {
                return None;
            }
            let value = CFDictionaryGetValue(dictionary, key);
            CFRelease(key);
            if value.is_null() { None } else { number(value) }
        }
    }

    unsafe fn number(value: CFTypeRef) -> Option<i64> {
        // SAFETY: the caller's live object, checked to be a CFNumber before it's read.
        unsafe {
            if CFGetTypeID(value) != CFNumberGetTypeID() {
                return None;
            }
            let mut out = 0i64;
            (CFNumberGetValue(value, SINT64, (&mut out as *mut i64).cast()) != 0).then_some(out)
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use super::Gpu;

    pub struct Reader;

    impl Reader {
        pub fn new() -> Option<Self> {
            None
        }

        /// No per-process GPU load here.
        pub fn take_by_process(&mut self) -> std::collections::HashMap<u32, f32> {
            std::collections::HashMap::new()
        }

        pub fn read(&mut self, _per_process: bool) -> Option<Gpu> {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_instances_name_their_process_and_engine() {
        assert_eq!(
            engine_instance("pid_1234_luid_0x00000000_0x0000CD88_phys_0_eng_3_engtype_3D"),
            (Some(1234), "0x00000000_0x0000CD88_phys_0_eng_3_engtype_3D")
        );
        // Two processes' shares of one engine name the same engine.
        assert_eq!(
            engine_instance("pid_8_luid_0x0_0x1_phys_0_eng_0_engtype_Copy").1,
            engine_instance("pid_99_luid_0x0_0x1_phys_0_eng_0_engtype_Copy").1
        );
        assert_eq!(
            engine_instance("luid_0x0_0x1_phys_0_eng_0_engtype_3D"),
            (None, "luid_0x0_0x1_phys_0_eng_0_engtype_3D")
        );
        assert_eq!(
            engine_instance("pid_x_luid_0x0_0x1_phys_0_eng_0"),
            (None, "0x0_0x1_phys_0_eng_0")
        );
    }

    #[test]
    #[ignore = "reads this computer's GPU; run by hand"]
    fn reads_this_computer() {
        let mut reader = Reader::new().expect("GPU counters");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let started = std::time::Instant::now();
        let gpu = reader.read(true).expect("a reading");
        println!("{gpu:?} in {:?}", started.elapsed());
        assert!((0.0..=100.0).contains(&gpu.usage));
    }
}
