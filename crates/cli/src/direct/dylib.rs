//! Runtime lookup keeps changed private interfaces reportable instead of preventing startup.
use std::ffi::{CStr, c_char, c_int, c_void};
unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
}
pub struct Library(*mut c_void);
impl Library {
    /// Opens a system library locally and immediately; callers retain it while using its symbols.
    pub fn open(path: &CStr) -> Result<Self, String> {
        // SAFETY: path is terminated; RTLD_NOW | RTLD_LOCAL are Darwin's 2 | 4.
        let handle = unsafe { dlopen(path.as_ptr(), 6) };
        if handle.is_null() {
            Err(format!("cannot load {}", path.to_string_lossy()))
        } else {
            Ok(Self(handle))
        }
    }
    /// A raw function address; the caller must use the system interface's exact C signature.
    pub fn symbol(&self, name: &CStr) -> Result<*mut c_void, String> {
        // SAFETY: the handle is live and the name is terminated.
        let ptr = unsafe { dlsym(self.0, name.as_ptr()) };
        if ptr.is_null() {
            Err(format!(
                "interface {} no longer answers",
                name.to_string_lossy()
            ))
        } else {
            Ok(ptr)
        }
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            dlclose(self.0);
        }
    }
}
