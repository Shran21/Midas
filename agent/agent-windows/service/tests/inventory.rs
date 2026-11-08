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
    // Desktop editions report Client; the hosted runners run Windows Server with a desktop.
    let installation = midas_agent_windows::win::inventory::installation_type();
    assert!(
        matches!(installation.as_deref(), Some("Client" | "Server")),
        "{installation:?}"
    );
    assert_eq!(
        snapshot.get(&key("os", "installation")),
        installation.as_ref()
    );
    assert!(
        snapshot
            .values()
            .all(|value| value.chars().count() <= midas_protocol::MAX_INVENTORY_TEXT)
    );
}

#[test]
fn a_directory_identity_is_reported_only_when_the_machine_has_one() {
    let joined = midas_agent_windows::win::inventory::domain_joined();
    let snapshot = collect();
    let domain = snapshot.get(&key("directory", "domain"));
    let guid = snapshot.get(&key("directory", "object_guid"));
    if !joined {
        // A workgroup computer, such as a CI runner, has no directory account.
        assert_eq!((domain, guid), (None, None));
        return;
    }
    let domain = domain.expect("a joined computer reports its domain even offline");
    assert!(domain.contains('.') && *domain == domain.to_lowercase());
    // The GUID needs a reachable domain controller; when it is known, it is well formed.
    if let Some(guid) = guid {
        assert_eq!(guid.len(), 36);
    }
}
