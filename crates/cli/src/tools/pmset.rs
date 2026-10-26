//! Reads power-source status; no power-management setting is changed.
pub(super) fn args(_pid: &str) -> Vec<&str> {
    vec!["-g", "batt"]
}

pub(crate) fn battery() -> Result<hostvitals_policy::energy::Battery, String> {
    super::capture(super::Tool::Pmset, vec!["-g", "batt"])
        .and_then(|text| hostvitals_policy::energy::parse_battery(&text).map_err(str::to_owned))
        .map_err(|reason| format!("pmset -g batt: {reason}"))
}
pub(crate) fn thermal() -> Result<hostvitals_policy::energy::Thermal, String> {
    super::capture(super::Tool::Pmset, vec!["-g", "therm"])
        .and_then(|text| hostvitals_policy::energy::parse_thermal(&text).map_err(str::to_owned))
        .map_err(|reason| format!("pmset -g therm: {reason}"))
}
