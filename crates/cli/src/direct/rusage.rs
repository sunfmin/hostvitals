//! Cumulative counters readable for this process via proc_pid_rusage v6.
use super::Reading;
use serde::{Deserialize, Serialize};
#[derive(Deserialize, Serialize)]
pub struct Counters {
    pub resident_bytes: u64,
    pub footprint_bytes: u64,
    pub peak_footprint_bytes: u64,
    pub read_bytes: u64,
    pub written_bytes: u64,
    pub energy_nj: u64,
}
// Darwin sys/resource.h, rusage_info_v6. Keep the whole ABI, including reserved space.
#[repr(C)]
#[derive(Default)]
struct RusageV6 {
    uuid: [u8; 16],
    user_time: u64,
    system_time: u64,
    pkg_idle_wkups: u64,
    interrupt_wkups: u64,
    pageins: u64,
    wired_size: u64,
    resident_size: u64,
    phys_footprint: u64,
    proc_start_abstime: u64,
    proc_exit_abstime: u64,
    child_user_time: u64,
    child_system_time: u64,
    child_pkg_idle_wkups: u64,
    child_interrupt_wkups: u64,
    child_pageins: u64,
    child_elapsed_abstime: u64,
    diskio_bytesread: u64,
    diskio_byteswritten: u64,
    cpu_time_qos_default: u64,
    cpu_time_qos_maintenance: u64,
    cpu_time_qos_background: u64,
    cpu_time_qos_utility: u64,
    cpu_time_qos_legacy: u64,
    cpu_time_qos_user_initiated: u64,
    cpu_time_qos_user_interactive: u64,
    billed_system_time: u64,
    serviced_system_time: u64,
    logical_writes: u64,
    lifetime_max_phys_footprint: u64,
    instructions: u64,
    cycles: u64,
    billed_energy: u64,
    serviced_energy: u64,
    interval_max_phys_footprint: u64,
    runnable_time: u64,
    flags: u64,
    user_ptime: u64,
    system_ptime: u64,
    pinstructions: u64,
    pcycles: u64,
    energy_nj: u64,
    penergy_nj: u64,
    secure_time_in_system: u64,
    secure_ptime_in_system: u64,
    neural_footprint: u64,
    lifetime_max_neural_footprint: u64,
    interval_max_neural_footprint: u64,
    conclave_footprint: u64,
    page_wait_time_mach: u64,
    page_cache_hits: u64,
    reserved: [u64; 6],
}
#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut std::ffi::c_void) -> i32;
}
/// Reads the observer, whose pid and ownership cannot change during the call.
pub fn read() -> Reading<Counters> {
    read_pid(std::process::id())
}
/// Reads one process; a denial or exit remains an unavailable reading.
pub fn read_pid(pid: u32) -> Reading<Counters> {
    let mut usage = RusageV6::default();
    // SAFETY: flavor 6 writes exactly rusage_info_v6 into this aligned, writable buffer.
    let status = unsafe { proc_pid_rusage(pid as i32, 6, (&mut usage as *mut RusageV6).cast()) };
    if status != 0 {
        return Reading::Unavailable {
            reason: format!("proc_pid_rusage v6: {}", std::io::Error::last_os_error()),
        };
    }
    Reading::Available {
        data: Counters {
            resident_bytes: usage.resident_size,
            footprint_bytes: usage.phys_footprint,
            peak_footprint_bytes: usage.lifetime_max_phys_footprint,
            read_bytes: usage.diskio_bytesread,
            written_bytes: usage.diskio_byteswritten,
            energy_nj: usage.energy_nj,
        },
    }
}
