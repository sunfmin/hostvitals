use hostvitals_policy::findings::{Finding, Host, Pressure, judge};

fn host(pressure: Pressure) -> Host {
    Host {
        pressure,
        memory_bytes: 1_000,
        swap_used_bytes: 0,
        swap_total_bytes: 1_000,
    }
}

#[test]
fn pressure_warns_at_warn_and_above_but_not_below() {
    for (pressure, expected) in [
        (Pressure::Normal, false),
        (Pressure::Warn, true),
        (Pressure::Critical, true),
    ] {
        let findings = judge(&host(pressure), &[], 1_000_000_000);
        if expected {
            assert!(
                matches!(findings.as_slice(), [Finding::MemoryPressure { pressure: actual }] if *actual == pressure)
            );
        } else {
            assert!(findings.is_empty());
        }
    }
}

#[test]
fn swap_must_be_strictly_past_eighty_percent() {
    for (used, expected) in [(799, false), (800, false), (801, true)] {
        let mut h = host(Pressure::Normal);
        h.swap_used_bytes = used;
        let findings = judge(&h, &[], 1_000_000_000);
        assert_eq!(findings.len(), usize::from(expected), "used={used}");
    }
}

#[test]
fn resident_memory_must_be_strictly_past_ten_percent_of_the_host() {
    use hostvitals_policy::findings::Process;
    for (resident_bytes, expected) in [(99, false), (100, false), (101, true)] {
        let process = Process {
            pid: 42,
            resident_bytes,
            cpu_hundredths: None,
        };
        let findings = judge(&host(Pressure::Normal), &[process], 1_000_000_000);
        assert_eq!(
            findings.len(),
            usize::from(expected),
            "resident={resident_bytes}"
        );
    }
}

#[test]
fn cpu_requires_at_least_one_core_across_the_actual_window() {
    use hostvitals_policy::findings::Process;
    for (hundredths, nanoseconds, expected) in [
        (Some(99), 1_000_000_000, false),
        (Some(100), 1_000_000_000, true),
        (Some(101), 1_000_000_000, true),
        (Some(100), 1_000_000_001, false),
        (Some(300), 3_000_000_000, true),
        (Some(299), 3_000_000_000, false),
        (None, 1_000_000_000, false),
        (Some(100), 0, false),
    ] {
        let process = Process {
            pid: 42,
            resident_bytes: 1,
            cpu_hundredths: hundredths,
        };
        let findings = judge(&host(Pressure::Normal), &[process], nanoseconds);
        assert_eq!(
            findings.len(),
            usize::from(expected),
            "cpu={hundredths:?}, window={nanoseconds}"
        );
    }
}
