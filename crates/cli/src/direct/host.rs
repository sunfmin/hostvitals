//! Host CPU ticks and memory accounting from Darwin's read-only interfaces.
use super::Reading;
use serde::{Deserialize, Serialize};
use std::{ffi::CStr, mem::size_of};

#[derive(Deserialize, Serialize)]
pub struct Snapshot {
    pub euid: u32,
    pub cpu_ticks: [u32; 4],
    pub logical_cores: u32,
    pub physical_cores: u32,
    pub load_average: [f64; 3],
    pub memory: Memory,
}
#[derive(Deserialize, Serialize)]
pub struct Memory {
    pub total_bytes: u64,
    pub free_percent: u32,
    pub pressure: String,
    pub swap_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub compressed_bytes: u64,
}
// Darwin sys/sysctl.h xsw_usage, including the two 32-bit trailing members.
#[repr(C)]
#[derive(Default)]
struct Swap {
    total: u64,
    available: u64,
    used: u64,
    page_size: u32,
    encrypted: i32,
}
/// Values that can receive a Darwin sysctl's bytes.
///
/// # Safety
/// Implementations must be plain Darwin ABI values valid for every bit pattern.
unsafe trait SysctlValue: Default {}
unsafe impl SysctlValue for u32 {}
unsafe impl SysctlValue for i32 {}
unsafe impl SysctlValue for u64 {}
unsafe impl SysctlValue for Swap {}
unsafe extern "C" {
    fn sysctlbyname(
        name: *const std::ffi::c_char,
        old: *mut std::ffi::c_void,
        len: *mut usize,
        new: *mut std::ffi::c_void,
        newlen: usize,
    ) -> i32;
    fn geteuid() -> u32;
    fn getloadavg(load: *mut f64, count: i32) -> i32;
    fn mach_host_self() -> u32;
    static mach_task_self_: u32;
    fn mach_port_deallocate(task: u32, name: u32) -> i32;
    fn host_statistics(host: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
}
// Only integer sysctls and the POD Swap layout are read through this private helper.
fn sysctl<T: SysctlValue>(name: &CStr) -> Result<T, String> {
    let mut value = T::default();
    let mut len = size_of::<T>();
    // SAFETY: each caller selects the Darwin ABI type for the named sysctl; no writes to the host.
    if unsafe {
        sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut T).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || len != size_of::<T>()
    {
        return Err(format!(
            "{}: {}",
            name.to_string_lossy(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(value)
}
pub fn read() -> Reading<Snapshot> {
    Reading::from_result(snapshot())
}
/// The actual effective user, including when native measurement fixtures are selected.
pub(crate) fn effective_uid() -> u32 {
    unsafe { geteuid() }
}

fn snapshot() -> Result<Snapshot, String> {
    let mut ticks = [0u32; 4];
    let mut count = 4;
    // SAFETY: HOST_CPU_LOAD_INFO (3) writes four natural_t CPU ticks, per mach/host_info.h.
    let status = unsafe {
        let host = mach_host_self();
        let status = host_statistics(host, 3, ticks.as_mut_ptr().cast(), &mut count);
        mach_port_deallocate(mach_task_self_, host);
        status
    };
    if status != 0 || count != 4 {
        return Err(format!("host CPU ticks: Mach status {status}"));
    }
    let mut load = [0.0; 3];
    // SAFETY: getloadavg writes at most the three allocated doubles.
    if unsafe { getloadavg(load.as_mut_ptr(), 3) } != 3 {
        return Err("cannot read load averages".into());
    }
    let pressure = match sysctl::<i32>(c"kern.memorystatus_vm_pressure_level")? {
        1 => "normal",
        2 => "warn",
        4 => "critical",
        level => return Err(format!("unknown memory pressure level {level}")),
    };
    let swap: Swap = sysctl(c"vm.swapusage")?;
    Ok(Snapshot {
        euid: effective_uid(),
        cpu_ticks: ticks,
        logical_cores: sysctl(c"hw.logicalcpu")?,
        physical_cores: sysctl(c"hw.physicalcpu")?,
        load_average: load,
        memory: Memory {
            total_bytes: sysctl(c"hw.memsize")?,
            free_percent: sysctl(c"kern.memorystatus_level")?,
            pressure: pressure.into(),
            swap_used_bytes: swap.used,
            swap_total_bytes: swap.total,
            compressed_bytes: sysctl(c"vm.compressor_bytes_used")?,
        },
    })
}
