//! Peak RSS probe.
//!
//! On macOS we read `resident_size_max` from `mach_task_basic_info` via the
//! `task_info()` Mach trap. On other platforms this returns `None` — the eval
//! is macOS-first per the project's roadmap. Linux/Windows ports can implement
//! a sibling `cfg` branch reading from `/proc/self/status` or
//! `GetProcessMemoryInfo` respectively.

#[cfg(target_os = "macos")]
mod imp {
    use mach2::kern_return::KERN_SUCCESS;
    use mach2::message::mach_msg_type_number_t;
    use mach2::task::task_info;
    use mach2::task_info::MACH_TASK_BASIC_INFO;
    use mach2::traps::mach_task_self;
    use mach2::vm_types::{integer_t, mach_vm_size_t, natural_t};

    // `policy_t` is not re-exported by mach2 0.4. From `<mach/policy.h>` it is
    // an alias for `int`, which matches `integer_t`.
    type PolicyT = integer_t;

    // `mach2 0.4` does not expose `mach_task_basic_info_data_t` directly, so we
    // mirror the C struct from `<mach/task_info.h>`. The layout is stable Mach
    // kernel ABI and matches what the kernel writes back for the
    // `MACH_TASK_BASIC_INFO` flavor.
    #[repr(C)]
    #[derive(Default)]
    struct MachTaskBasicInfo {
        virtual_size: mach_vm_size_t,
        resident_size: mach_vm_size_t,
        resident_size_max: mach_vm_size_t,
        user_time: TimeValue,
        system_time: TimeValue,
        policy: PolicyT,
        suspend_count: integer_t,
    }

    #[repr(C)]
    #[derive(Default)]
    struct TimeValue {
        seconds: integer_t,
        microseconds: integer_t,
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn peak_rss_kb() -> Option<u64> {
        let mut info = MachTaskBasicInfo::default();
        let info_size = std::mem::size_of::<MachTaskBasicInfo>();
        let count_units = info_size / std::mem::size_of::<natural_t>();
        let mut count = count_units as mach_msg_type_number_t;
        // SAFETY: `mach_task_self()` returns a valid task port for the current
        // process. We pass a mutable pointer to a stack-allocated
        // `MachTaskBasicInfo` whose layout matches the kernel's
        // `mach_task_basic_info_data_t`, together with a `count` set to the
        // struct size in `natural_t` units. The kernel will not write past
        // `count * sizeof(integer_t)` bytes.
        let kr = unsafe {
            task_info(
                mach_task_self(),
                MACH_TASK_BASIC_INFO,
                std::ptr::addr_of_mut!(info).cast(),
                std::ptr::addr_of_mut!(count),
            )
        };
        if kr != KERN_SUCCESS {
            return None;
        }
        Some(info.resident_size_max / 1024)
    }
}

#[cfg(target_os = "macos")]
#[must_use]
pub fn peak_rss_kb() -> Option<u64> {
    imp::peak_rss_kb()
}

#[cfg(not(target_os = "macos"))]
#[must_use]
#[allow(clippy::missing_const_for_fn)]
pub fn peak_rss_kb() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::peak_rss_kb;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_peak_rss_returns_some_positive_value() {
        // Allocate a heavy buffer to make sure RSS is non-trivial.
        let _heavy = vec![0u8; 16 * 1024 * 1024];
        let r = peak_rss_kb();
        assert!(r.is_some());
        assert!(r.unwrap() > 1024, "expected RSS > 1 MiB, got {r:?}");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_returns_none() {
        assert!(peak_rss_kb().is_none());
    }
}
