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

#[test]
fn a_directory_identity_is_reported_only_when_the_machine_has_one() {
    let identity = midas_agent_windows::win::inventory::directory_identity();
    let snapshot = collect();
    let reported = (
        snapshot.get(&key("directory", "domain")).cloned(),
        snapshot.get(&key("directory", "object_guid")).cloned(),
    );
    match identity {
        Some((domain, guid)) => {
            assert!(domain.contains('.') && domain == domain.to_lowercase());
            assert_eq!(guid.len(), 36);
            assert_eq!(reported, (Some(domain), Some(guid)));
        }
        // A workgroup computer, such as a CI runner, has no directory account.
        None => assert_eq!(reported, (None, None)),
    }
}
