//! Task information readable for this process via libproc; this is not the source for other users' processes.
//! It also counts every Process on the host, which takes milliseconds where ps can take seconds.
use super::Reading;
use serde::{Deserialize, Serialize};
#[derive(Deserialize, Serialize)]
pub struct Task {
    pub pid: u32,
    pub resident_bytes: u64,
    pub threads: u32,
}
// Darwin sys/proc_info.h, proc_taskinfo: six 64-bit counters and twelve 32-bit fields.
#[repr(C)]
#[derive(Default)]
struct TaskInfo {
    counters: [u64; 6],
    fields: [i32; 12],
}
#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        arg: u64,
        buffer: *mut std::ffi::c_void,
        size: i32,
    ) -> i32;
    fn proc_listallpids(buffer: *mut std::ffi::c_void, size: i32) -> i32;
}
/// Counts every Process on the host, every user's, without spawning anything.
pub fn process_count() -> Reading<u64> {
    // SAFETY: a null buffer asks only for the current number of pids.
    let estimate = unsafe { proc_listallpids(std::ptr::null_mut(), 0) };
    if estimate <= 0 {
        return Reading::Unavailable {
            reason: format!("proc_listallpids: {}", std::io::Error::last_os_error()),
        };
    }
    // Room for Processes started between the two calls.
    let mut pids = vec![0i32; estimate as usize + 256];
    let size = (pids.len() * std::mem::size_of::<i32>()) as i32;
    // SAFETY: the buffer holds `size` writable bytes of pid_t.
    let count = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), size) };
    if count <= 0 {
        return Reading::Unavailable {
            reason: format!("proc_listallpids: {}", std::io::Error::last_os_error()),
        };
    }
    Reading::Available { data: count as u64 }
}
/// Checks task information for the observer without seeking a task port.
pub fn read() -> Reading<Task> {
    read_pid(std::process::id())
}
/// Reads the target's thread count without seeking a task port.
pub fn read_pid(pid: u32) -> Reading<Task> {
    let mut task = TaskInfo::default();
    let size = std::mem::size_of::<TaskInfo>() as i32;
    // SAFETY: PROC_PIDTASKINFO (4) takes the proc_taskinfo buffer and its exact byte size.
    let got = unsafe { proc_pidinfo(pid as i32, 4, 0, (&mut task as *mut TaskInfo).cast(), size) };
    if got != size || task.fields[9] <= 0 {
        return Reading::Unavailable {
            reason: format!(
                "PROC_PIDTASKINFO returned {got} bytes: {}",
                std::io::Error::last_os_error()
            ),
        };
    }
    Reading::Available {
        data: Task {
            pid,
            resident_bytes: task.counters[1],
            threads: task.fields[9] as u32,
        },
    }
}
// Darwin sys/proc_info.h, proc_bsdinfo: twelve 32-bit ids, the 16- and 32-byte names, six
// 32-bit fields, then the start time's seconds and microseconds.
#[repr(C)]
struct BsdInfo {
    ids: [u32; 12],
    names: [u8; 48],
    fields: [u32; 6],
    start_seconds: u64,
    start_microseconds: u64,
}
/// How long ago this process was started, from the kernel's own record of its exec, so a
/// bound counted from it includes the time a crowded host took to start the process at all.
pub fn own_age() -> Result<std::time::Duration, String> {
    // SAFETY: an all-zero proc_bsdinfo is a valid value of its plain integer fields.
    let mut info: BsdInfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<BsdInfo>() as i32;
    // SAFETY: PROC_PIDTBSDINFO (3) takes the proc_bsdinfo buffer and its exact byte size.
    let got = unsafe {
        proc_pidinfo(
            std::process::id() as i32,
            3,
            0,
            (&mut info as *mut BsdInfo).cast(),
            size,
        )
    };
    if got != size || info.ids[3] != std::process::id() {
        return Err(format!(
            "PROC_PIDTBSDINFO returned {got} bytes: {}",
            std::io::Error::last_os_error()
        ));
    }
    let started = std::time::UNIX_EPOCH
        + std::time::Duration::new(info.start_seconds, 0)
        + std::time::Duration::from_micros(info.start_microseconds);
    std::time::SystemTime::now()
        .duration_since(started)
        .map_err(|e| format!("process start is in the future: {e}"))
}
#[cfg(test)]
mod tests {
    #[test]
    fn own_age_is_the_time_since_this_test_process_started() {
        let first = super::own_age().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let second = super::own_age().unwrap();
        assert!(
            second >= first + std::time::Duration::from_millis(50),
            "{first:?} {second:?}"
        );
        // A test binary is seconds old at most, never years: the layout's offsets are right.
        assert!(second < std::time::Duration::from_secs(600), "{second:?}");
    }
}
