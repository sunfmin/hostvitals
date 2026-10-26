use hostvitals_policy::{energy, findings};

#[test]
fn top_rejects_malformed_or_partial_final_samples() {
    for report in [
        "",
        "PID POWER\n10 0.0\n",
        "PID POWER\n10 0.0\nPID POWER\nbroken\n10 1.0\n",
        "PID POWER\n10 0.0\nPID POWER\n10 NaN\n",
        "PID POWER\n10 0.0\nPID POWER\n10 -1\n",
        "PID POWER\n10 0.0\nPID POWER\n10 1\n10 2\n",
    ] {
        assert!(energy::parse_top(report).is_err(), "{report}");
    }
}

#[test]
fn thermal_limit_has_an_exact_full_speed_threshold() {
    for (limit, expected) in [(0, true), (99, true), (100, false), (101, false)] {
        assert_eq!(findings::cpu_throttled(limit), expected);
    }
    let t = energy::parse_thermal("CPU_Speed_Limit = 100\nCPU_Scheduler_Limit = 100\n").unwrap();
    assert!(!findings::cpu_throttled(t.cpu_speed_limit_percent.unwrap()));
    assert!(energy::parse_thermal("CPU_Speed_Limit = 101\n").is_err());
    assert!(energy::parse_thermal("CPU_Speed_Limit = bad\n").is_err());
    assert!(energy::parse_thermal("unrecognized status").is_err());
}

#[test]
fn component_units_missing_channels_and_counter_resets_never_invent_power() {
    assert_eq!(energy::watts(2000, "mJ", 2.0), Some(1.0));
    assert_eq!(energy::watts(2000000, "uJ", 2.0), Some(1.0));
    assert_eq!(energy::watts(2000000000, "nJ", 2.0), Some(1.0));
    assert_eq!(energy::watts(-1, "mJ", 2.0), None);
    assert_eq!(energy::watts(1, "unknown", 2.0), None);
    assert_eq!(energy::watts(1, "mJ", 0.0), None);
    assert_eq!(energy::watts(1, "mJ", f64::NAN), None);
    assert_eq!(energy::counter_watts(0, Some(0), Some(0), "mJ", 1.0), None);
    assert_eq!(
        energy::counter_watts(0, Some(10), Some(10), "mJ", 1.0),
        Some(0.0)
    );
    assert_eq!(
        energy::counter_watts(5, Some(10), Some(20), "mJ", 1.0),
        None
    );
    assert_eq!(energy::gpu_active_share(&[]), None);
    assert_eq!(energy::gpu_active_share(&[("OFF".into(), 0)]), None);
    assert_eq!(energy::gpu_active_share(&[("OFF".into(), -1)]), None);
    assert_eq!(
        energy::gpu_active_share(&[
            ("OFF".into(), 60),
            ("DOWN".into(), 10),
            ("IDLE".into(), 10),
            ("P1".into(), 20)
        ]),
        Some(0.2)
    );
}

#[test]
fn malformed_battery_and_gpu_reports_are_not_observed_empty_readings() {
    for report in [
        "",
        "Now drawing from 'AC Power'\nbroken",
        "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1) 101%; discharging; 1:00 remaining present: true",
        "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1) 50%; discharging; 1:99 remaining present: true",
    ] {
        assert!(energy::parse_battery(report).is_err(), "{report}");
    }
    for report in [
        "",
        "Name ID CPU ms/s User% GPU ms/s\nApp 1 0 0 5\n",
        "*** Sampled system activity (0.0ms elapsed) ***\nName ID GPU ms/s\nApp 1 5\n",
        "*** Sampled system activity (1000.0ms elapsed) ***\nName ID GPU ms/s\nApp 1 NaN\n",
        "*** Sampled system activity (1000.0ms elapsed) ***\nName ID GPU ms/s\nApp 1 1\nApp 1 1\n",
    ] {
        assert!(energy::parse_gpu_times(report).is_err(), "{report}");
    }
    let report = "*** Sampled system activity (2000.0ms elapsed) ***\nName ID CPU ms/s User% Deadlines (<2 ms, 2-5 ms) Wakeups (Intr, Pkg idle) GPU ms/s\nWorker with 2 spaces 15 1.0 20.0 0.0 (0.0, 0.0) 0.0 (0.0, 0.0) 25.0\n\n";
    let times = energy::parse_gpu_times(report).unwrap();
    assert_eq!(times.window_seconds, 2.0);
    assert_eq!(times.milliseconds.get(&15), Some(&50.0));
}

#[test]
fn battery_not_charging_and_low_battery_warnings_preserve_the_status() {
    let b=energy::parse_battery("Now drawing from 'AC Power'\n -InternalBattery-0 (id=1) 80%; AC attached; not charging present: true\n").unwrap();
    assert_eq!(b.charging, Some(false));
    assert_eq!(b.state.as_deref(), Some("AC attached; not charging"));
    assert_eq!(b.time_remaining_minutes, None);
    let b=energy::parse_battery("Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1) 5%; discharging; 0:12 remaining present: true\n\tBattery Warning: Final\n").unwrap();
    assert_eq!(b.charge_percent, Some(5));
    assert_eq!(b.time_remaining_minutes, Some(12));
    let b=energy::parse_battery("Now drawing from 'AC Power'\n -InternalBattery-0 (id=1) 99%; finishing charge; (no estimate) present: true\n").unwrap();
    assert_eq!(b.charging, Some(true));
}
