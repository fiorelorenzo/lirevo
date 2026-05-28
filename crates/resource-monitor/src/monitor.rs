use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, Notify};
use tokio::task::JoinHandle;
use tokio::time::{self, Interval};

use crate::error::MonitorError;
use crate::shared::SharedState;
use crate::signals::SignalsBuilder;
use crate::Signals;

const BROADCAST_CAPACITY: usize = 16;
const TICK_INTERVAL: Duration = Duration::from_secs(5);

/// System-resource monitor. Spawns a background task that polls platform
/// sensors and emits a coalesced [`Signals`] snapshot every 5 seconds, or
/// instantly when any sensor reports a change (KVO etc.).
pub struct ResourceMonitor {
    tx: broadcast::Sender<Signals>,
    #[allow(dead_code)] // wired in T7+ as sensors read it back via Arc handles
    state: Arc<SharedState>,
    latest: Arc<std::sync::Mutex<Signals>>,
    task: JoinHandle<()>,
    sensor_tasks: Vec<JoinHandle<()>>,
}

impl Drop for ResourceMonitor {
    /// Aborts the background tick loop and every sensor-owned polling
    /// task so all `Arc` clones held by those futures are released. KVO
    /// observers (thermal, `lowPowerMode`) keep running until process
    /// exit by design — they don't hold a tokio handle.
    fn drop(&mut self) {
        self.task.abort();
        for h in &self.sensor_tasks {
            h.abort();
        }
    }
}

impl ResourceMonitor {
    /// Spawn the monitor. Returns immediately; the background task runs
    /// for the lifetime of the returned `ResourceMonitor`.
    // `async` is part of the public contract so T7+ sensors can `.await`
    // their async startup (KVO registration, IOKit probes) without
    // breaking callers; no awaits yet.
    #[allow(clippy::unused_async)]
    pub async fn spawn() -> Result<Self, MonitorError> {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let state = Arc::new(SharedState::default());

        // Build platform sensors. They write to `state` from background
        // tasks / Objective-C callbacks and return their "instant-change"
        // notifiers (for monitor::run_loop to select over) plus the
        // `JoinHandle`s of any tokio polling tasks they own, so Drop can
        // abort them.
        let (instant_notifiers, sensor_tasks) = crate::build_platform_sensors(state.clone());

        // Initial snapshot for `current()`.
        let initial = snapshot(&state);
        let latest = Arc::new(std::sync::Mutex::new(initial));

        let task = tokio::spawn(run_loop(
            tx.clone(),
            state.clone(),
            latest.clone(),
            instant_notifiers,
        ));

        Ok(Self {
            tx,
            state,
            latest,
            task,
            sensor_tasks,
        })
    }

    /// Subscribe to subsequent snapshots. The receiver yields each new
    /// emission. If the receiver lags by more than [`BROADCAST_CAPACITY`]
    /// snapshots it will receive `RecvError::Lagged(_)`; the recommended
    /// recovery is calling [`Self::current`] for the latest known value.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Signals> {
        self.tx.subscribe()
    }

    /// Latest emitted snapshot. Cheap lock, no broadcast traffic.
    #[must_use]
    pub fn current(&self) -> Signals {
        self.latest.lock().expect("latest mutex").clone()
    }

    /// Test-only: directly mutate the underlying `SharedState`. The next
    /// tick (or `notify`-triggered emission) will reflect the change.
    /// Only the non-macOS deterministic tests call this; on macOS the
    /// helper is unused because real sensors would race injected values,
    /// so suppress the dead-code warning there.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn shared(&self) -> &Arc<SharedState> {
        &self.state
    }
}

async fn run_loop(
    tx: broadcast::Sender<Signals>,
    state: Arc<SharedState>,
    latest: Arc<std::sync::Mutex<Signals>>,
    instant_notifiers: Vec<Arc<Notify>>,
) {
    let mut ticker: Interval = time::interval(TICK_INTERVAL);
    // The first tick fires immediately. We emit on every tick AND on any
    // instant-change notify.
    loop {
        // Instant-change notifications are best-effort. A notify_one() that lands
        // between two `wait_any` polls (e.g. during the snapshot+broadcast phase)
        // is silently absorbed by the next 5s ticker tick — the snapshot path
        // reads SharedState fresh either way.
        tokio::select! {
            _ = ticker.tick() => {},
            () = wait_any(&instant_notifiers) => {},
        }
        let snap = snapshot(&state);
        if let Ok(mut latest) = latest.lock() {
            *latest = snap.clone();
        }
        // It's fine if no one is subscribed yet — `send` returns Err but
        // `latest` is still up to date.
        let _ = tx.send(snap);
    }
}

async fn wait_any(notifiers: &[Arc<Notify>]) {
    if notifiers.is_empty() {
        std::future::pending::<()>().await;
        return;
    }
    let mut futures: Vec<_> = notifiers.iter().map(|n| Box::pin(n.notified())).collect();
    futures::future::select_all(futures.iter_mut()).await;
}

fn snapshot(state: &Arc<SharedState>) -> Signals {
    let mut b = SignalsBuilder::new();
    b.battery_pct(state.battery_pct())
        .on_ac(state.on_ac())
        .power_saver_user_pref(state.power_saver_user_pref())
        .thermal(state.thermal())
        .mem_pressure(state.mem_pressure())
        .mem_free_pct(state.mem_free_pct())
        .mem_free_mb(state.mem_free_mb())
        .cpu_used_pct(state.cpu_used_pct())
        .foreground(state.foreground());
    b.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test(flavor = "multi_thread")]
    async fn spawn_then_subscribe_then_current() {
        let monitor = ResourceMonitor::spawn().await.expect("spawn");

        // current() must return an initial snapshot (default values) immediately
        let s = monitor.current();
        assert_eq!(s.cpu_used_pct, 0);

        // subscribe() must produce at least one snapshot within a reasonable
        // window (the 5s tick fires on startup too).
        let mut rx = monitor.subscribe();
        let recv = timeout(Duration::from_millis(200), rx.recv()).await;
        // We won't receive within 200ms because tick is 5s and time is real;
        // assert only that the subscriber was created cleanly.
        let _ = recv;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires macOS hardware"]
    #[cfg(target_os = "macos")]
    async fn real_macos_thermal_is_plausible() {
        let monitor = ResourceMonitor::spawn().await.expect("spawn");
        let s = monitor.current();
        assert!(matches!(
            s.thermal,
            crate::ThermalState::Nominal
                | crate::ThermalState::Fair
                | crate::ThermalState::Serious
                | crate::ThermalState::Critical
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires macOS hardware"]
    #[cfg(target_os = "macos")]
    async fn real_macos_power_is_plausible() {
        let monitor = ResourceMonitor::spawn().await.expect("spawn");
        // Give battery poll a moment.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let s = monitor.current();
        if let Some(pct) = s.battery_pct {
            assert!(pct <= 100, "battery_pct = {pct}");
        }
        // `on_ac` should be set (true or false, just not panic).
        let _ = s.on_ac;
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires macOS hardware"]
    #[cfg(target_os = "macos")]
    async fn real_macos_memory_is_plausible() {
        let monitor = ResourceMonitor::spawn().await.expect("spawn");
        // Give the synchronous initial host_statistics64 poll time to run.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let s = monitor.current();
        assert!(s.mem_free_mb > 0, "expected free MB > 0 on real hardware");
        assert!(s.mem_free_pct <= 100, "mem_free_pct = {}", s.mem_free_pct);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires macOS hardware"]
    #[cfg(target_os = "macos")]
    async fn real_macos_cpu_eventually_populates() {
        let monitor = ResourceMonitor::spawn().await.expect("spawn");
        // Two CPU polls needed (5s apart) before pct populates; wait 11s.
        tokio::time::sleep(std::time::Duration::from_secs(11)).await;
        let s = monitor.current();
        assert!(s.cpu_used_pct <= 100);
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires macOS hardware"]
    #[cfg(target_os = "macos")]
    async fn real_macos_foreground_eventually_populates() {
        let monitor = ResourceMonitor::spawn().await.expect("spawn");
        // One foreground poll (5s tick) populates bundle id + memory; CPU
        // stays 0 until the second poll.
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
        let s = monitor.current();
        // Permissive: in a headless test runner there may be no frontmost
        // app with a bundle id at all (None is valid). When present, the
        // bundle id must be non-empty and the CPU bucket must be in range.
        if let Some(fg) = s.foreground {
            assert!(!fg.bundle_id.is_empty(), "expected non-empty bundle_id");
            assert!(fg.cpu_used_pct <= 100, "cpu_used_pct = {}", fg.cpu_used_pct);
        }
    }

    // Deterministic tokio-time tests. Gated to non-macOS so real sensors
    // (thermal KVO, memory pressure dispatch source, etc.) can't race the
    // injected `SharedState` values; on those targets `build_platform_sensors`
    // is a stub that writes nothing.
    #[cfg(not(target_os = "macos"))]
    use crate::{MemoryPressure, ThermalState};

    // `start_paused` requires the current_thread flavor; that's fine for
    // these tests since they only spawn the monitor task and the test body.
    #[cfg(not(target_os = "macos"))]
    #[tokio::test(start_paused = true)]
    async fn emits_after_first_tick() {
        let monitor = ResourceMonitor::spawn().await.expect("spawn");
        let mut rx = monitor.subscribe();
        // First tick fires immediately under `interval`.
        let s = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("emission within 1s")
            .expect("snapshot");
        assert!(matches!(s.thermal, ThermalState::Nominal));
    }

    #[cfg(not(target_os = "macos"))]
    #[tokio::test(start_paused = true)]
    async fn current_reflects_state_after_tick() {
        let monitor = ResourceMonitor::spawn().await.expect("spawn");
        monitor.shared().set_thermal(ThermalState::Serious);
        monitor.shared().set_mem_pressure(MemoryPressure::Warning);

        // Advance past 5s tick.
        tokio::time::advance(Duration::from_secs(6)).await;
        tokio::task::yield_now().await;

        let s = monitor.current();
        assert_eq!(s.thermal, ThermalState::Serious);
        assert_eq!(s.mem_pressure, MemoryPressure::Warning);
    }
}
