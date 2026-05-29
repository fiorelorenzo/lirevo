//! Energy-profile selection policy.
//!
//! Consumes [`resource_monitor::Signals`] and decides the active
//! [`ProfileName`] (`PowerSaver` / `Balanced` / `Performance`) via additive
//! scoring, hysteresis, an Auto/PinnedSoft override FSM, and emergency
//! triggers. A profile maps to a [`ProfilePolicy`] (resource knobs:
//! idle-unload timeouts, thread count, learning pace), consumed by the
//! M5.3 Engine — not yet wired.
//!
//! The decision logic is split into pure free functions + a [`Decider`]
//! that takes an injected `Instant` (deterministically testable) and a
//! thin async [`ProfileSelector`] shell that consumes a broadcast of
//! `Signals`.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use resource_monitor::{MemoryPressure, Signals, ThermalState};
use serde::Serialize;

/// The three energy profiles. Ordered conceptually from least to most
/// resource-hungry, but `Ord` is intentionally not derived — there is no
/// meaningful "greater than" between profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum ProfileName {
    PowerSaver,
    Balanced,
    Performance,
}

/// How the active profile is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ProfileMode {
    /// The selector decides via scoring + hysteresis.
    Auto,
    /// User pinned a profile. Honoured except when an emergency trigger
    /// forces `PowerSaver` (override-of-override).
    PinnedSoft(ProfileName),
}

/// CPU threading hint passed to the LLM backend per profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum NThreads {
    /// Only efficiency cores.
    EcoOnly,
    /// Default mixed scheduling (`llama.cpp` `n_threads = -1` style).
    Mixed,
    /// All performance cores.
    AllPCores,
}

/// How aggressively the (future) continuous-learning worker runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum LearningPace {
    /// Worker does not run.
    Off,
    /// Background, only when the system is idle and on AC.
    Slow,
    /// Background, whenever not under pressure.
    Normal,
    /// Foreground priority, always trying.
    Aggressive,
}

/// The bundle of resource knobs a profile maps to. Consumed by the M5.3
/// Engine to drive idle-unload timing, threading, batch size, and the
/// learning worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfilePolicy {
    pub name: ProfileName,

    // Idle-unload behaviour.
    pub llm_idle_unload: Duration,
    pub stt_idle_unload: Duration,

    // CPU / threading.
    pub n_threads: NThreads,
    pub batch_size: u32,

    // Learning behaviour.
    pub learning_pace: LearningPace,

    /// Below this battery percentage (on battery), the Engine unloads the
    /// LLM aggressively. `0` = never; `100` = always.
    pub unload_below_battery_pct: u8,
}

pub const POWER_SAVER: ProfilePolicy = ProfilePolicy {
    name: ProfileName::PowerSaver,
    llm_idle_unload: Duration::from_secs(10),
    stt_idle_unload: Duration::from_secs(60),
    n_threads: NThreads::EcoOnly,
    batch_size: 1,
    learning_pace: LearningPace::Slow,
    unload_below_battery_pct: 50,
};

pub const BALANCED: ProfilePolicy = ProfilePolicy {
    name: ProfileName::Balanced,
    llm_idle_unload: Duration::from_secs(120),
    stt_idle_unload: Duration::from_secs(300),
    n_threads: NThreads::Mixed,
    batch_size: 4,
    learning_pace: LearningPace::Normal,
    unload_below_battery_pct: 20,
};

pub const PERFORMANCE: ProfilePolicy = ProfilePolicy {
    name: ProfileName::Performance,
    llm_idle_unload: Duration::from_secs(600),
    stt_idle_unload: Duration::from_secs(900),
    n_threads: NThreads::AllPCores,
    batch_size: 8,
    learning_pace: LearningPace::Aggressive,
    unload_below_battery_pct: 0,
};

/// The `ProfilePolicy` for a given profile.
#[must_use]
pub fn policy_for(name: ProfileName) -> ProfilePolicy {
    match name {
        ProfileName::PowerSaver => POWER_SAVER,
        ProfileName::Balanced => BALANCED,
        ProfileName::Performance => PERFORMANCE,
    }
}

/// Bundle ids of apps known to be sustained-heavy CPU consumers. Not
/// exhaustive — the `cpu > 50` generic branch in `score` catches anything
/// missed. Exact-match; version-suffixed bundle ids fall through to the
/// generic branch.
const KNOWN_HEAVY_APPS: &[&str] = &[
    "com.blackmagic-design.DaVinciResolve",
    "com.adobe.PremierePro",
    "com.apple.FinalCut",
    "org.blenderfoundation.blender",
    "net.maxon.cinema4d",
    "com.apple.logic10",
    "com.apple.dt.Xcode",
];

fn is_heavy_app(bundle_id: &str) -> bool {
    KNOWN_HEAVY_APPS.contains(&bundle_id)
}

/// Additive resource-pressure score. Higher = more pressure = bias toward
/// `PowerSaver`. Typical range 0..130 (can go to -20 when on AC and idle).
/// Pure: no clock, no I/O. See spec §4 "Scoring".
#[must_use]
pub fn score(s: &Signals) -> i32 {
    let mut total = 0;

    // Power.
    if s.on_ac {
        total -= 20;
    } else if let Some(b) = s.battery_pct {
        if b < 20 {
            total += 40;
        } else if b < 50 {
            // (50 - b) * 0.6, integer round-half-up.
            total += (i32::from(50 - b) * 6 + 5) / 10;
        }
    }

    // Thermal.
    total += match s.thermal {
        ThermalState::Nominal => 0,
        ThermalState::Fair => 15,
        ThermalState::Serious => 40,
        ThermalState::Critical => 80,
    };

    // Memory. The +50 case (Critical pressure OR very low free) wins over
    // the +30 Warning case.
    if s.mem_pressure == MemoryPressure::Critical || s.mem_free_pct < 15 {
        total += 50;
    } else if s.mem_pressure == MemoryPressure::Warning && s.mem_free_pct < 30 {
        total += 30;
    }

    // CPU.
    if s.cpu_used_pct > 70 {
        total += i32::from(s.cpu_used_pct - 70) / 2;
    }

    // Foreground app.
    if let Some(fg) = &s.foreground {
        if fg.cpu_used_pct > 30 && is_heavy_app(&fg.bundle_id) {
            total += 25;
        } else if fg.cpu_used_pct > 50 {
            total += 15;
        }
    }

    total
}

/// Map a score to a profile band. See spec §4 "Profile bands".
#[must_use]
pub fn band_for_score(score: i32) -> ProfileName {
    if score < 25 {
        ProfileName::Performance
    } else if score < 65 {
        ProfileName::Balanced
    } else {
        ProfileName::PowerSaver
    }
}

/// Why an emergency forced `PowerSaver`. Surfaced to the UI (toast) by the
/// M5.3+ wiring; carried here so the Decider can report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EmergencyReason {
    LowPowerMode,
    ThermalCritical,
    MemoryCritical,
    BatteryCritical,
}

/// If any emergency trigger is active, return the highest-priority reason
/// (checked in the order: low-power-mode, thermal, memory, battery). Any
/// `Some(_)` forces `PowerSaver` regardless of scoring or pinned mode.
/// `battery_pct == None` never triggers the battery case.
#[must_use]
pub fn emergency_target(s: &Signals) -> Option<EmergencyReason> {
    if s.power_saver_user_pref {
        Some(EmergencyReason::LowPowerMode)
    } else if s.thermal == ThermalState::Critical {
        Some(EmergencyReason::ThermalCritical)
    } else if s.mem_pressure == MemoryPressure::Critical {
        Some(EmergencyReason::MemoryCritical)
    } else if matches!(s.battery_pct, Some(b) if b < 5) {
        Some(EmergencyReason::BatteryCritical)
    } else {
        None
    }
}

const HISTORY_WINDOW: Duration = Duration::from_secs(30);
const MIN_DWELL: Duration = Duration::from_secs(30);

/// Pure decision engine. Owns hysteresis history + override state. Takes
/// an injected `Instant` per [`Decider::observe`] so tests are
/// deterministic (no wall clock). Not thread-safe on its own; the async
/// shell wraps it in a `Mutex`.
pub(crate) struct Decider {
    mode: ProfileMode,
    decided: ProfileName,
    last_change: Instant,
    history: VecDeque<(Instant, i32)>,
    emergency: Option<EmergencyReason>,
}

impl Decider {
    pub(crate) fn new(mode: ProfileMode, initial: ProfileName, now: Instant) -> Self {
        Self {
            mode,
            decided: initial,
            last_change: now,
            history: VecDeque::new(),
            emergency: None,
        }
    }

    pub(crate) fn decided(&self) -> ProfileName {
        self.decided
    }

    pub(crate) fn mode(&self) -> ProfileMode {
        self.mode
    }

    // Consumed by M5.3 toast wiring; not yet read by the async shell.
    #[allow(dead_code)]
    pub(crate) fn emergency(&self) -> Option<EmergencyReason> {
        self.emergency
    }

    /// User override. Takes effect on the next `observe`.
    pub(crate) fn set_mode(&mut self, mode: ProfileMode) {
        self.mode = mode;
    }

    /// Feed one signal sample. Updates internal state and returns the
    /// freshly-decided profile.
    pub(crate) fn observe(&mut self, signals: &Signals, now: Instant) -> ProfileName {
        let emergency = emergency_target(signals);
        self.emergency = emergency;

        // Any emergency forces PowerSaver immediately, regardless of mode,
        // bypassing hysteresis.
        if emergency.is_some() {
            self.decided = ProfileName::PowerSaver;
            // Refresh the dwell timer on every emergency sample. When the
            // emergency clears, the Auto path must re-earn the full 30s dwell
            // (and rebuild its score history, which is not accumulated during
            // an emergency since this branch returns early) before changing —
            // no instant max-swing on a single stale sample.
            self.last_change = now;
            return self.decided;
        }

        match self.mode {
            ProfileMode::PinnedSoft(p) => {
                // No emergency: honour the pin. Immediate (the user asked
                // for it explicitly), no hysteresis.
                if self.decided != p {
                    self.decided = p;
                    self.last_change = now;
                }
            }
            ProfileMode::Auto => {
                let s = score(signals);
                self.push_history(now, s);
                let avg = self.mean_recent(now);
                let target = band_for_score(avg);
                if target != self.decided && now.duration_since(self.last_change) >= MIN_DWELL {
                    self.decided = target;
                    self.last_change = now;
                }
            }
        }

        self.decided
    }

    fn push_history(&mut self, now: Instant, s: i32) {
        self.history.push_back((now, s));
        // Trim anything older than the window.
        while let Some(&(t, _)) = self.history.front() {
            if now.duration_since(t) > HISTORY_WINDOW {
                self.history.pop_front();
            } else {
                break;
            }
        }
    }

    fn mean_recent(&self, now: Instant) -> i32 {
        let mut sum: i64 = 0;
        let mut count: i64 = 0;
        for (t, s) in &self.history {
            if now.duration_since(*t) <= HISTORY_WINDOW {
                sum += i64::from(*s);
                count += 1;
            }
        }
        if count == 0 {
            return 0;
        }
        // Scores are bounded ~[-20, 130], so the mean is always within
        // i32 range; `try_from` keeps clippy::pedantic happy without an
        // allow and the fallback is unreachable.
        i32::try_from(sum / count).unwrap_or(0)
    }
}

use std::sync::{Arc, Mutex};

use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

struct Inner {
    decider: Mutex<Decider>,
    /// Published profile, for UI subscribers.
    changes_tx: watch::Sender<ProfileName>,
    /// The profile last handed to a consumer via `take_pending_apply`.
    /// A change is "pending" when `decided != applied`.
    applied: Mutex<ProfileName>,
}

/// Async front-end over the [`Decider`]. Consumes a broadcast of
/// [`Signals`], updates the decision on each sample, and exposes the
/// current profile + a pending-apply hook for the Engine and a watch
/// channel for the UI.
pub struct ProfileSelector {
    inner: Arc<Inner>,
    task: JoinHandle<()>,
}

impl ProfileSelector {
    /// Spawn the selector. `initial` is the profile to start from before
    /// any signal arrives.
    #[must_use]
    pub fn new(
        rx: broadcast::Receiver<Signals>,
        mode: ProfileMode,
        initial: ProfileName,
    ) -> Arc<Self> {
        let (changes_tx, _) = watch::channel(initial);
        let inner = Arc::new(Inner {
            decider: Mutex::new(Decider::new(mode, initial, Instant::now())),
            changes_tx,
            applied: Mutex::new(initial),
        });

        let task_inner = inner.clone();
        let task = tokio::spawn(run_loop(task_inner, rx));

        Arc::new(Self { inner, task })
    }

    #[must_use]
    pub fn current_profile(&self) -> ProfileName {
        self.inner.decider.lock().expect("decider mutex").decided()
    }

    #[must_use]
    pub fn current_policy(&self) -> ProfilePolicy {
        policy_for(self.current_profile())
    }

    #[must_use]
    pub fn current_mode(&self) -> ProfileMode {
        self.inner.decider.lock().expect("decider mutex").mode()
    }

    /// User override (Auto vs `PinnedSoft`). Takes effect on the next signal.
    pub fn set_mode(&self, mode: ProfileMode) {
        self.inner
            .decider
            .lock()
            .expect("decider mutex")
            .set_mode(mode);
    }

    /// Returns the next-desired policy if the decided profile has changed
    /// since the last call (consumes the pending change); `None` otherwise.
    /// The Engine calls this at the end-of-dictation boundary.
    // Not `#[must_use]`: the call has a side effect (clears the pending
    // change), so dropping the result to merely acknowledge it is valid.
    #[allow(clippy::must_use_candidate)]
    pub fn take_pending_apply(&self) -> Option<ProfilePolicy> {
        let decided = self.current_profile();
        let mut applied = self.inner.applied.lock().expect("applied mutex");
        if decided == *applied {
            None
        } else {
            *applied = decided;
            Some(policy_for(decided))
        }
    }

    /// Watch channel of decided-profile changes, for the UI.
    #[must_use]
    pub fn subscribe_changes(&self) -> watch::Receiver<ProfileName> {
        self.inner.changes_tx.subscribe()
    }
}

impl Drop for ProfileSelector {
    fn drop(&mut self) {
        // Stop the background consumer so it releases its `Arc<Inner>`.
        self.task.abort();
    }
}

async fn run_loop(inner: Arc<Inner>, mut rx: broadcast::Receiver<Signals>) {
    loop {
        match rx.recv().await {
            Ok(signals) => {
                let decided = {
                    let mut decider = inner.decider.lock().expect("decider mutex");
                    decider.observe(&signals, Instant::now())
                };
                // Publish to UI subscribers only when the value actually
                // changes (send_if_modified avoids spurious wakeups).
                inner.changes_tx.send_if_modified(|cur| {
                    if *cur == decided {
                        false
                    } else {
                        *cur = decided;
                        true
                    }
                });
            }
            // Lagged: we missed some signals; the next recv resyncs. Keep
            // going — the decider reads each fresh sample anyway.
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            // Sender dropped: no more signals will arrive, exit the loop.
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_constants_match_spec() {
        assert_eq!(POWER_SAVER.name, ProfileName::PowerSaver);
        assert_eq!(POWER_SAVER.llm_idle_unload, Duration::from_secs(10));
        assert_eq!(POWER_SAVER.stt_idle_unload, Duration::from_secs(60));
        assert_eq!(POWER_SAVER.batch_size, 1);
        assert_eq!(POWER_SAVER.learning_pace, LearningPace::Slow);
        assert_eq!(POWER_SAVER.unload_below_battery_pct, 50);

        assert_eq!(BALANCED.name, ProfileName::Balanced);
        assert_eq!(BALANCED.llm_idle_unload, Duration::from_secs(120));
        assert_eq!(BALANCED.stt_idle_unload, Duration::from_secs(300));
        assert_eq!(BALANCED.batch_size, 4);
        assert_eq!(BALANCED.learning_pace, LearningPace::Normal);
        assert_eq!(BALANCED.unload_below_battery_pct, 20);

        assert_eq!(PERFORMANCE.name, ProfileName::Performance);
        assert_eq!(PERFORMANCE.llm_idle_unload, Duration::from_secs(600));
        assert_eq!(PERFORMANCE.stt_idle_unload, Duration::from_secs(900));
        assert_eq!(PERFORMANCE.batch_size, 8);
        assert_eq!(PERFORMANCE.learning_pace, LearningPace::Aggressive);
        assert_eq!(PERFORMANCE.unload_below_battery_pct, 0);
    }

    #[test]
    fn policy_for_returns_matching_policy() {
        assert_eq!(
            policy_for(ProfileName::PowerSaver).name,
            ProfileName::PowerSaver
        );
        assert_eq!(
            policy_for(ProfileName::Balanced).name,
            ProfileName::Balanced
        );
        assert_eq!(
            policy_for(ProfileName::Performance).name,
            ProfileName::Performance
        );
    }

    use resource_monitor::ForegroundApp;
    use std::time::SystemTime;

    /// A `Signals` with the conservative "nothing wrong" baseline:
    /// on AC, nominal thermal, normal memory, idle CPU, no foreground.
    fn baseline() -> Signals {
        Signals {
            ts: SystemTime::UNIX_EPOCH,
            battery_pct: None,
            on_ac: true,
            power_saver_user_pref: false,
            thermal: ThermalState::Nominal,
            mem_pressure: MemoryPressure::Normal,
            mem_free_pct: 100,
            mem_free_mb: 16_000,
            cpu_used_pct: 0,
            foreground: None,
        }
    }

    #[test]
    fn score_baseline_on_ac_is_negative_20() {
        assert_eq!(score(&baseline()), -20);
    }

    #[test]
    fn score_battery_below_20() {
        let mut s = baseline();
        s.on_ac = false;
        s.battery_pct = Some(15);
        assert_eq!(score(&s), 40);
    }

    #[test]
    fn score_battery_smooth_range() {
        let mut s = baseline();
        s.on_ac = false;
        s.battery_pct = Some(30);
        assert_eq!(score(&s), 12);
    }

    #[test]
    fn score_battery_above_50_is_zero() {
        let mut s = baseline();
        s.on_ac = false;
        s.battery_pct = Some(80);
        assert_eq!(score(&s), 0);
    }

    #[test]
    fn score_thermal_levels() {
        let mut s = baseline();
        s.thermal = ThermalState::Fair;
        assert_eq!(score(&s), -20 + 15);
        s.thermal = ThermalState::Serious;
        assert_eq!(score(&s), -20 + 40);
        s.thermal = ThermalState::Critical;
        assert_eq!(score(&s), -20 + 80);
    }

    #[test]
    fn score_memory_pressure() {
        let mut s = baseline();
        s.mem_pressure = MemoryPressure::Warning;
        s.mem_free_pct = 25;
        assert_eq!(score(&s), -20 + 30);

        s.mem_pressure = MemoryPressure::Critical;
        s.mem_free_pct = 25;
        assert_eq!(score(&s), -20 + 50);

        s.mem_pressure = MemoryPressure::Normal;
        s.mem_free_pct = 10;
        assert_eq!(score(&s), -20 + 50);
    }

    #[test]
    fn score_cpu_above_70() {
        let mut s = baseline();
        s.cpu_used_pct = 90;
        assert_eq!(score(&s), -20 + 10);
    }

    #[test]
    fn score_foreground_heavy_app() {
        let mut s = baseline();
        s.foreground = Some(ForegroundApp {
            bundle_id: "com.apple.dt.Xcode".into(),
            cpu_used_pct: 40,
            mem_resident_mb: 2000,
        });
        assert_eq!(score(&s), -20 + 25);
    }

    #[test]
    fn score_foreground_generic_high_cpu() {
        let mut s = baseline();
        s.foreground = Some(ForegroundApp {
            bundle_id: "com.apple.TextEdit".into(),
            cpu_used_pct: 60,
            mem_resident_mb: 200,
        });
        assert_eq!(score(&s), -20 + 15);
    }

    #[test]
    fn score_foreground_low_cpu_is_zero() {
        let mut s = baseline();
        s.foreground = Some(ForegroundApp {
            bundle_id: "com.apple.TextEdit".into(),
            cpu_used_pct: 5,
            mem_resident_mb: 200,
        });
        assert_eq!(score(&s), -20);
    }

    #[test]
    fn band_boundaries() {
        assert_eq!(band_for_score(0), ProfileName::Performance);
        assert_eq!(band_for_score(24), ProfileName::Performance);
        assert_eq!(band_for_score(25), ProfileName::Balanced);
        assert_eq!(band_for_score(64), ProfileName::Balanced);
        assert_eq!(band_for_score(65), ProfileName::PowerSaver);
        assert_eq!(band_for_score(130), ProfileName::PowerSaver);
        assert_eq!(band_for_score(-20), ProfileName::Performance);
    }

    #[test]
    fn emergency_low_power_mode() {
        let mut s = baseline();
        s.power_saver_user_pref = true;
        assert_eq!(emergency_target(&s), Some(EmergencyReason::LowPowerMode));
    }

    #[test]
    fn emergency_thermal_critical() {
        let mut s = baseline();
        s.thermal = ThermalState::Critical;
        assert_eq!(emergency_target(&s), Some(EmergencyReason::ThermalCritical));
    }

    #[test]
    fn emergency_memory_critical() {
        let mut s = baseline();
        s.mem_pressure = MemoryPressure::Critical;
        assert_eq!(emergency_target(&s), Some(EmergencyReason::MemoryCritical));
    }

    #[test]
    fn emergency_battery_critical() {
        let mut s = baseline();
        s.on_ac = false;
        s.battery_pct = Some(3);
        assert_eq!(emergency_target(&s), Some(EmergencyReason::BatteryCritical));
    }

    #[test]
    fn emergency_battery_none_never_triggers() {
        let mut s = baseline();
        s.on_ac = false;
        s.battery_pct = None;
        assert_eq!(emergency_target(&s), None);
    }

    #[test]
    fn emergency_none_on_baseline() {
        assert_eq!(emergency_target(&baseline()), None);
    }

    /// A `Signals` whose score lands squarely in the `PowerSaver` band
    /// (score >= 65) without tripping any emergency trigger: on battery at
    /// 30% (+12), serious thermal (+40), warning memory <30 (+30) = 82.
    fn high_pressure() -> Signals {
        let mut s = baseline();
        s.on_ac = false;
        s.battery_pct = Some(30);
        s.thermal = ThermalState::Serious;
        s.mem_pressure = MemoryPressure::Warning;
        s.mem_free_pct = 25;
        s
    }

    #[test]
    fn auto_starts_at_initial_and_needs_dwell_to_change() {
        let t0 = Instant::now();
        let mut d = Decider::new(ProfileMode::Auto, ProfileName::Balanced, t0);
        assert_eq!(d.decided(), ProfileName::Balanced);

        for i in 0..5 {
            let t = t0 + Duration::from_secs(i * 5);
            d.observe(&high_pressure(), t);
        }
        assert_eq!(d.decided(), ProfileName::Balanced);

        let t = t0 + Duration::from_secs(35);
        d.observe(&high_pressure(), t);
        assert_eq!(d.decided(), ProfileName::PowerSaver);
    }

    #[test]
    fn auto_emergency_bypasses_hysteresis() {
        let t0 = Instant::now();
        let mut d = Decider::new(ProfileMode::Auto, ProfileName::Performance, t0);

        let mut s = baseline();
        s.power_saver_user_pref = true;
        d.observe(&s, t0 + Duration::from_secs(1));
        assert_eq!(d.decided(), ProfileName::PowerSaver);
        assert_eq!(d.emergency(), Some(EmergencyReason::LowPowerMode));
    }

    #[test]
    fn pinned_holds_regardless_of_score() {
        let t0 = Instant::now();
        let mut d = Decider::new(
            ProfileMode::PinnedSoft(ProfileName::Performance),
            ProfileName::Performance,
            t0,
        );

        for i in 0..12 {
            let t = t0 + Duration::from_secs(i * 5);
            d.observe(&high_pressure(), t);
        }
        assert_eq!(d.decided(), ProfileName::Performance);
        assert_eq!(d.emergency(), None);
    }

    #[test]
    fn pinned_emergency_override_then_restore() {
        let t0 = Instant::now();
        let mut d = Decider::new(
            ProfileMode::PinnedSoft(ProfileName::Performance),
            ProfileName::Performance,
            t0,
        );

        let mut emerg = baseline();
        emerg.thermal = ThermalState::Critical;
        d.observe(&emerg, t0 + Duration::from_secs(5));
        assert_eq!(d.decided(), ProfileName::PowerSaver);
        assert_eq!(d.emergency(), Some(EmergencyReason::ThermalCritical));

        d.observe(&baseline(), t0 + Duration::from_secs(10));
        assert_eq!(d.decided(), ProfileName::Performance);
        assert_eq!(d.emergency(), None);
    }

    #[test]
    fn auto_post_emergency_does_not_instant_flip() {
        // Regression: an Auto Decider already in PowerSaver, hit by a long
        // emergency, must NOT instantly swing to Performance the moment the
        // emergency clears — it must re-earn the 30s dwell first.
        let t0 = Instant::now();
        let mut d = Decider::new(ProfileMode::Auto, ProfileName::PowerSaver, t0);

        // Sustained emergency (thermal Critical) for 120s.
        let mut emerg = baseline();
        emerg.thermal = ThermalState::Critical;
        for i in 1..=24 {
            d.observe(&emerg, t0 + Duration::from_secs(i * 5));
        }
        assert_eq!(d.decided(), ProfileName::PowerSaver);

        // Emergency clears with a benign (Performance-band) signal. Because
        // last_change was refreshed throughout the emergency, the dwell is
        // NOT yet satisfied → stays PowerSaver on the first clean sample.
        let clean = baseline(); // on AC, idle → score -20 → Performance band
        d.observe(&clean, t0 + Duration::from_secs(121));
        assert_eq!(d.decided(), ProfileName::PowerSaver);

        // After 30s of clean samples, the dwell elapses and history is full
        // of Performance-band scores → it finally climbs to Performance.
        for i in 25..=31 {
            d.observe(&clean, t0 + Duration::from_secs(121 + i));
        }
        d.observe(&clean, t0 + Duration::from_secs(160));
        assert_eq!(d.decided(), ProfileName::Performance);
    }

    #[test]
    fn auto_climbs_back_down_after_pressure_drops() {
        // Forward then reverse: pressure drives PowerSaver, then drops, and
        // after the dwell the Decider returns to Performance.
        let t0 = Instant::now();
        let mut d = Decider::new(ProfileMode::Auto, ProfileName::Balanced, t0);

        // Drive to PowerSaver with sustained high pressure past the dwell.
        for i in 1..=8 {
            d.observe(&high_pressure(), t0 + Duration::from_secs(i * 5));
        }
        assert_eq!(d.decided(), ProfileName::PowerSaver);

        // Pressure drops to benign for >30s → climbs back to Performance.
        let calm_base = t0 + Duration::from_secs(45);
        for i in 0..=8 {
            d.observe(&baseline(), calm_base + Duration::from_secs(i * 5));
        }
        assert_eq!(d.decided(), ProfileName::Performance);
    }

    #[test]
    fn set_mode_to_pinned_takes_effect_next_observe() {
        let t0 = Instant::now();
        let mut d = Decider::new(ProfileMode::Auto, ProfileName::Balanced, t0);
        d.set_mode(ProfileMode::PinnedSoft(ProfileName::PowerSaver));
        d.observe(&baseline(), t0 + Duration::from_secs(1));
        assert_eq!(d.decided(), ProfileName::PowerSaver);
        assert_eq!(d.mode(), ProfileMode::PinnedSoft(ProfileName::PowerSaver));
    }

    use tokio::sync::broadcast;

    #[tokio::test(flavor = "multi_thread")]
    async fn selector_reacts_to_emergency_signal() {
        let (tx, rx) = broadcast::channel(16);
        let selector = ProfileSelector::new(rx, ProfileMode::Auto, ProfileName::Balanced);

        assert_eq!(selector.current_profile(), ProfileName::Balanced);
        assert_eq!(selector.current_mode(), ProfileMode::Auto);

        let mut emerg = baseline();
        emerg.power_saver_user_pref = true;
        tx.send(emerg).unwrap();

        for _ in 0..50 {
            if selector.current_profile() == ProfileName::PowerSaver {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(selector.current_profile(), ProfileName::PowerSaver);
        assert_eq!(selector.current_policy().name, ProfileName::PowerSaver);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn take_pending_apply_reports_change_once() {
        let (tx, rx) = broadcast::channel(16);
        let selector = ProfileSelector::new(rx, ProfileMode::Auto, ProfileName::Balanced);

        assert!(selector.take_pending_apply().is_none());

        let mut emerg = baseline();
        emerg.power_saver_user_pref = true;
        tx.send(emerg).unwrap();
        for _ in 0..50 {
            if selector.current_profile() == ProfileName::PowerSaver {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let first = selector.take_pending_apply();
        assert_eq!(first.map(|p| p.name), Some(ProfileName::PowerSaver));
        assert!(selector.take_pending_apply().is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn subscribe_changes_yields_updates() {
        let (tx, rx) = broadcast::channel(16);
        let selector = ProfileSelector::new(rx, ProfileMode::Auto, ProfileName::Balanced);
        let mut changes = selector.subscribe_changes();

        let mut emerg = baseline();
        emerg.power_saver_user_pref = true;
        tx.send(emerg).unwrap();

        tokio::time::timeout(Duration::from_secs(2), changes.changed())
            .await
            .expect("a change within 2s")
            .expect("sender alive");
        assert_eq!(*changes.borrow_and_update(), ProfileName::PowerSaver);
    }
}
