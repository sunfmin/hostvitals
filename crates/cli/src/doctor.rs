//! A complete capability inventory; missing capabilities are answers, not command failures.
use crate::{
    direct::{self, Reading, Source},
    tools::Tool,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize)]
struct HostFixture {
    macos_version: String,
    arch: String,
    euid: u32,
}

/// Reports every capability, including its remediation, without aborting on the first failed probe.
pub fn answer(source: &Source) -> Result<Value, String> {
    let host = if let Some(dir) = &source.0 {
        let bytes = std::fs::read(dir.join("host.json")).map_err(|e| {
            format!(
                "host fixture: {e}; set HOSTVITALS_PROBE_FIXTURES to a complete fixture directory"
            )
        })?;
        serde_json::from_slice::<HostFixture>(&bytes)
            .map_err(|e| format!("host fixture: {e}; correct host.json"))?
    } else {
        host()?
    };
    let is_root = host.euid == 0;
    let mut checks = Vec::new();
    for tool in Tool::all() {
        let path = tool.find();
        let command = "hostvitals doctor".to_string();
        match path {
            None => checks.push(json!({"name":tool.name(),"kind":"cli","status":"unavailable","reason":"executable not found on PATH","fix":format!("PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH {command}")})),
            Some(path) if hostvitals_policy::may_probe(true, tool.needs_root(), is_root) => match tool.probe(path) {
                Ok(()) => checks.push(json!({"name":tool.name(),"kind":"cli","status":"available"})),
                Err(reason) => checks.push(json!({"name":tool.name(),"kind":"cli","status":"unavailable","reason":reason,"fix":if is_root { command } else { "sudo hostvitals doctor".into() },"guidance":"For a permission denial, the caller can retry as root. If the CLI interface changed, update hostvitals before retrying."})),
            },
            Some(_) => checks.push(json!({"name":tool.name(),"kind":"cli","status":"skipped","reason":"measurement requires root","fix":"sudo hostvitals doctor"})),
        }
    }
    checks.push(native(
        "rusage",
        source.read("rusage", direct::rusage::read),
    ));
    checks.push(native(
        "libproc",
        source.read("libproc", direct::libproc::read),
    ));
    checks.push(native(
        "ioreport",
        source.read("ioreport", direct::ioreport::read),
    ));
    checks.push(native(
        "responsibility",
        source.read("responsibility", direct::responsibility::read),
    ));
    Ok(
        json!({"source":if source.0.is_some() { "fixtures" } else { "host" },"host":{"macos_version":host.macos_version,"arch":host.arch,"is_root":is_root},"checks":checks}),
    )
}
fn native<T: Serialize>(name: &str, reading: Reading<T>) -> Value {
    match reading {
        Reading::Available { data } => {
            json!({"name":name,"kind":"direct","status":"available","data":data})
        }
        Reading::Unavailable { reason } => {
            json!({"name":name,"kind":"direct","status":"unavailable","reason":reason,"fix":"brew upgrade sunfmin/tap/hostvitals && hostvitals doctor","guidance":"Private interfaces can change or be absent on this Mac. Retry with the latest hostvitals on a supported macOS host; root does not repair a missing interface."})
        }
    }
}

fn host() -> Result<HostFixture, String> {
    unsafe extern "C" {
        fn sysctlbyname(
            name: *const std::ffi::c_char,
            old: *mut std::ffi::c_void,
            len: *mut usize,
            new: *mut std::ffi::c_void,
            newlen: usize,
        ) -> i32;
        fn geteuid() -> u32;
    }
    let mut bytes = [0u8; 128];
    let mut len = bytes.len();
    // SAFETY: this is a read-only sysctl with a bounded writable buffer.
    let status = unsafe {
        sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 || len > bytes.len() {
        return Err("cannot read macOS version; run hostvitals doctor on macOS".into());
    }
    let version = std::ffi::CStr::from_bytes_until_nul(&bytes)
        .map_err(|e| format!("macOS version: {e}; run hostvitals doctor again"))?;
    Ok(HostFixture {
        macos_version: version.to_string_lossy().into_owned(),
        arch: if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x86_64"
        }
        .into(),
        euid: unsafe { geteuid() },
    })
}
