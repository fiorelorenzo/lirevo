//! Cross-platform `Signals` snapshot, built once per monitor tick.

use std::time::SystemTime;

use serde::Serialize;

/// One snapshot of system resource state.
///
/// Field semantics are described inline. Cross-platform consumers should
/// not depend on which sensor populated each field — only on the value.
#[derive(Debug, Clone, Serialize)]
pub struct Signals {
    /// When the snapshot was assembled.
    pub ts: SystemTime,

    // Power
    /// `Some(0..=100)` on devices with a battery; `None` on AC-only desktops.
    pub battery_pct: Option<u8>,
    /// True if the machine is on AC power. On desktops without battery,
    /// always true.
    pub on_ac: bool,
    /// User has explicitly asked for power-saving mode. On macOS this
    /// mirrors `NSProcessInfo.isLowPowerModeEnabled`. On Linux this maps
    /// to `power-profiles-daemon` `ActiveProfile` == "power-saver"
    /// (see spec §12). On Windows it mirrors the system power slider.
    pub power_saver_user_pref: bool,

    // Thermal
    pub thermal: ThermalState,

    // Memory
    pub mem_pressure: MemoryPressure,
    /// Free memory as a percentage of physical memory (0..=100).
    pub mem_free_pct: u8,
    /// Free memory in MB. Useful for absolute "below 2 GB" budget checks.
    pub mem_free_mb: u32,

    // CPU
    /// System-wide CPU utilisation over the last 5-second window.
    pub cpu_used_pct: u8,

    // Foreground
    pub foreground: Option<ForegroundApp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ThermalState {
    Nominal,
    Fair,
    Serious,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MemoryPressure {
    Normal,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForegroundApp {
    pub bundle_id: String,
    pub cpu_used_pct: u8,
    pub mem_resident_mb: u32,
}

/// Builder used by the monitor task to assemble a `Signals` from the
/// current sensor state. All fields default to conservative "nothing
/// wrong" values so unmapped sensors don't unfairly bias the
/// `ProfileSelector` scoring.
#[derive(Debug, Clone)]
pub struct SignalsBuilder {
    inner: Signals,
}

impl SignalsBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Signals {
                ts: SystemTime::now(),
                battery_pct: None,
                on_ac: true,
                power_saver_user_pref: false,
                thermal: ThermalState::Nominal,
                mem_pressure: MemoryPressure::Normal,
                mem_free_pct: 100,
                mem_free_mb: u32::MAX,
                cpu_used_pct: 0,
                foreground: None,
            },
        }
    }

    pub fn battery_pct(&mut self, v: Option<u8>) -> &mut Self {
        self.inner.battery_pct = v;
        self
    }
    pub fn on_ac(&mut self, v: bool) -> &mut Self {
        self.inner.on_ac = v;
        self
    }
    pub fn power_saver_user_pref(&mut self, v: bool) -> &mut Self {
        self.inner.power_saver_user_pref = v;
        self
    }
    pub fn thermal(&mut self, v: ThermalState) -> &mut Self {
        self.inner.thermal = v;
        self
    }
    pub fn mem_pressure(&mut self, v: MemoryPressure) -> &mut Self {
        self.inner.mem_pressure = v;
        self
    }
    pub fn mem_free_pct(&mut self, v: u8) -> &mut Self {
        self.inner.mem_free_pct = v;
        self
    }
    pub fn mem_free_mb(&mut self, v: u32) -> &mut Self {
        self.inner.mem_free_mb = v;
        self
    }
    pub fn cpu_used_pct(&mut self, v: u8) -> &mut Self {
        self.inner.cpu_used_pct = v;
        self
    }
    pub fn foreground(&mut self, v: Option<ForegroundApp>) -> &mut Self {
        self.inner.foreground = v;
        self
    }

    #[must_use]
    pub fn build(self) -> Signals {
        Signals {
            ts: SystemTime::now(),
            ..self.inner
        }
    }
}

impl Default for SignalsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_builder_produces_defaults() {
        let s = SignalsBuilder::new().build();
        assert!(matches!(s.thermal, ThermalState::Nominal));
        assert!(matches!(s.mem_pressure, MemoryPressure::Normal));
        assert_eq!(s.battery_pct, None);
        assert!(s.on_ac);
        assert!(!s.power_saver_user_pref);
        assert_eq!(s.mem_free_pct, 100);
        assert_eq!(s.cpu_used_pct, 0);
        assert!(s.foreground.is_none());
    }

    #[test]
    fn signals_is_serializable() {
        let s = SignalsBuilder::new().build();
        let _ = serde_json::to_string(&s).expect("serde");
    }
}
