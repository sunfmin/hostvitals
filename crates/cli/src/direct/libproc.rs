//! Task information readable for this process via libproc; this is not the source for other users' processes.
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
