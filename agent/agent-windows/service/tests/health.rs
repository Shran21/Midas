#![cfg(windows)]

use midas_agent_windows::win::health::{CHECKS, check};
use midas_protocol::HealthState;

/// Every check gives a definite answer on a real machine; with user rights BitLocker may
/// legitimately be unavailable, but it must say why instead of disappearing.
#[test]
fn the_real_machine_answers_every_check() {
    let checks = check();
    let ids: Vec<&str> = checks.iter().map(|check| check.id.as_str()).collect();
    assert_eq!(ids, CHECKS);
    for check in &checks {
        let state = HealthState::try_from(check.state).unwrap();
        println!(
            "{}: {state:?} {} - {} {:?}",
            check.id, check.code, check.summary, check.subjects
        );
        assert_ne!(state, HealthState::Unspecified);
        assert!(!check.code.is_empty() && !check.summary.is_empty());
    }
    let state = |id: &str| {
        let check = checks.iter().find(|check| check.id == id).unwrap();
        HealthState::try_from(check.state).unwrap()
    };
    assert_ne!(state("services"), HealthState::Unavailable);
    assert_ne!(state("storage"), HealthState::Unavailable);
}
