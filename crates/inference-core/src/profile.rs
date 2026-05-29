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

use std::time::Duration;

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
        assert_eq!(policy_for(ProfileName::PowerSaver).name, ProfileName::PowerSaver);
        assert_eq!(policy_for(ProfileName::Balanced).name, ProfileName::Balanced);
        assert_eq!(policy_for(ProfileName::Performance).name, ProfileName::Performance);
    }
}
