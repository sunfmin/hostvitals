//! IOReport subscriptions live for one Window; private symbols are resolved at runtime.
use super::{Reading, dylib::Library};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{CStr, c_char, c_void},
    ptr,
    time::Instant,
};

type Object = *const c_void;
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(allocator: Object, text: *const c_char, encoding: u32) -> Object;
    fn CFStringGetCString(string: Object, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
    fn CFRelease(object: Object);
    fn CFDictionaryCreateMutableCopy(
        allocator: Object,
        capacity: isize,
        dictionary: Object,
    ) -> Object;
    fn CFDictionaryGetValue(dictionary: Object, key: Object) -> Object;
    fn CFArrayGetCount(array: Object) -> isize;
    fn CFArrayGetValueAtIndex(array: Object, index: isize) -> Object;
    fn CFGetTypeID(object: Object) -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFStringGetTypeID() -> usize;
}
struct Owned(Object);
impl Owned {
    fn new(ptr: Object, label: &str) -> Result<Self, String> {
        if ptr.is_null() {
            Err(format!("IOReport {label} returned no readable channels"))
        } else {
            Ok(Self(ptr))
        }
    }
    fn string(value: &CStr) -> Result<Self, String> {
        // SAFETY: value is NUL-terminated; the new string is owned here.
        Self::new(
            unsafe { CFStringCreateWithCString(ptr::null(), value.as_ptr(), 0x08000100) },
            "string",
        )
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.0);
        }
    }
}
// SAFETY at each invocation: the specified signatures are the IOReport C interfaces;
// the library outlives its function pointers and every CF argument remains live.
macro_rules! symbol {
    ($library:expr, $name:literal, $kind:ty) => {
        unsafe { std::mem::transmute::<*mut c_void, $kind>($library.symbol($name)?) }
    };
}
#[derive(Deserialize, Serialize)]
pub struct Channels {
    pub energy_channels: u64,
    pub sampled_channels: u64,
}
#[derive(Deserialize)]
pub struct Energy {
    pub name: String,
    pub unit: String,
    pub value: i64,
    pub counter_before: Option<i64>,
    pub counter_after: Option<i64>,
}
#[derive(Deserialize)]
pub struct Power {
    pub window_seconds: f64,
    pub energy: Vec<Energy>,
    pub gpu_states: Vec<(String, i64)>,
}

/// The retained initial sample and subscription are dropped before unloading their library.
pub struct Window {
    before: Owned,
    subscription: Owned,
    channels: Owned,
    library: Library,
    start: Instant,
    energy_channels: u64,
}
impl Window {
    pub fn start(with_gpu: bool) -> Result<Self, String> {
        let library = Library::open(c"/usr/lib/libIOReport.dylib")?;
        let copy = symbol!(
            library,
            c"IOReportCopyChannelsInGroup",
            unsafe extern "C" fn(Object, Object, u64, u64, u64) -> Object
        );
        let count = symbol!(
            library,
            c"IOReportGetChannelCount",
            unsafe extern "C" fn(Object) -> i32
        );
        let group = Owned::string(c"Energy Model")?;
        let energy = Owned::new(
            unsafe { copy(group.0, ptr::null(), 0, 0, 0) },
            "Energy Model discovery",
        )?;
        let energy_count = unsafe { count(energy.0) };
        if energy_count <= 0 {
            return Err("Energy Model has no channels".into());
        }
        let channels = Owned::new(
            unsafe { CFDictionaryCreateMutableCopy(ptr::null(), 0, energy.0) },
            "mutable channels",
        )?;
        if with_gpu {
            let group = Owned::string(c"GPU Stats")?;
            let subgroup = Owned::string(c"GPU Performance States")?;
            let gpu = unsafe { copy(group.0, subgroup.0, 0, 0, 0) };
            if !gpu.is_null() {
                let gpu = Owned(gpu);
                let merge = symbol!(
                    library,
                    c"IOReportMergeChannels",
                    unsafe extern "C" fn(Object, Object, Object)
                );
                unsafe {
                    merge(channels.0, gpu.0, ptr::null());
                }
            }
        }
        let subscribe = symbol!(
            library,
            c"IOReportCreateSubscription",
            unsafe extern "C" fn(Object, Object, *mut Object, u64, Object) -> Object
        );
        let mut subscribed = ptr::null();
        let subscription = Owned::new(
            unsafe { subscribe(ptr::null(), channels.0, &mut subscribed, 0, ptr::null()) },
            "subscription",
        )?;
        let channels = Owned::new(subscribed, "subscribed channels")?;
        let sample = symbol!(
            library,
            c"IOReportCreateSamples",
            unsafe extern "C" fn(Object, Object, Object) -> Object
        );
        let before = Owned::new(
            unsafe { sample(subscription.0, channels.0, ptr::null()) },
            "initial sample",
        )?;
        let start = Instant::now();
        Ok(Self {
            before,
            subscription,
            channels,
            library,
            start,
            energy_channels: energy_count as u64,
        })
    }
    pub fn finish(self) -> Reading<Power> {
        Reading::from_result(self.delta())
    }
    fn delta(&self) -> Result<Power, String> {
        let sample = symbol!(
            self.library,
            c"IOReportCreateSamples",
            unsafe extern "C" fn(Object, Object, Object) -> Object
        );
        let after = Owned::new(
            unsafe { sample(self.subscription.0, self.channels.0, ptr::null()) },
            "final sample",
        )?;
        let window_seconds = self.start.elapsed().as_secs_f64();
        let before_counters = energy_counters(&self.library, self.before.0)?;
        let after_counters = energy_counters(&self.library, after.0)?;
        let delta = symbol!(
            self.library,
            c"IOReportCreateSamplesDelta",
            unsafe extern "C" fn(Object, Object, Object) -> Object
        );
        let delta = Owned::new(
            unsafe { delta(self.before.0, after.0, ptr::null()) },
            "sample delta",
        )?;
        let channels = channel_array(delta.0)?;
        let name = symbol!(
            self.library,
            c"IOReportChannelGetChannelName",
            unsafe extern "C" fn(Object) -> Object
        );
        let group = symbol!(
            self.library,
            c"IOReportChannelGetGroup",
            unsafe extern "C" fn(Object) -> Object
        );
        let unit = symbol!(
            self.library,
            c"IOReportChannelGetUnitLabel",
            unsafe extern "C" fn(Object) -> Object
        );
        let value = symbol!(
            self.library,
            c"IOReportSimpleGetIntegerValue",
            unsafe extern "C" fn(Object, i32) -> i64
        );
        let state_count = symbol!(
            self.library,
            c"IOReportStateGetCount",
            unsafe extern "C" fn(Object) -> i32
        );
        let state_name = symbol!(
            self.library,
            c"IOReportStateGetNameForIndex",
            unsafe extern "C" fn(Object, i32) -> Object
        );
        let residency = symbol!(
            self.library,
            c"IOReportStateGetResidency",
            unsafe extern "C" fn(Object, i32) -> i64
        );
        let mut power = Power {
            window_seconds,
            energy: Vec::new(),
            gpu_states: Vec::new(),
        };
        for i in 0..unsafe { CFArrayGetCount(channels) } {
            let channel = unsafe { CFArrayGetValueAtIndex(channels, i) };
            if channel.is_null() || unsafe { CFGetTypeID(channel) != CFDictionaryGetTypeID() } {
                return Err("IOReport delta contains a non-channel".into());
            }
            let channel_name = string(unsafe { name(channel) })?;
            match string(unsafe { group(channel) })?.as_str() {
                "Energy Model" => power.energy.push(Energy {
                    counter_before: before_counters.get(&channel_name).copied(),
                    counter_after: after_counters.get(&channel_name).copied(),
                    name: channel_name,
                    unit: string(unsafe { unit(channel) })?,
                    value: unsafe { value(channel, 0) },
                }),
                "GPU Stats" if channel_name == "GPUPH" => {
                    let count = unsafe { state_count(channel) };
                    if !(1..=4096).contains(&count) {
                        return Err("invalid GPU state count".into());
                    }
                    for j in 0..count {
                        power
                            .gpu_states
                            .push((string(unsafe { state_name(channel, j) })?, unsafe {
                                residency(channel, j)
                            }));
                    }
                }
                _ => {}
            }
        }
        Ok(power)
    }
}
fn string(value: Object) -> Result<String, String> {
    if value.is_null() || unsafe { CFGetTypeID(value) != CFStringGetTypeID() } {
        return Err("IOReport string is absent".into());
    }
    let mut buffer = [0i8; 1024];
    if unsafe {
        CFStringGetCString(
            value,
            buffer.as_mut_ptr(),
            buffer.len() as isize,
            0x08000100,
        )
    } == 0
    {
        return Err("IOReport string is too long".into());
    }
    Ok(unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned())
}

fn channel_array(sample: Object) -> Result<Object, String> {
    let key = Owned::string(c"IOReportChannels")?;
    let array = unsafe { CFDictionaryGetValue(sample, key.0) };
    if array.is_null() || unsafe { CFGetTypeID(array) != CFArrayGetTypeID() } {
        Err("IOReport sample has no channel array".into())
    } else {
        Ok(array)
    }
}
fn energy_counters(
    library: &Library,
    sample: Object,
) -> Result<std::collections::BTreeMap<String, i64>, String> {
    let channels = channel_array(sample)?;
    let group = symbol!(
        library,
        c"IOReportChannelGetGroup",
        unsafe extern "C" fn(Object) -> Object
    );
    let name = symbol!(
        library,
        c"IOReportChannelGetChannelName",
        unsafe extern "C" fn(Object) -> Object
    );
    let value = symbol!(
        library,
        c"IOReportSimpleGetIntegerValue",
        unsafe extern "C" fn(Object, i32) -> i64
    );
    let mut counters = std::collections::BTreeMap::new();
    for i in 0..unsafe { CFArrayGetCount(channels) } {
        let channel = unsafe { CFArrayGetValueAtIndex(channels, i) };
        if channel.is_null() || unsafe { CFGetTypeID(channel) != CFDictionaryGetTypeID() } {
            return Err("IOReport sample contains a non-channel".into());
        }
        if string(unsafe { group(channel) })? == "Energy Model" {
            counters.insert(string(unsafe { name(channel) })?, unsafe {
                value(channel, 0)
            });
        }
    }
    Ok(counters)
}
/// Discovery alone does not establish readability: doctor takes a real initial sample.
pub fn read() -> Reading<Channels> {
    Reading::from_result((|| {
        let window = Window::start(false)?;
        let count = symbol!(
            window.library,
            c"IOReportGetChannelCount",
            unsafe extern "C" fn(Object) -> i32
        );
        let sampled = unsafe { count(window.before.0) };
        if sampled <= 0 {
            return Err("Energy Model subscription produced an empty sample".into());
        }
        let counters = energy_counters(&window.library, window.before.0)?;
        if !counters
            .iter()
            .any(|(name, value)| name.ends_with("CPU Energy") && *value > 0)
        {
            return Err("Energy Model CPU counters have no accumulated energy; channel enumeration and sampling do not establish readable power on this macOS".into());
        }
        Ok(Channels {
            energy_channels: window.energy_channels,
            sampled_channels: sampled as u64,
        })
    })())
}
