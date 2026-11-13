//! Reads macOS's posted thermal pressure level; nothing is posted or registered.
use hostvitals_policy::energy::{self, ThermalPressure};

pub(super) fn args() -> Vec<&'static str> {
    vec!["-g", energy::THERMAL_PRESSURE_KEY]
}

pub(crate) fn thermal_pressure() -> Result<ThermalPressure, String> {
    super::capture(super::Tool::Notifyutil, args())
        .and_then(|text| energy::parse_thermal_pressure(&text).map_err(str::to_owned))
        .map_err(|reason| format!("notifyutil -g {}: {reason}", energy::THERMAL_PRESSURE_KEY))
}
