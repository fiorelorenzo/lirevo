use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Mutex;

use crate::{ForegroundApp, MemoryPressure, ThermalState};

/// Sensor-writable shared state. Sensors write here from any thread or
/// Objective-C callback; the monitor task reads it once per tick to
/// assemble a [`Signals`](crate::Signals).
///
/// Discrete enums (`ThermalState`, `MemoryPressure`) and the `Option<u8>`
/// battery are encoded into atomics via small numeric maps; the
/// `ForegroundApp` struct is owned by a `Mutex` because it carries an
/// owned `String`.
#[derive(Debug, Default)]
pub struct SharedState {
    // 0xFF = None, 0..=100 = Some(value)
    battery_pct: AtomicU8,
    battery_pct_present: AtomicBool,
    on_ac: AtomicBool,
    power_saver_user_pref: AtomicBool,
    thermal: AtomicU8,      // 0=Nominal 1=Fair 2=Serious 3=Critical
    mem_pressure: AtomicU8, // 0=Normal 1=Warning 2=Critical
    mem_free_pct: AtomicU8,
    mem_free_mb: AtomicU32,
    cpu_used_pct: AtomicU8,
    foreground: Mutex<Option<ForegroundApp>>,
}

// Setters are exercised by `#[cfg(test)]` round-trip tests and by sensors
// landing in T7+; they are not called from non-test crate code yet, so
// suppress dead-code on the setter half of the API for now.
#[allow(dead_code)]
impl SharedState {
    // Battery
    pub fn battery_pct(&self) -> Option<u8> {
        if self.battery_pct_present.load(Ordering::Acquire) {
            Some(self.battery_pct.load(Ordering::Acquire))
        } else {
            None
        }
    }
    /// Single writer required (e.g. one polling task or one KVO observer).
    /// Concurrent writes from multiple sources may produce a stale value
    /// paired with `present=true` because the value + presence flag are
    /// two separate atomics. The current architecture has exactly one
    /// writer per sensor (`macos::power::poll_battery_once` for battery),
    /// so this is safe in practice.
    pub fn set_battery_pct(&self, v: Option<u8>) {
        match v {
            Some(p) => {
                self.battery_pct.store(p, Ordering::Release);
                self.battery_pct_present.store(true, Ordering::Release);
            }
            None => self.battery_pct_present.store(false, Ordering::Release),
        }
    }

    // On AC + power saver pref + simple bools
    pub fn on_ac(&self) -> bool {
        self.on_ac.load(Ordering::Acquire)
    }
    pub fn set_on_ac(&self, v: bool) {
        self.on_ac.store(v, Ordering::Release);
    }

    pub fn power_saver_user_pref(&self) -> bool {
        self.power_saver_user_pref.load(Ordering::Acquire)
    }
    pub fn set_power_saver_user_pref(&self, v: bool) {
        self.power_saver_user_pref.store(v, Ordering::Release);
    }

    // Thermal
    pub fn thermal(&self) -> ThermalState {
        match self.thermal.load(Ordering::Acquire) {
            0 => ThermalState::Nominal,
            1 => ThermalState::Fair,
            2 => ThermalState::Serious,
            _ => ThermalState::Critical,
        }
    }
    pub fn set_thermal(&self, v: ThermalState) {
        let n = match v {
            ThermalState::Nominal => 0,
            ThermalState::Fair => 1,
            ThermalState::Serious => 2,
            ThermalState::Critical => 3,
        };
        self.thermal.store(n, Ordering::Release);
    }

    // Memory pressure
    pub fn mem_pressure(&self) -> MemoryPressure {
        match self.mem_pressure.load(Ordering::Acquire) {
            0 => MemoryPressure::Normal,
            1 => MemoryPressure::Warning,
            _ => MemoryPressure::Critical,
        }
    }
    pub fn set_mem_pressure(&self, v: MemoryPressure) {
        let n = match v {
            MemoryPressure::Normal => 0,
            MemoryPressure::Warning => 1,
            MemoryPressure::Critical => 2,
        };
        self.mem_pressure.store(n, Ordering::Release);
    }

    // Free memory
    pub fn mem_free_pct(&self) -> u8 {
        self.mem_free_pct.load(Ordering::Acquire)
    }
    pub fn set_mem_free_pct(&self, v: u8) {
        self.mem_free_pct.store(v, Ordering::Release);
    }

    pub fn mem_free_mb(&self) -> u32 {
        self.mem_free_mb.load(Ordering::Acquire)
    }
    pub fn set_mem_free_mb(&self, v: u32) {
        self.mem_free_mb.store(v, Ordering::Release);
    }

    // CPU
    pub fn cpu_used_pct(&self) -> u8 {
        self.cpu_used_pct.load(Ordering::Acquire)
    }
    pub fn set_cpu_used_pct(&self, v: u8) {
        self.cpu_used_pct.store(v, Ordering::Release);
    }

    // Foreground (cloned out because it owns a String)
    pub fn foreground(&self) -> Option<ForegroundApp> {
        self.foreground.lock().expect("foreground mutex").clone()
    }
    pub fn set_foreground(&self, v: Option<ForegroundApp>) {
        *self.foreground.lock().expect("foreground mutex") = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn shared_state_round_trip_atomics() {
        let s = Arc::new(SharedState::default());

        s.set_battery_pct(Some(42));
        s.set_on_ac(false);
        s.set_power_saver_user_pref(true);
        s.set_thermal(ThermalState::Serious);
        s.set_mem_pressure(MemoryPressure::Warning);
        s.set_mem_free_pct(25);
        s.set_mem_free_mb(1024);
        s.set_cpu_used_pct(80);

        assert_eq!(s.battery_pct(), Some(42));
        s.set_battery_pct(None);
        assert_eq!(s.battery_pct(), None);
        assert!(!s.on_ac());
        assert!(s.power_saver_user_pref());
        assert_eq!(s.thermal(), ThermalState::Serious);
        assert_eq!(s.mem_pressure(), MemoryPressure::Warning);
        assert_eq!(s.mem_free_pct(), 25);
        assert_eq!(s.mem_free_mb(), 1024);
        assert_eq!(s.cpu_used_pct(), 80);
    }

    #[test]
    fn shared_state_round_trip_foreground() {
        let s = SharedState::default();
        assert!(s.foreground().is_none());

        s.set_foreground(Some(ForegroundApp {
            bundle_id: "com.example".into(),
            cpu_used_pct: 50,
            mem_resident_mb: 512,
        }));

        let got = s.foreground().expect("set above");
        assert_eq!(got.bundle_id, "com.example");
        assert_eq!(got.cpu_used_pct, 50);
        assert_eq!(got.mem_resident_mb, 512);
    }
}
