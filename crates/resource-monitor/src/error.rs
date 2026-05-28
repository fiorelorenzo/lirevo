use thiserror::Error;

#[derive(Debug, Error)]
pub enum MonitorError {
    /// A system-API call (`IOKit`, `vm_statistics64`, `host_processor_info`, ...)
    /// failed. The string contains the call name and any error code.
    #[error("system API failure: {0}")]
    SystemApi(String),

    /// The current target platform has no real sensor implementation.
    /// Returned by `ResourceMonitor::spawn()` on Linux/Windows in v0.6.
    #[error("resource monitor is not supported on this platform")]
    NotSupportedOnPlatform,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_display_clearly() {
        let e = MonitorError::SystemApi("IOPSCopyPowerSourcesInfo failed".into());
        assert!(format!("{e}").contains("IOPSCopyPowerSourcesInfo"));

        let e = MonitorError::NotSupportedOnPlatform;
        assert!(format!("{e}").contains("not supported"));
    }
}
