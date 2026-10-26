//! Converts independent energy observations to the public answer without hiding other sources.
use crate::direct::{Reading, ioreport};
use crate::tools::pmset;
use hostvitals_policy::findings;
use serde_json::{Value, json};

pub(crate) fn power(reading: Reading<ioreport::Power>, skipped: &mut Vec<Value>) -> Value {
    let empty = json!({"window_seconds":null,"cpu_watts":null,"gpu_watts":null,"ane_watts":null,"gpu_active_share":null});
    match reading {
        Reading::Unavailable { reason } => {
            skipped.push(json!({"measurement":"power","count":1,"reason":reason,"fix":"brew upgrade sunfmin/tap/hostvitals && hostvitals doctor && hostvitals now"}));
            empty
        }
        Reading::Available { data } => {
            if !data.window_seconds.is_finite() || data.window_seconds <= 0.0 {
                return power(
                    Reading::Unavailable {
                        reason: "IOReport returned an invalid window".into(),
                    },
                    skipped,
                );
            }
            let mut answer = empty;
            answer["window_seconds"] = json!(data.window_seconds);
            for key in ["cpu_watts", "gpu_watts", "ane_watts"] {
                let channels: Vec<_> = data
                    .energy
                    .iter()
                    .filter(|c| match key {
                        "cpu_watts" => c.name.ends_with("CPU Energy"),
                        "gpu_watts" => c.name == "GPU Energy",
                        _ => c.name.starts_with("ANE"),
                    })
                    .collect();
                let total = if channels.is_empty() {
                    None
                } else {
                    channels.iter().try_fold(0.0, |sum, c| {
                        hostvitals_policy::energy::counter_watts(
                            c.value,
                            c.counter_before,
                            c.counter_after,
                            &c.unit,
                            data.window_seconds,
                        )
                        .and_then(|v| hostvitals_policy::add_rates(Some(sum), Some(v)))
                    })
                };
                answer[key] = json!(total);
                if total.is_none() {
                    power_skip(skipped, key);
                }
            }
            let active = hostvitals_policy::energy::gpu_active_share(&data.gpu_states);
            answer["gpu_active_share"] = json!(active);
            if active.is_none() {
                power_skip(skipped, "gpu_active_share");
            }
            answer
        }
    }
}
fn power_skip(skipped: &mut Vec<Value>, key: &str) {
    skipped.push(json!({"measurement":format!("power.{key}"),"count":1,"reason":"IOReport channel absent or invalid: unknown unit, regressed/inconsistent counters, no accumulated energy in either sample (idle versus restricted is unknown), or invalid GPU residency","fix":"brew upgrade sunfmin/tap/hostvitals && hostvitals doctor && hostvitals now"}));
}

pub(crate) fn battery(skipped: &mut Vec<Value>) -> Value {
    match pmset::battery() {
        Ok(b) => {
            json!({"present":b.present,"charge_percent":b.charge_percent,"charging":b.charging,"state":b.state,"power_source":b.power_source,"time_remaining_minutes":b.time_remaining_minutes})
        }
        Err(reason) => {
            skip(skipped, "battery", reason);
            Value::Null
        }
    }
}
pub(crate) fn thermal(skipped: &mut Vec<Value>, judgements: &mut Vec<Value>) -> Value {
    match pmset::thermal() {
        Ok(t) => {
            let throttled = t
                .cpu_speed_limit_percent
                .is_some_and(findings::cpu_throttled);
            if throttled {
                judgements.push(json!({"rule":"cpu_throttled","subject":{"kind":"host"},
                    "measured":{"cpu_speed_limit_percent":t.cpu_speed_limit_percent},
                    "threshold":{"comparison":"<","cpu_speed_limit_percent":findings::CPU_SPEED_LIMIT_PERCENT},
                    "next_command":"hostvitals now --top 20"}));
            }
            json!({"cpu_speed_limit_percent":t.cpu_speed_limit_percent,"cpu_scheduler_limit_percent":t.cpu_scheduler_limit_percent,"available_cpus":t.available_cpus,"cpu_throttled":throttled})
        }
        Err(reason) => {
            skip(skipped, "thermal", reason);
            Value::Null
        }
    }
}
fn skip(skipped: &mut Vec<Value>, measurement: &str, reason: String) {
    skipped.push(json!({"measurement":measurement,"count":1,"reason":reason,"fix":"PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH hostvitals doctor && hostvitals now"}));
}
