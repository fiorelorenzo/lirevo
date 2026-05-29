//! Pure lifecycle decision logic. Given slot snapshots, signals, policy,
//! and timing, returns the actions the Engine shell should apply. No clock,
//! no I/O — `now` and all timestamps are injected so this is
//! deterministically testable without real models.

#![allow(dead_code)] // consumed by the Engine shell in Phase C

use std::time::{Duration, Instant};

use inference_core::profile::{NThreads, ProfilePolicy};
use resource_monitor::{MemoryPressure, Signals};

use crate::engine::slot::SlotSnapshot;

/// Free RAM below this (MB) forces an LLM unload regardless of profile.
const LOW_FREE_RAM_MB: u32 = 2048;
/// A loaded-but-idle model is only unloaded for foreground-heavy pressure
/// if it has been idle at least this long (avoid yanking mid-use).
const FOREGROUND_IDLE_GRACE: Duration = Duration::from_secs(5);

/// Why a model was unloaded. Surfaced in tracing + the `*_state_changed`
/// Tauri events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnloadReason {
    IdleTimeout,
    MemPressureCritical,
    LowFreeRam,
    ForegroundHeavy,
    BatteryBelowThreshold,
}

/// An action for the Engine shell to apply. Pure data — the shell does the
/// actual load/unload/reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    UnloadLlm(UnloadReason),
    UnloadStt(UnloadReason),
    /// Reload the LLM with a new context thread count (profile changed).
    ReloadLlmForThreads { n_threads: i32 },
    /// Proactively load the LLM (startup pre-load heuristic).
    PreloadLlm,
}

/// Resolve a profile thread hint to a concrete core count. Pure: uses
/// `num_cpus` topology but no clock/I/O.
#[must_use]
pub fn resolve_n_threads(n: NThreads) -> i32 {
    let total = i32::try_from(num_cpus::get()).unwrap_or(1).max(1);
    let physical = i32::try_from(num_cpus::get_physical()).unwrap_or(total).max(1);
    match n {
        // Efficiency-only: a small slice. Heuristic: half the physical
        // cores, min 2, on Apple Silicon this lands near the E-core count.
        NThreads::EcoOnly => (physical / 2).max(2),
        NThreads::Mixed => physical,
        NThreads::AllPCores => total,
    }
}

/// Decide lifecycle actions. Pure. `now` and timestamps injected.
///
/// `foreground_heavy_streak` is how long the foreground app has been
/// sustained-heavy (see `streak`). `last_dictation` is when the last
/// dictation finished (reserved for future preload refinement).
#[must_use]
pub fn lifecycle_decision(
    llm: SlotSnapshot,
    stt: SlotSnapshot,
    signals: &Signals,
    policy: &ProfilePolicy,
    now: Instant,
    _last_dictation: Instant,
    foreground_heavy_streak: Duration,
) -> Vec<Action> {
    let mut actions = Vec::new();

    // 1. Hard pressure: memory critical → unload everything that is loaded.
    if signals.mem_pressure == MemoryPressure::Critical {
        if is_loaded(llm) {
            actions.push(Action::UnloadLlm(UnloadReason::MemPressureCritical));
        }
        if is_loaded(stt) {
            actions.push(Action::UnloadStt(UnloadReason::MemPressureCritical));
        }
        return actions; // nothing else matters under critical pressure
    }

    // 2. Low free RAM → unload the LLM (the heavier model).
    if signals.mem_free_mb < LOW_FREE_RAM_MB && is_loaded(llm) {
        actions.push(Action::UnloadLlm(UnloadReason::LowFreeRam));
        return actions;
    }

    // 3. Foreground heavy app sustained > 30s → unload the LLM if it has
    // been idle past the grace window.
    if foreground_heavy_streak > Duration::from_secs(30) {
        if let SlotSnapshot::Loaded { last_use, .. } = llm {
            if now.duration_since(last_use) > FOREGROUND_IDLE_GRACE {
                actions.push(Action::UnloadLlm(UnloadReason::ForegroundHeavy));
                return actions;
            }
        }
    }

    // 4. Battery below the profile threshold (on battery) → unload the LLM.
    if !signals.on_ac
        && matches!(signals.battery_pct, Some(b) if b < policy.unload_below_battery_pct)
        && is_loaded(llm)
    {
        actions.push(Action::UnloadLlm(UnloadReason::BatteryBelowThreshold));
        return actions;
    }

    // 5. Idle timeout per policy.
    if let SlotSnapshot::Loaded { last_use, .. } = llm {
        if now.duration_since(last_use) > policy.llm_idle_unload {
            actions.push(Action::UnloadLlm(UnloadReason::IdleTimeout));
        }
    }
    if let SlotSnapshot::Loaded { last_use, .. } = stt {
        if now.duration_since(last_use) > policy.stt_idle_unload {
            actions.push(Action::UnloadStt(UnloadReason::IdleTimeout));
        }
    }

    // 6. Reload the LLM if the profile's thread count changed (only when
    // loaded, idle, and not already being unloaded above).
    if actions.is_empty() {
        if let SlotSnapshot::Loaded { loaded_n_threads: Some(loaded), .. } = llm {
            let desired = resolve_n_threads(policy.n_threads);
            if desired != loaded {
                actions.push(Action::ReloadLlmForThreads { n_threads: desired });
            }
        }
    }

    // 7. Startup / opportunistic pre-load: LLM unloaded, profile is
    // Balanced|Performance, on AC. PowerSaver and on-battery start cold.
    if actions.is_empty()
        && matches!(llm, SlotSnapshot::Unloaded)
        && signals.on_ac
        && preload_profile(policy)
    {
        actions.push(Action::PreloadLlm);
    }

    actions
}

fn is_loaded(s: SlotSnapshot) -> bool {
    matches!(s, SlotSnapshot::Loaded { .. })
}

fn preload_profile(policy: &ProfilePolicy) -> bool {
    use inference_core::profile::ProfileName;
    matches!(policy.name, ProfileName::Balanced | ProfileName::Performance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use inference_core::profile::{BALANCED, PERFORMANCE, POWER_SAVER};
    use resource_monitor::ThermalState;
    use std::time::SystemTime;

    fn signals_ok() -> Signals {
        Signals {
            ts: SystemTime::UNIX_EPOCH,
            battery_pct: None,
            on_ac: true,
            power_saver_user_pref: false,
            thermal: ThermalState::Nominal,
            mem_pressure: MemoryPressure::Normal,
            mem_free_pct: 80,
            mem_free_mb: 16_000,
            cpu_used_pct: 5,
            foreground: None,
        }
    }

    fn n_threads_count(n: NThreads) -> i32 {
        resolve_n_threads(n)
    }

    #[test]
    fn idle_unload_after_policy_timeout() {
        let t0 = Instant::now();
        // PowerSaver llm_idle_unload = 10s. Loaded, last_use 15s ago.
        let llm = SlotSnapshot::Loaded {
            last_use: t0,
            loaded_n_threads: Some(n_threads_count(POWER_SAVER.n_threads)),
        };
        let actions = lifecycle_decision(
            llm,
            SlotSnapshot::Unloaded,
            &signals_ok(),
            &POWER_SAVER,
            t0 + Duration::from_secs(15),
            t0, // last_dictation long ago
            Duration::ZERO,
        );
        assert!(actions.contains(&Action::UnloadLlm(UnloadReason::IdleTimeout)));
    }

    #[test]
    fn no_idle_unload_before_timeout() {
        let t0 = Instant::now();
        let llm = SlotSnapshot::Loaded {
            last_use: t0,
            loaded_n_threads: Some(n_threads_count(BALANCED.n_threads)),
        };
        // BALANCED llm_idle_unload = 120s; only 30s elapsed.
        let actions = lifecycle_decision(
            llm,
            SlotSnapshot::Unloaded,
            &signals_ok(),
            &BALANCED,
            t0 + Duration::from_secs(30),
            t0,
            Duration::ZERO,
        );
        assert!(!actions.iter().any(|a| matches!(a, Action::UnloadLlm(_))));
    }

    #[test]
    fn mem_pressure_critical_unloads_both() {
        let t0 = Instant::now();
        let mut sig = signals_ok();
        sig.mem_pressure = MemoryPressure::Critical;
        let llm = SlotSnapshot::Loaded { last_use: t0, loaded_n_threads: Some(8) };
        let stt = SlotSnapshot::Loaded { last_use: t0, loaded_n_threads: None };
        let actions = lifecycle_decision(llm, stt, &sig, &BALANCED, t0, t0, Duration::ZERO);
        assert!(actions.contains(&Action::UnloadLlm(UnloadReason::MemPressureCritical)));
        assert!(actions.contains(&Action::UnloadStt(UnloadReason::MemPressureCritical)));
    }

    #[test]
    fn low_free_ram_unloads_llm() {
        let t0 = Instant::now();
        let mut sig = signals_ok();
        sig.mem_free_mb = 1500; // < 2048
        let llm = SlotSnapshot::Loaded { last_use: t0, loaded_n_threads: Some(8) };
        let actions = lifecycle_decision(llm, SlotSnapshot::Unloaded, &sig, &BALANCED, t0, t0, Duration::ZERO);
        assert!(actions.contains(&Action::UnloadLlm(UnloadReason::LowFreeRam)));
    }

    #[test]
    fn foreground_heavy_unloads_idle_llm() {
        let t0 = Instant::now();
        // Heavy streak > 30s, llm idle > 5s grace.
        let llm = SlotSnapshot::Loaded { last_use: t0, loaded_n_threads: Some(8) };
        let actions = lifecycle_decision(
            llm,
            SlotSnapshot::Unloaded,
            &signals_ok(),
            &BALANCED,
            t0 + Duration::from_secs(10),
            t0,
            Duration::from_secs(35), // foreground heavy streak
        );
        assert!(actions.contains(&Action::UnloadLlm(UnloadReason::ForegroundHeavy)));
    }

    #[test]
    fn foreground_heavy_skips_recently_used_llm() {
        let t0 = Instant::now();
        let llm = SlotSnapshot::Loaded { last_use: t0 + Duration::from_secs(8), loaded_n_threads: Some(8) };
        // now only 2s after last_use → within 5s grace → no unload.
        let actions = lifecycle_decision(
            llm,
            SlotSnapshot::Unloaded,
            &signals_ok(),
            &BALANCED,
            t0 + Duration::from_secs(10),
            t0,
            Duration::from_secs(35),
        );
        assert!(!actions.iter().any(|a| matches!(a, Action::UnloadLlm(UnloadReason::ForegroundHeavy))));
    }

    #[test]
    fn battery_below_threshold_unloads_llm() {
        let t0 = Instant::now();
        let mut sig = signals_ok();
        sig.on_ac = false;
        sig.battery_pct = Some(15); // BALANCED unload_below_battery_pct = 20
        let llm = SlotSnapshot::Loaded { last_use: t0, loaded_n_threads: Some(8) };
        let actions = lifecycle_decision(llm, SlotSnapshot::Unloaded, &sig, &BALANCED, t0, t0, Duration::ZERO);
        assert!(actions.contains(&Action::UnloadLlm(UnloadReason::BatteryBelowThreshold)));
    }

    #[test]
    fn battery_on_ac_never_unloads_for_battery() {
        let t0 = Instant::now();
        let mut sig = signals_ok();
        sig.on_ac = true;
        sig.battery_pct = Some(5);
        let llm = SlotSnapshot::Loaded { last_use: t0, loaded_n_threads: Some(8) };
        let actions = lifecycle_decision(llm, SlotSnapshot::Unloaded, &sig, &BALANCED, t0, t0, Duration::ZERO);
        assert!(!actions.contains(&Action::UnloadLlm(UnloadReason::BatteryBelowThreshold)));
    }

    #[test]
    fn reload_llm_when_thread_count_changed() {
        let t0 = Instant::now();
        // Loaded with PowerSaver threads (EcoOnly), now policy is PERFORMANCE.
        let llm = SlotSnapshot::Loaded {
            last_use: t0,
            loaded_n_threads: Some(n_threads_count(POWER_SAVER.n_threads)),
        };
        // Idle (last dictation long ago, not under pressure).
        let actions = lifecycle_decision(
            llm,
            SlotSnapshot::Unloaded,
            &signals_ok(),
            &PERFORMANCE,
            t0 + Duration::from_secs(1),
            t0,
            Duration::ZERO,
        );
        // Only reload if the resolved counts actually differ.
        if n_threads_count(POWER_SAVER.n_threads) != n_threads_count(PERFORMANCE.n_threads) {
            assert!(actions.contains(&Action::ReloadLlmForThreads {
                n_threads: n_threads_count(PERFORMANCE.n_threads),
            }));
        }
    }

    #[test]
    fn preload_when_balanced_on_ac_and_unloaded() {
        let t0 = Instant::now();
        let actions = lifecycle_decision(
            SlotSnapshot::Unloaded,
            SlotSnapshot::Unloaded,
            &signals_ok(),
            &BALANCED,
            t0,
            t0, // recent dictation not required for startup preload
            Duration::ZERO,
        );
        assert!(actions.contains(&Action::PreloadLlm));
    }

    #[test]
    fn no_preload_on_powersaver() {
        let t0 = Instant::now();
        let actions = lifecycle_decision(
            SlotSnapshot::Unloaded,
            SlotSnapshot::Unloaded,
            &signals_ok(),
            &POWER_SAVER,
            t0,
            t0,
            Duration::ZERO,
        );
        assert!(!actions.contains(&Action::PreloadLlm));
    }

    #[test]
    fn no_preload_on_battery() {
        let t0 = Instant::now();
        let mut sig = signals_ok();
        sig.on_ac = false;
        sig.battery_pct = Some(90);
        let actions = lifecycle_decision(
            SlotSnapshot::Unloaded,
            SlotSnapshot::Unloaded,
            &sig,
            &PERFORMANCE,
            t0,
            t0,
            Duration::ZERO,
        );
        assert!(!actions.contains(&Action::PreloadLlm));
    }

    #[test]
    fn no_unload_while_loading() {
        let t0 = Instant::now();
        let mut sig = signals_ok();
        sig.mem_pressure = MemoryPressure::Critical;
        // Loading state: nothing to unload yet.
        let actions = lifecycle_decision(
            SlotSnapshot::Loading,
            SlotSnapshot::Loading,
            &sig,
            &BALANCED,
            t0,
            t0,
            Duration::ZERO,
        );
        assert!(actions.is_empty());
    }
}
