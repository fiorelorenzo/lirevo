//! Foreground app sensor.
//!
//! - Bundle id: `NSWorkspace.sharedWorkspace.frontmostApplication.bundleIdentifier`
//!   (instant KVO via `NSWorkspaceDidActivateApplicationNotification`, posted
//!   on `NSWorkspace.notificationCenter`, NOT the default center).
//! - CPU + memory of that app: `proc_pidinfo(PROC_PIDTASKINFO)` polled every
//!   5s. Per-process CPU% is approximated as `(delta_total_ticks * 100) /
//!   elapsed_ns`. On Apple Silicon (M1+) `mach_timebase_info` is 1/1 so a
//!   tick equals a nanosecond — adequate for the 0..=100 bucket we emit.
//!
//! If the frontmost app has no bundle id (login window, Finder activation
//! glitch, transient process) `foreground` is `None`; the score function in
//! the controller treats that as "neutral".

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSRunningApplication, NSWorkspace, NSWorkspaceDidActivateApplicationNotification};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::warn;

use crate::ForegroundApp;
use crate::shared::SharedState;

const POLL_INTERVAL: Duration = Duration::from_secs(5);

// proc_pidinfo FFI. `PROC_PIDTASKINFO` lives in `<sys/proc_info.h>`; the
// libc crate doesn't re-export it on every target, so we declare it here
// alongside the layout of `proc_taskinfo` (also from `<sys/proc_info.h>`).
const PROC_PIDTASKINFO: i32 = 4;

// The `pti_*` prefix matches the kernel ABI for `<sys/proc_info.h>`;
// renaming would obscure the mapping to the documented field names.
#[allow(clippy::struct_field_names)]
#[repr(C)]
#[derive(Default, Copy, Clone)]
struct ProcTaskInfo {
    pti_virtual_size: u64,
    pti_resident_size: u64,
    pti_total_user: u64,
    pti_total_system: u64,
    pti_threads_user: u64,
    pti_threads_system: u64,
    pti_policy: i32,
    pti_faults: i32,
    pti_pageins: i32,
    pti_cow_faults: i32,
    pti_messages_sent: i32,
    pti_messages_received: i32,
    pti_syscalls_mach: i32,
    pti_syscalls_unix: i32,
    pti_csw: i32,
    pti_threadnum: i32,
    pti_numrunning: i32,
    pti_priority: i32,
}

// `proc_pidinfo` takes a buffer size as `int`. The size of `ProcTaskInfo` is
// a small constant well under `i32::MAX`, so the cast is sound; compute it
// once and `const`-assert it stays representable. `i32::try_from` is not
// const, so we hand-roll the bounds check.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "const-asserted to fit in i32 immediately above"
)]
const PROC_PIDTASKINFO_SIZE: i32 = {
    let n = std::mem::size_of::<ProcTaskInfo>();
    assert!(n <= i32::MAX as usize);
    n as i32
};

#[link(name = "System", kind = "dylib")]
unsafe extern "C" {
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        arg: u64,
        buffer: *mut c_void,
        buffersize: i32,
    ) -> i32;
}

/// Tracked state for the current foreground PID. All fields are touched
/// from both the `NSWorkspace` KVO block (on a Foundation thread) and the
/// 5 s tokio polling task, so each is wrapped in a `Mutex`.
struct PidContext {
    pid: Mutex<Option<i32>>,
    bundle_id: Mutex<Option<String>>,
    /// `pti_total_user + pti_total_system` from the previous poll. Reset
    /// to `None` whenever the foreground PID changes so the next poll
    /// starts a fresh delta baseline (rather than reporting a meaningless
    /// CPU% from app A's ticks minus app B's ticks).
    prev_ticks: Mutex<Option<u64>>,
}

/// Spawn the foreground sensor. Returns `(Some(notify), handles)` where
/// `notify_one` is called every time `NSWorkspaceDidActivateApplicationNotification`
/// fires (so `monitor::run_loop` emits an extra off-tick snapshot for the
/// new bundle id), and `handles` carries the 5 s `proc_pidinfo` polling
/// task so [`crate::ResourceMonitor`] can abort it on drop.
#[allow(clippy::needless_pass_by_value, clippy::unnecessary_wraps)]
pub(super) fn spawn(state: Arc<SharedState>) -> (Option<Arc<Notify>>, Vec<JoinHandle<()>>) {
    let notify = Arc::new(Notify::new());
    let ctx = Arc::new(PidContext {
        pid: Mutex::new(None),
        bundle_id: Mutex::new(None),
        prev_ticks: Mutex::new(None),
    });

    // Seed bundle/pid from the current frontmost app so the first snapshot
    // has the right identity (CPU% will still be 0 until two polls have
    // run, same as the system-wide cpu sensor).
    update_frontmost(&ctx);

    // Subscribe to NSWorkspace activation notifications for instant bundle
    // id changes (Cmd-Tab, click, app launch).
    let ctx_for_block = ctx.clone();
    let notify_for_block = notify.clone();
    let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
        update_frontmost(&ctx_for_block);
        notify_for_block.notify_one();
    });

    // SAFETY: `addObserverForName:object:queue:usingBlock:` retains the
    // block and returns an opaque observer object we must keep alive for
    // the duration we want the callback. We `Retained::into_raw` it
    // because the `ResourceMonitor` lives for the lifetime of the app;
    // wiring `removeObserver:` on shutdown is not part of v0.6. The
    // notification is posted on `NSWorkspace.notificationCenter` (NOT the
    // default center), per Apple's NSWorkspace docs. The
    // `NSWorkspaceDidActivateApplicationNotification` global is loaded via
    // a typed `&'static NSNotificationName`, mirroring how the thermal and
    // power sensors handle their respective NSProcessInfo notifications.
    unsafe {
        let ws: Retained<NSWorkspace> = NSWorkspace::sharedWorkspace();
        let nc: Retained<NSNotificationCenter> = ws.notificationCenter();
        let observer = nc.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidActivateApplicationNotification),
            None,
            None::<&NSOperationQueue>,
            &block,
        );
        // Leak intentionally — see comment above.
        let _ = Retained::into_raw(observer);
    }

    // Poll per-PID CPU + memory every 5 s.
    let ctx_for_poll = ctx.clone();
    let state_for_poll = state.clone();
    let handle = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(POLL_INTERVAL);
        loop {
            ticker.tick().await;
            poll_foreground(&ctx_for_poll, &state_for_poll);
        }
    });

    (Some(notify), vec![handle])
}

fn update_frontmost(ctx: &Arc<PidContext>) {
    let ws = NSWorkspace::sharedWorkspace();
    let Some(app): Option<Retained<NSRunningApplication>> = ws.frontmostApplication() else {
        *ctx.pid.lock().expect("foreground pid mutex") = None;
        *ctx.bundle_id.lock().expect("foreground bundle_id mutex") = None;
        *ctx.prev_ticks.lock().expect("foreground prev_ticks mutex") = None;
        return;
    };
    let pid = app.processIdentifier();
    let bid = app.bundleIdentifier().map(|s| s.to_string());

    *ctx.pid.lock().expect("foreground pid mutex") = Some(pid);
    *ctx.bundle_id.lock().expect("foreground bundle_id mutex") = bid;
    // Reset the previous-tick baseline whenever the foreground PID changes;
    // mixing two processes' tick counters would produce nonsense CPU%.
    *ctx.prev_ticks.lock().expect("foreground prev_ticks mutex") = None;
}

fn poll_foreground(ctx: &Arc<PidContext>, state: &Arc<SharedState>) {
    let pid_opt = *ctx.pid.lock().expect("foreground pid mutex");
    let bid_opt = ctx.bundle_id.lock().expect("foreground bundle_id mutex").clone();

    let (Some(pid), Some(bundle_id)) = (pid_opt, bid_opt) else {
        state.set_foreground(None);
        return;
    };

    let mut info = ProcTaskInfo::default();
    // SAFETY: `proc_pidinfo` writes up to `PROC_PIDTASKINFO_SIZE` bytes into
    // `info`. We pass exactly that size, and `info` is owned on the stack
    // for the duration of the call.
    let n = unsafe {
        proc_pidinfo(
            pid,
            PROC_PIDTASKINFO,
            0,
            std::ptr::from_mut::<ProcTaskInfo>(&mut info).cast::<c_void>(),
            PROC_PIDTASKINFO_SIZE,
        )
    };
    if n != PROC_PIDTASKINFO_SIZE {
        // App may have exited between the KVO callback and this poll, or
        // it may be sandboxed in a way that denies introspection. Either
        // way: leave previous values in place rather than zeroing them.
        warn!(pid, n, "proc_pidinfo PROC_PIDTASKINFO returned unexpected size");
        return;
    }

    // `pti_total_user` + `pti_total_system` are cumulative ticks in
    // `mach_absolute_time` units. On Apple Silicon `mach_timebase_info`
    // returns 1/1 so the unit is nanoseconds; on Intel it's also
    // nanoseconds for this particular kernel API. We approximate elapsed
    // as `POLL_INTERVAL` rather than measuring it (the 5 s tokio ticker
    // is steady to within a few ms — within the rounding error of the
    // 0..=100 bucket).
    let total_ticks = info.pti_total_user.saturating_add(info.pti_total_system);
    let mut prev_guard = ctx.prev_ticks.lock().expect("foreground prev_ticks mutex");
    let cpu_pct: u8 = match *prev_guard {
        Some(p) => {
            let delta = total_ticks.saturating_sub(p);
            let elapsed_ns = u64::try_from(POLL_INTERVAL.as_nanos()).unwrap_or(u64::MAX);
            if elapsed_ns == 0 {
                0
            } else {
                let pct = (delta.saturating_mul(100)) / elapsed_ns;
                u8::try_from(pct.min(100)).unwrap_or(u8::MAX)
            }
        }
        None => 0,
    };
    *prev_guard = Some(total_ticks);
    drop(prev_guard);

    let mem_mb =
        u32::try_from(info.pti_resident_size / 1_048_576).unwrap_or(u32::MAX);

    state.set_foreground(Some(ForegroundApp {
        bundle_id,
        cpu_used_pct: cpu_pct,
        mem_resident_mb: mem_mb,
    }));
}
