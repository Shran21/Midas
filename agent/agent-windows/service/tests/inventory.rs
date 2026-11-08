#![cfg(windows)]

use midas_agent_core::inventory::key;
use midas_agent_windows::win::inventory::collect;

#[test]
fn the_real_machine_reports_software_administrators_and_os() {
    let snapshot = collect();
    let count = |kind: &str| snapshot.keys().filter(|item| item.kind == kind).count();
    assert!(count("software") > 0, "no installed software found");
    assert!(count("admin") > 0, "no local administrators found");
    assert!(snapshot.contains_key(&key("os", "name")));
    assert!(
        snapshot
            .values()
            .all(|value| value.chars().count() <= midas_protocol::MAX_INVENTORY_TEXT)
    );
}
