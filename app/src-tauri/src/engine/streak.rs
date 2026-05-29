//! Tracks how long the foreground app has been "heavy" (sustained high
//! CPU). Pure — `now` is injected. The Engine updates it per signal and
//! passes the streak to `lifecycle_decision`.

#![allow(dead_code)] // consumed by the Engine shell in Phase C

use std::time::{Duration, Instant};

use resource_monitor::Signals;

/// Foreground CPU above this is considered "heavy".
const HEAVY_CPU_PCT: u8 = 50;

pub struct ForegroundHeavyStreak {
    /// When the current heavy streak started, or `None` if not heavy now.
    started: Option<Instant>,
}

impl ForegroundHeavyStreak {
    #[must_use]
    pub fn new() -> Self {
        Self { started: None }
    }

    /// Feed a signal. Returns the current heavy-streak duration (`ZERO` if
    /// the foreground app is not currently heavy).
    pub fn observe(&mut self, signals: &Signals, now: Instant) -> Duration {
        let heavy = signals
            .foreground
            .as_ref()
            .is_some_and(|fg| fg.cpu_used_pct >= HEAVY_CPU_PCT);

        if heavy {
            let started = *self.started.get_or_insert(now);
            now.duration_since(started)
        } else {
            self.started = None;
            Duration::ZERO
        }
    }
}

impl Default for ForegroundHeavyStreak {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resource_monitor::{ForegroundApp, MemoryPressure, ThermalState};
    use std::time::SystemTime;

    fn sig_with_fg_cpu(cpu: u8) -> Signals {
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
            foreground: Some(ForegroundApp {
                bundle_id: "com.example".into(),
                cpu_used_pct: cpu,
                mem_resident_mb: 100,
            }),
        }
    }

    #[test]
    fn streak_zero_when_not_heavy() {
        let mut s = ForegroundHeavyStreak::new();
        let t0 = Instant::now();
        assert_eq!(s.observe(&sig_with_fg_cpu(10), t0), Duration::ZERO);
    }

    #[test]
    fn streak_accumulates_while_heavy() {
        let mut s = ForegroundHeavyStreak::new();
        let t0 = Instant::now();
        assert_eq!(s.observe(&sig_with_fg_cpu(80), t0), Duration::ZERO); // streak starts now
        assert_eq!(
            s.observe(&sig_with_fg_cpu(80), t0 + Duration::from_secs(40)),
            Duration::from_secs(40)
        );
    }

    #[test]
    fn streak_resets_when_heavy_ends() {
        let mut s = ForegroundHeavyStreak::new();
        let t0 = Instant::now();
        s.observe(&sig_with_fg_cpu(80), t0);
        s.observe(&sig_with_fg_cpu(80), t0 + Duration::from_secs(20));
        // Drops below heavy → reset.
        assert_eq!(
            s.observe(&sig_with_fg_cpu(10), t0 + Duration::from_secs(25)),
            Duration::ZERO
        );
        // Heavy again → fresh streak.
        assert_eq!(
            s.observe(&sig_with_fg_cpu(80), t0 + Duration::from_secs(30)),
            Duration::ZERO
        );
    }

    #[test]
    fn streak_zero_when_no_foreground() {
        let mut s = ForegroundHeavyStreak::new();
        let t0 = Instant::now();
        let mut sig = sig_with_fg_cpu(80);
        sig.foreground = None;
        assert_eq!(s.observe(&sig, t0), Duration::ZERO);
    }
}
