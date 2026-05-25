//! JSON sidecar — currently just (de)serializes `ReportData`. Used by
//! `lirevo-eval judge` to re-score an existing report.

use std::path::Path;

use crate::report::ReportData;

pub fn load(path: &Path) -> anyhow::Result<ReportData> {
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}
