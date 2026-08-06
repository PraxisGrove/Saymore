#[cfg(target_os = "macos")]
use mach2::{
    kern_return::KERN_SUCCESS,
    task::task_info,
    task_info::{MACH_TASK_BASIC_INFO, MACH_TASK_BASIC_INFO_COUNT, mach_task_basic_info},
    traps::mach_task_self,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::{
    ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
    Threading::GetCurrentProcess,
};

#[cfg(target_os = "macos")]
pub fn current_process_resident_memory_bytes() -> Option<u64> {
    let mut info = mach_task_basic_info::default();
    let mut count = MACH_TASK_BASIC_INFO_COUNT;
    // SAFETY: `info` is a writable Mach task-info buffer of the advertised size.
    let status = unsafe {
        task_info(
            mach_task_self(),
            MACH_TASK_BASIC_INFO,
            std::ptr::addr_of_mut!(info).cast(),
            &mut count,
        )
    };
    if status != KERN_SUCCESS {
        return None;
    }
    // `mach2` models this ABI structure as packed to four-byte alignment.
    Some(unsafe { std::ptr::addr_of!(info.resident_size).read_unaligned() })
}

#[cfg(target_os = "windows")]
pub fn current_process_resident_memory_bytes() -> Option<u64> {
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?,
        ..PROCESS_MEMORY_COUNTERS::default()
    };
    // SAFETY: the pseudo-handle is valid for the current process and `counters`
    // is a writable buffer whose size is passed to the API.
    let sampled = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            std::ptr::addr_of_mut!(counters),
            counters.cb,
        )
    };
    sampled
        .as_bool()
        .then(|| u64::try_from(counters.WorkingSetSize).ok())
        .flatten()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn current_process_resident_memory_bytes() -> Option<u64> {
    None
}

#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn current_process_has_a_nonzero_resident_working_set() {
        assert!(current_process_resident_memory_bytes().is_some_and(|bytes| bytes > 0));
    }
}
