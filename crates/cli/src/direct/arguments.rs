//! Exact argv acquisition. The environment-capable buffer never crosses this module.
use super::{Reading, Source};
use std::{fs::File, io::Read};
pub const LIMIT_MIB: usize = 1;
const LIMIT: usize = LIMIT_MIB * 1024 * 1024;

unsafe extern "C" {
    fn sysctl(
        name: *mut i32,
        namelen: u32,
        old: *mut std::ffi::c_void,
        oldlen: *mut usize,
        new: *mut std::ffi::c_void,
        newlen: usize,
    ) -> i32;
}

/// Returns sanitized arguments only; errors contain no input bytes, including fixtures.
pub fn read(source: &Source, pid: u32) -> Reading<Vec<String>> {
    Reading::from_result((|| {
        let bytes = match &source.0 {
            Some(dir) => {
                let mut bytes = Vec::new();
                File::open(dir.join("process-arguments.bin"))
                    .map_err(|_| "argument fixture unavailable")?
                    .take((LIMIT + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "argument fixture unreadable")?;
                if bytes.len() > LIMIT {
                    return Err("argument buffer exceeds limit".into());
                }
                bytes
            }
            None => native(pid)?,
        };
        // Drop the entire native buffer and temporary raw argv before returning any result.
        parse(&bytes).map_err(str::to_owned)
    })())
}

fn native(pid: u32) -> Result<Vec<u8>, String> {
    let mut max = 0i32;
    let mut len = std::mem::size_of::<i32>();
    let mut mib = [1, 8]; // CTL_KERN, KERN_ARGMAX
    // SAFETY: KERN_ARGMAX writes one int; all pointers cover their specified sizes.
    if unsafe {
        sysctl(
            mib.as_mut_ptr(),
            2,
            (&mut max as *mut i32).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || len != 4
        || max <= 0
        || max as usize > LIMIT
    {
        return Err("KERN_ARGMAX unavailable or exceeds bounded argument allocation".into());
    }
    let mut bytes = vec![0; max as usize];
    len = bytes.len();
    let mut mib = [1, 49, pid as i32]; // CTL_KERN, KERN_PROCARGS2, pid
    // SAFETY: the kernel receives an initialized writable allocation and its capacity.
    if unsafe {
        sysctl(
            mib.as_mut_ptr(),
            3,
            bytes.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return Err(format!(
            "KERN_PROCARGS2: {}",
            std::io::Error::last_os_error()
        ));
    }
    if len > bytes.len() {
        return Err("invalid argument buffer length".into());
    }
    bytes.truncate(len);
    Ok(bytes)
}

fn parse(bytes: &[u8]) -> Result<Vec<String>, &'static str> {
    let header: [u8; 4] = bytes
        .get(..4)
        .ok_or("missing argument count")?
        .try_into()
        .map_err(|_| "invalid argument count")?;
    let argc = i32::from_ne_bytes(header);
    if argc <= 0 || argc as usize > bytes.len() {
        return Err("invalid argument count");
    }
    let tail = bytes.get(4..).ok_or("missing executable path")?;
    let end = tail
        .iter()
        .position(|b| *b == 0)
        .ok_or("unterminated executable path")?;
    // XNU exec_extract_strings aligns the path area to the target pointer size.
    // Both supported process architectures use eight-byte pointers. sysctl strips
    // the 16-byte executable_path= prefix, preserving that alignment relative to
    // the path start (not the four-byte argc). Never scan NULs into an empty argv[0].
    // apple-oss-distributions/xnu bsd/kern/kern_exec.c, exec_extract_strings.
    let after_path = end + 1;
    let padding = (8 - after_path % 8) % 8;
    let argv_start = after_path + padding;
    if tail
        .get(after_path..argv_start)
        .ok_or("missing argument padding")?
        .iter()
        .any(|b| *b != 0)
    {
        return Err("invalid argument padding");
    }
    let mut remaining = tail.get(argv_start..).ok_or("missing argv")?;
    let mut arguments = Vec::new();
    for _ in 0..argc {
        let end = remaining
            .iter()
            .position(|b| *b == 0)
            .ok_or("unterminated argument vector")?;
        arguments.push(
            String::from_utf8_lossy(remaining.get(..end).ok_or("invalid argument length")?)
                .into_owned(),
        );
        remaining = remaining.get(end + 1..).ok_or("invalid argument length")?;
    }
    // Never scan, decode or include the environment suffix in a diagnostic.
    Ok(hostvitals_policy::redaction::redact(&arguments))
}
