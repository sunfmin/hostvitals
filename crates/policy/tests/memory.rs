use hostvitals_policy::memory::{parse_footprint, parse_regions, parse_summary};

#[test]
fn apple_reports_describe_footprint_region_kinds_zones_and_largest_regions() {
    let summary = parse_summary(include_str!("fixtures/vmmap-summary.txt")).unwrap();
    let stack = summary.kinds.iter().find(|r| r.kind == "Stack").unwrap();
    assert_eq!(stack.memory.resident_bytes, 32768);
    assert_eq!(stack.memory.dirty_bytes, 32768);
    assert_eq!(stack.memory.swapped_bytes, 0);
    assert_eq!(stack.memory.clean_bytes, 0);
    let data = summary.kinds.iter().find(|r| r.kind == "__DATA").unwrap();
    assert_eq!(data.memory.clean_bytes, 77824);
    assert_eq!(summary.zones.len(), 2);
    let zone = &summary.zones[0];
    assert_eq!(zone.zone, "DefaultMallocZone_0x1032b4000");
    assert_eq!(zone.memory.resident_bytes, 868352);
    assert_eq!(zone.allocations, 1192);
    assert_eq!(zone.allocated_bytes, 236544);
    assert_eq!(
        parse_footprint(include_str!("fixtures/footprint.txt")),
        Ok(1966488)
    );
    let regions = parse_regions(include_str!("fixtures/vmmap-regions.txt"), 3).unwrap();
    assert_eq!(regions.len(), 3);
    assert!(
        regions
            .windows(2)
            .all(|r| r[0].dirty_swapped_bytes >= r[1].dirty_swapped_bytes)
    );
    assert!(regions.iter().all(|r| !r.address.is_empty()));
}

#[test]
fn largest_regions_include_kernel_accounting_and_use_dirty_plus_swapped() {
    let regions = parse_regions("\
REGION TYPE START - END [ VSIZE RSDNT DIRTY SWAP] PRT/MAX SHRMOD REGION DETAIL
Stack Guard 1000-9000 [ 8G 0K 0K 0K] ---/rwx SM=NUL
mapped file a000-b000 [ 4K 4K 0K 0K] r--/r-- SM=COW /tmp/a file
IOSurface c000-d000 [ 2M 1M 256K 512K] rw-/rw- SM=SHM
IOKit e000-f000 [ 1.5M 1M 512K 0K] rw-/rw- SM=SHM
page table in kernel kernel-kernel [ 1M 1M 1M 0K] rw-/rw- SM=PRV charged to process physical footprint
", 3).unwrap();
    assert_eq!(regions[0].kind, "page table in kernel");
    assert_eq!(regions[0].address, "kernel-kernel");
    assert_eq!(regions[1].kind, "IOSurface");
    assert_eq!(regions[1].dirty_swapped_bytes, 786432);
    assert_eq!(regions[1].memory.clean_bytes, 786432);
    assert_eq!(regions[2].kind, "IOKit");
    assert_eq!(regions[2].memory.virtual_bytes, 1572864);
}

#[test]
fn malformed_and_truncated_reports_are_errors_and_empty_zones_are_observations() {
    for text in [
        "",
        "permission denied",
        "REGION TYPE SIZE COUNT\nMalloc 1K 1K 1K 0K 0K 0K 0K 1\n",
    ] {
        assert!(parse_summary(text).is_err(), "{text}");
    }
    let zero_zones = "REGION TYPE SIZE COUNT\nIOKit 1.5M 1M 512K 256K 0K 0K 0K 1\nTOTAL 1.5M 1M 512K 256K 0K 0K 0K 1\nMALLOC ZONE SIZE COUNT\nTOTAL 0K 0K 0K 0K 0 0 0 0% 0\n";
    let summary = parse_summary(zero_zones).unwrap();
    assert!(summary.zones.is_empty());
    assert_eq!(summary.kinds[0].memory.clean_bytes, 524288);
    for value in [
        "-1 B",
        "NaN B",
        "18446744073709551616 B",
        "18446744073709551615T",
    ] {
        assert!(parse_footprint(&format!("phys_footprint: {value}")).is_err());
    }
    for text in [
        "",
        "Malloc 1000-2000 [ 4K 4K",
        "Malloc 1000-2000 [ 4K 1K 2K 0K]",
        "Malloc 1000-2000 [ 4K 4K 4K -1K]",
    ] {
        assert!(parse_regions(text, 15).is_err(), "{text}");
    }
    assert!(parse_footprint("phys_footprint_peak: 4096 B").is_err());
}
