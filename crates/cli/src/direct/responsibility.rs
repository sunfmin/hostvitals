//! macOS's responsible process, which defines this process's App.
use super::{Reading, dylib::Library};
use serde::{Deserialize, Serialize};
#[derive(Deserialize, Serialize)]
pub struct App {
    pub pid: u32,
    pub app_pid: u32,
}
/// Reports a missing private symbol distinctly from a process having no other responsible App.
pub fn read() -> Reading<App> {
    read_pid(std::process::id())
}
/// Resolves the responsible App without substituting a parent or executable name.
pub fn read_pid(pid: u32) -> Reading<App> {
    Reading::from_result((|| {
        let library = Library::open(c"/usr/lib/libSystem.B.dylib")?;
        let ptr = library.symbol(c"responsibility_get_pid_responsible_for_pid")?;
        // SAFETY: this private libSystem interface takes and returns a scalar pid_t.
        let responsible: unsafe extern "C" fn(i32) -> i32 = unsafe { std::mem::transmute(ptr) };
        let app = unsafe { responsible(pid as i32) };
        // Darwin uses -1 when there is no other responsible process; zero is not a process.
        if app == 0 {
            return Err("responsibility interface returned pid 0".into());
        }
        Ok(App {
            pid,
            app_pid: if app < 0 { pid } else { app as u32 },
        })
    })())
}
