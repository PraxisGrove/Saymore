#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalInferenceDevice {
    pub architecture: &'static str,
    pub total_physical_memory_bytes: Option<u64>,
    pub available_physical_memory_bytes: Option<u64>,
    pub avx: bool,
    pub avx2: bool,
    pub cpu_backend_supported: bool,
}

pub fn local_inference_device() -> LocalInferenceDevice {
    let (total_physical_memory_bytes, available_physical_memory_bytes) = physical_memory();
    LocalInferenceDevice {
        architecture: std::env::consts::ARCH,
        total_physical_memory_bytes,
        available_physical_memory_bytes,
        avx: avx_available(),
        avx2: avx2_available(),
        cpu_backend_supported: cfg!(target_arch = "x86_64"),
    }
}

#[cfg(target_os = "windows")]
fn physical_memory() -> (Option<u64>, Option<u64>) {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: Windows writes to a correctly sized, initialized MEMORYSTATUSEX value.
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_err() {
        return (None, None);
    }
    (Some(status.ullTotalPhys), Some(status.ullAvailPhys))
}

#[cfg(not(target_os = "windows"))]
fn physical_memory() -> (Option<u64>, Option<u64>) {
    (None, None)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn avx_available() -> bool {
    std::arch::is_x86_feature_detected!("avx")
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn avx_available() -> bool {
    false
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn avx2_available() -> bool {
    std::arch::is_x86_feature_detected!("avx2")
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
fn avx2_available() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_device_reports_the_compiled_architecture() {
        let device = local_inference_device();
        assert_eq!(std::env::consts::ARCH, device.architecture);
        #[cfg(target_os = "windows")]
        assert!(
            device
                .total_physical_memory_bytes
                .is_some_and(|bytes| bytes > 0)
        );
    }
}
