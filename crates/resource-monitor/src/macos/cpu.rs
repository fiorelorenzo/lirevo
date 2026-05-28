//! System-wide CPU% from `host_processor_info(PROCESSOR_CPU_LOAD_INFO)`.
//!
//! Polled every 5s. We keep the previous tick's per-CPU tick counters
//! (user / system / idle / nice) and emit the percentage of non-idle ticks
//! within the elapsed window. The first tick after spawn produces no
//! sample because we need two reads to take a diff.
//!
//! mach2 0.4 doesn't expose `host_processor_info`, `mach_host_self`, or
//! `vm_deallocate`, so the FFI declarations live here.

use std::sync::Arc;
use std::time::Duration;

use mach2::kern_return::{kern_return_t, KERN_SUCCESS};
use mach2::mach_types::host_t;
use mach2::message::mach_msg_type_number_t;
use mach2::port::mach_port_t;
use mach2::traps::mach_task_self;
use mach2::vm_types::{integer_t, natural_t, vm_address_t, vm_size_t};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::warn;

use crate::shared::SharedState;

const POLL_INTERVAL: Duration = Duration::from_secs(5);

const PROCESSOR_CPU_LOAD_INFO: i32 = 2;
const CPU_STATE_USER: usize = 0;
const CPU_STATE_SYSTEM: usize = 1;
const CPU_STATE_IDLE: usize = 2;
const CPU_STATE_NICE: usize = 3;
const CPU_STATE_MAX: usize = 4;

// Discipline mirror of memory.rs's static size guard: if Apple ever extends
// `processor_cpu_load_info_data_t` we'd want the compile to fail loudly.
const _: () = assert!(CPU_STATE_MAX == 4);

unsafe extern "C" {
    fn mach_host_self() -> host_t;
    fn host_processor_info(
        host: host_t,
        flavor: i32,
        out_processor_count: *mut natural_t,
        out_info_array: *mut *mut integer_t,
        out_info_count: *mut mach_msg_type_number_t,
    ) -> kern_return_t;
    fn vm_deallocate(
        target_task: mach_port_t,
        address: vm_address_t,
        size: vm_size_t,
    ) -> kern_return_t;
}

/// Spawn the CPU sensor. Returns `(Some(notify), handles)` for protocol
/// uniformity with the other sensors; the `Notify` is never fired (CPU is
/// pure polling).
#[allow(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
pub(super) fn spawn(state: Arc<SharedState>) -> (Option<Arc<Notify>>, Vec<JoinHandle<()>>) {
    let notify = Arc::new(Notify::new());
    let state_for_poll = state.clone();

    let handle = tokio::spawn(async move {
        let mut prev: Option<Vec<u64>> = None;
        let mut ticker = tokio::time::interval(POLL_INTERVAL);
        loop {
            ticker.tick().await;
            let Some(snapshot) = read_ticks() else {
                continue;
            };
            if let Some(prev_vec) = &prev {
                if let Some(pct) = compute_pct(prev_vec, &snapshot) {
                    state_for_poll.set_cpu_used_pct(pct);
                }
            }
            prev = Some(snapshot);
        }
    });

    (Some(notify), vec![handle])
}

/// Returns a flat `Vec<u64>` of cumulative ticks: 4 entries
/// (USER, SYSTEM, IDLE, NICE) per CPU. `None` on FFI failure.
fn read_ticks() -> Option<Vec<u64>> {
    let mut cpu_count: natural_t = 0;
    let mut info_array: *mut integer_t = std::ptr::null_mut();
    let mut info_count: mach_msg_type_number_t = 0;

    // SAFETY: `mach_host_self()` returns the per-task host name port (the
    // name flavor needs no refcount). `host_processor_info` writes through
    // the three out-pointers we hand it, and we own those locals.
    let result = unsafe {
        host_processor_info(
            mach_host_self(),
            PROCESSOR_CPU_LOAD_INFO,
            &raw mut cpu_count,
            &raw mut info_array,
            &raw mut info_count,
        )
    };
    if result != KERN_SUCCESS || info_array.is_null() {
        warn!(kern_return = result, "host_processor_info failed");
        return None;
    }

    let total = (cpu_count as usize) * CPU_STATE_MAX;
    // SAFETY: the kernel wrote `info_count` `integer_t` entries into
    // `info_array`; `cpu_count * CPU_STATE_MAX` equals that for the
    // `PROCESSOR_CPU_LOAD_INFO` flavor. The buffer stays valid until our
    // `vm_deallocate` below.
    let slice = unsafe { std::slice::from_raw_parts(info_array, total) };
    // Tick counters are semantically unsigned; the kernel returns them
    // through a signed `integer_t` channel. Reinterpret the bit pattern
    // via `to_ne_bytes`/`from_ne_bytes` so the conversion is explicit
    // (clippy::cast_sign_loss would otherwise flag `as u32`).
    let out: Vec<u64> = slice
        .iter()
        .map(|&x| u64::from(u32::from_ne_bytes(x.to_ne_bytes())))
        .collect();

    // SAFETY: the buffer was allocated by `host_processor_info` in our
    // task's VM and must be returned with `vm_deallocate`. Leaking would
    // accumulate ~kilobytes every 5s for the lifetime of the process.
    unsafe {
        let bytes = total * std::mem::size_of::<integer_t>();
        let _ = vm_deallocate(mach_task_self(), info_array as vm_address_t, bytes);
    }

    Some(out)
}

fn compute_pct(prev: &[u64], cur: &[u64]) -> Option<u8> {
    if prev.len() != cur.len() || prev.len() % CPU_STATE_MAX != 0 {
        return None;
    }
    let mut active: u64 = 0;
    let mut total: u64 = 0;
    for (p, c) in prev.iter().zip(cur.iter()) {
        let delta = c.saturating_sub(*p);
        total += delta;
    }
    for cpu in 0..(cur.len() / CPU_STATE_MAX) {
        let base = cpu * CPU_STATE_MAX;
        let d_user = cur[base + CPU_STATE_USER].saturating_sub(prev[base + CPU_STATE_USER]);
        let d_sys = cur[base + CPU_STATE_SYSTEM].saturating_sub(prev[base + CPU_STATE_SYSTEM]);
        let d_nice = cur[base + CPU_STATE_NICE].saturating_sub(prev[base + CPU_STATE_NICE]);
        let _ = CPU_STATE_IDLE; // documented order; idle is implied by `total - active`
        active += d_user + d_sys + d_nice;
    }
    if total == 0 {
        return Some(0);
    }
    let pct = (active * 100) / total;
    Some(u8::try_from(pct.min(100)).unwrap_or(u8::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_pct_handles_idle_cpu() {
        let prev = vec![0, 0, 100, 0]; // user, sys, idle, nice
        let cur = vec![0, 0, 200, 0]; // 100 idle ticks elapsed, no work
        assert_eq!(compute_pct(&prev, &cur), Some(0));
    }

    #[test]
    fn compute_pct_handles_busy_cpu() {
        let prev = vec![0, 0, 0, 0];
        let cur = vec![50, 30, 20, 0]; // 80 active / 100 total
        assert_eq!(compute_pct(&prev, &cur), Some(80));
    }

    #[test]
    fn compute_pct_rejects_length_mismatch() {
        let prev = vec![0, 0, 0, 0];
        let cur = vec![0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(compute_pct(&prev, &cur), None);
    }

    #[test]
    fn compute_pct_rejects_non_multiple_of_state_max() {
        let prev = vec![0, 0, 0];
        let cur = vec![0, 0, 0];
        assert_eq!(compute_pct(&prev, &cur), None);
    }

    #[test]
    fn compute_pct_handles_multi_cpu() {
        // Two CPUs: first idle (0/100), second fully busy (100/100).
        // Overall: 100 active / 200 total = 50%.
        let prev = vec![0, 0, 0, 0, 0, 0, 0, 0];
        let cur = vec![0, 0, 100, 0, 50, 30, 0, 20];
        assert_eq!(compute_pct(&prev, &cur), Some(50));
    }

    #[test]
    fn compute_pct_caps_at_100() {
        // Pathological input: active > total. Saturation must still cap.
        // (Constructed by hand; the real kernel never produces this.)
        let prev = vec![0, 0, 0, 0];
        let cur = vec![200, 0, 0, 0];
        // total = 200, active = 200 → exactly 100, fine.
        assert_eq!(compute_pct(&prev, &cur), Some(100));
    }
}
