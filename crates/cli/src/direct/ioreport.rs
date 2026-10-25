//! Availability of Energy Model channels and a real IOReport sample; all objects live for this read only.
use super::{Reading, dylib::Library};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{c_char, c_void},
    ptr,
};
#[derive(Deserialize, Serialize)]
pub struct Channels {
    pub energy_channels: u64,
    pub sampled_channels: u64,
}
type Object = *const c_void;
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(allocator: Object, text: *const c_char, encoding: u32) -> Object;
    fn CFRelease(object: Object);
}
struct Owned(Object);
impl Owned {
    fn new(ptr: Object, label: &str) -> Result<Self, String> {
        if ptr.is_null() {
            Err(format!(
                "{label} returned no readable Energy Model channels"
            ))
        } else {
            Ok(Self(ptr))
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.0);
        }
    }
}
/// Enumerates, subscribes and samples without root; discovery alone does not establish readability.
pub fn read() -> Reading<Channels> {
    Reading::from_result((|| {
        let library = Library::open(c"/usr/lib/libIOReport.dylib")?;
        // SAFETY: signatures follow IOReport's C interfaces; all arguments are live CF objects or null optional arguments.
        let copy: unsafe extern "C" fn(Object, Object, u64, u64, u64) -> Object =
            unsafe { std::mem::transmute(library.symbol(c"IOReportCopyChannelsInGroup")?) };
        let count: unsafe extern "C" fn(Object) -> i32 =
            unsafe { std::mem::transmute(library.symbol(c"IOReportGetChannelCount")?) };
        let subscribe: unsafe extern "C" fn(Object, Object, *mut Object, u64, Object) -> Object =
            unsafe { std::mem::transmute(library.symbol(c"IOReportCreateSubscription")?) };
        let sample: unsafe extern "C" fn(Object, Object, Object) -> Object =
            unsafe { std::mem::transmute(library.symbol(c"IOReportCreateSamples")?) };
        let group = Owned::new(
            unsafe { CFStringCreateWithCString(ptr::null(), c"Energy Model".as_ptr(), 0x08000100) },
            "group name",
        )?;
        let channels = Owned::new(
            unsafe { copy(group.0, ptr::null(), 0, 0, 0) },
            "channel discovery",
        )?;
        let energy_channels = unsafe { count(channels.0) };
        if energy_channels <= 0 {
            return Err("Energy Model has no channels".into());
        }
        let mut subscribed = ptr::null();
        let subscription = Owned::new(
            unsafe { subscribe(ptr::null(), channels.0, &mut subscribed, 0, ptr::null()) },
            "subscription",
        )?;
        let subscribed = Owned::new(subscribed, "subscribed channels")?;
        let samples = Owned::new(
            unsafe { sample(subscription.0, subscribed.0, ptr::null()) },
            "sampling",
        )?;
        let sampled_channels = unsafe { count(samples.0) };
        if sampled_channels <= 0 {
            return Err("Energy Model subscription produced an empty sample".into());
        }
        Ok(Channels {
            energy_channels: energy_channels as u64,
            sampled_channels: sampled_channels as u64,
        })
    })())
}
