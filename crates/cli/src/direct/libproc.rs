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
