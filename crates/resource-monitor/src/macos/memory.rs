//! Memory pressure dispatch source + free-memory polling.
//!
//! `mem_pressure` (Normal/Warning/Critical) is driven by a libdispatch
//! `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE` source we register on the global
//! default-QoS queue at spawn time; the handler runs whenever the kernel
//! transitions between pressure levels.
//!
//! `mem_free_pct` + `mem_free_mb` are polled every 10 s from
//! `host_statistics64(HOST_VM_INFO64)`. mach2 0.4 doesn't expose
//! `host_statistics64`, `mach_host_self`, or the `vm_statistics64` layout,
//! so the FFI declarations live here.

use std::ffi::c_void;
use std::sync::Arc;
use std::time::Duration;

use mach2::kern_return::{kern_return_t, KERN_SUCCESS};
use mach2::mach_types::host_t;
use mach2::vm_page_size::vm_page_size;
use mach2::vm_types::natural_t;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::warn;

use crate::shared::SharedState;
use crate::MemoryPressure;

const POLL_INTERVAL: Duration = Duration::from_secs(10);

const HOST_VM_INFO64: i32 = 4;
// `HOST_VM_INFO64_COUNT` from `<mach/host_info.h>` is the size of
// `vm_statistics64_data_t` in units of `integer_t` (u32). 38 covers the
// 14 `natural_t` (u32) + 12 `u64` fields below.
const HOST_VM_INFO64_COUNT: u32 = 38;

#[repr(C)]
#[derive(Default, Copy, Clone)]
struct VmStatistics64 {
    free_count: natural_t,
    active_count: natural_t,
    inactive_count: natural_t,
    wire_count: natural_t,
    zero_fill_count: u64,
    reactivations: u64,
    pageins: u64,
    pageouts: u64,
    faults: u64,
    cow_faults: u64,
    lookups: u64,
    hits: u64,
    purges: u64,
    purgeable_count: natural_t,
    speculative_count: natural_t,
    decompressions: u64,
    compressions: u64,
    swapins: u64,
    swapouts: u64,
    compressor_page_count: natural_t,
    throttled_count: natural_t,
    external_page_count: natural_t,
    internal_page_count: natural_t,
    total_uncompressed_pages_in_compressor: u64,
}

// Static guard: the kernel writes only the prefix of `VmStatistics64` we
// declare via `HOST_VM_INFO64_COUNT` units. If a future SDK adds a field
// without bumping the count, we'd silently leave it zero-initialised.
const _: () = assert!(
    std::mem::size_of::<VmStatistics64>() / std::mem::size_of::<u32>()
        == HOST_VM_INFO64_COUNT as usize
);

unsafe extern "C" {
    fn mach_host_self() -> host_t;
    fn host_statistics64(
        host_priv: host_t,
        flavor: i32,
        host_info_out: *mut VmStatistics64,
        host_info_out_cnt: *mut u32,
    ) -> kern_return_t;
}

// libdispatch FFI. libdispatch ships as part of libSystem on macOS, so
// no explicit `#[link]` is required — the symbols resolve through the
// default linker search path.
type DispatchSourceT = *mut c_void;
type DispatchQueueT = *mut c_void;
type DispatchFunctionT = extern "C" fn(*mut c_void);

// `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE` is a `&_dispatch_source_type_memorypressure`
// pointer in the SDK. We expose it as a function so the address-of-extern
// expression isn't required to be a `const` initializer (which stable Rust
// disallows for non-`const` extern statics).
unsafe extern "C" {
    static _dispatch_source_type_memorypressure: c_void;
}
fn dispatch_source_type_memorypressure() -> *const c_void {
    // `_dispatch_source_type_memorypressure` is a process-wide immutable
    // symbol exported by libdispatch; taking its address via `&raw const`
    // on an extern static does not require an `unsafe` block under the
    // 2024 raw-ref rules used here.
    &raw const _dispatch_source_type_memorypressure
}

const DISPATCH_MEMORYPRESSURE_NORMAL: usize = 0x01;
const DISPATCH_MEMORYPRESSURE_WARN: usize = 0x02;
const DISPATCH_MEMORYPRESSURE_CRITICAL: usize = 0x04;

// QoS class for `dispatch_get_global_queue`. We don't need the main queue
// for the pressure handler — a background task that writes one atomic and
// pings a `Notify` is fine on any non-UI queue.
const QOS_CLASS_UTILITY: isize = 0x11;

unsafe extern "C" {
    fn dispatch_source_create(
        type_: *const c_void,
        handle: usize,
        mask: usize,
        queue: DispatchQueueT,
    ) -> DispatchSourceT;
    fn dispatch_source_set_event_handler_f(source: DispatchSourceT, handler: DispatchFunctionT);
    fn dispatch_source_get_data(source: DispatchSourceT) -> usize;
    fn dispatch_set_context(object: *mut c_void, context: *mut c_void);
    fn dispatch_resume(object: *mut c_void);
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> DispatchQueueT;
}

struct PressureCtx {
    state: Arc<SharedState>,
    notify: Arc<Notify>,
    source: DispatchSourceT,
}

// The context is read-only from the handler thread (we only call
// `set_mem_pressure` and `notify_one`, both internally synchronized).
unsafe impl Send for PressureCtx {}
unsafe impl Sync for PressureCtx {}

extern "C" fn pressure_handler(ctx: *mut c_void) {
    // SAFETY: ctx is the `Box::leak`ed `PressureCtx` we attached via
    // `dispatch_set_context`. It lives for the lifetime of the dispatch
    // source (which itself outlives the process by design — we never
    // release it). The cast back to `&PressureCtx` is sound as long as no
    // one else writes through that pointer, which we guarantee.
    let ctx = unsafe { &*ctx.cast::<PressureCtx>() };
    // SAFETY: `dispatch_source_get_data` is a stable libdispatch call.
    let data = unsafe { dispatch_source_get_data(ctx.source) };
    let v = if data & DISPATCH_MEMORYPRESSURE_CRITICAL != 0 {
        MemoryPressure::Critical
    } else if data & DISPATCH_MEMORYPRESSURE_WARN != 0 {
        MemoryPressure::Warning
    } else {
        MemoryPressure::Normal
    };
    ctx.state.set_mem_pressure(v);
    ctx.notify.notify_one();
}

/// Spawn the memory sensor. Returns `(Some(notify), handles)` where
/// `notify_one` fires whenever the libdispatch memory-pressure source
/// emits, and `handles` carries the 10 s `host_statistics64` polling task
/// so [`crate::ResourceMonitor`] can abort it on drop.
#[allow(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
pub(super) fn spawn(state: Arc<SharedState>) -> (Option<Arc<Notify>>, Vec<JoinHandle<()>>) {
    let notify = Arc::new(Notify::new());

    // SAFETY: libdispatch stores the `context` pointer verbatim and does NOT
    // retain it. We transfer ownership via `Box::leak` so the allocation
    // outlives every handler invocation. The dispatch source itself is never
    // released (lifetime-of-process), so the ctx pointer is always valid when
    // `pressure_handler` fires. This matches the pattern used by
    // `thermal::spawn` for the KVO observer.
    unsafe {
        let mask = DISPATCH_MEMORYPRESSURE_NORMAL
            | DISPATCH_MEMORYPRESSURE_WARN
            | DISPATCH_MEMORYPRESSURE_CRITICAL;
        let queue = dispatch_get_global_queue(QOS_CLASS_UTILITY, 0);
        if queue.is_null() {
            warn!("dispatch_get_global_queue returned null; memory-pressure source not armed");
        } else {
            let source =
                dispatch_source_create(dispatch_source_type_memorypressure(), 0, mask, queue);
            if source.is_null() {
                warn!("dispatch_source_create memorypressure returned null");
            } else {
                let ctx = Box::leak(Box::new(PressureCtx {
                    state: state.clone(),
                    notify: notify.clone(),
                    source,
                }));
                dispatch_set_context(
                    source.cast::<c_void>(),
                    std::ptr::from_mut::<PressureCtx>(ctx).cast(),
                );
                dispatch_source_set_event_handler_f(source, pressure_handler);
                dispatch_resume(source.cast::<c_void>());
            }
        }
    }

    // Initial poll + 10s tokio ticker.
    poll_vm_once(&state);
    let state_for_poll = state.clone();
    let handle = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(POLL_INTERVAL);
        // First tick fires immediately and we already polled above; skip it.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            poll_vm_once(&state_for_poll);
        }
    });

    (Some(notify), vec![handle])
}

fn poll_vm_once(state: &Arc<SharedState>) {
    let mut vm = VmStatistics64::default();
    let mut count = HOST_VM_INFO64_COUNT;
    // SAFETY: `mach_host_self()` returns the per-task host name port (no
    // refcount required for the name flavor we use). `host_statistics64`
    // writes `count` u32-sized fields into `vm`; we sized
    // `HOST_VM_INFO64_COUNT` to match `vm_statistics64_data_t`.
    let result = unsafe {
        host_statistics64(
            mach_host_self(),
            HOST_VM_INFO64,
            &raw mut vm,
            &raw mut count,
        )
    };
    if result != KERN_SUCCESS {
        warn!(kern_return = result, "host_statistics64 failed");
        return;
    }

    // `vm_page_size` is an extern static populated by libSystem at load
    // time. On Apple Silicon it's 16 KiB; on x86_64 it's 4 KiB.
    // SAFETY: read of an immutable extern static initialized before main.
    let page_size_bytes = unsafe { vm_page_size } as u64;

    let free_pages = u64::from(vm.free_count) + u64::from(vm.speculative_count);
    let active_pages = u64::from(vm.active_count);
    let inactive_pages = u64::from(vm.inactive_count);
    let wire_pages = u64::from(vm.wire_count);
    let total_pages = free_pages + active_pages + inactive_pages + wire_pages;

    if total_pages == 0 {
        return;
    }

    // u32 caps at ~4 PiB worth of MB, way above any realistic RAM total,
    // so the `as u32` cast is safe.
    let free_mb = u32::try_from(free_pages * page_size_bytes / 1_048_576).unwrap_or(u32::MAX);
    let free_pct = u8::try_from(((free_pages * 100) / total_pages).min(100)).unwrap_or(u8::MAX);
    state.set_mem_free_mb(free_mb);
    state.set_mem_free_pct(free_pct);
}
