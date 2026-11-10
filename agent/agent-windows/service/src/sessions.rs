use std::collections::HashMap;

use midas_protocol::sessions::{OsSession, Presence, valid_sessions};
use uuid::Uuid;

const ACTIVE: i32 = 0;
const CONNECTED: i32 = 1;
const DISCONNECTED: i32 = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeSession {
    pub id: u32,
    pub state: i32,
    pub flags: i32,
    pub station: String,
    pub user: String,
    pub domain: String,
    pub logon_time: i64,
    pub console: bool,
}

#[derive(Default)]
pub struct Tracker {
    epoch: u64,
    entries: HashMap<u32, (NativeSession, Uuid)>,
}

impl Tracker {
    pub fn observe(&mut self, sessions: Vec<NativeSession>, epoch: u64) -> Option<Vec<OsSession>> {
        if epoch != self.epoch {
            self.entries.clear();
            self.epoch = epoch;
        }
        let mut next = HashMap::new();
        let mut result = Vec::new();
        for session in sessions {
            let generation = self
                .entries
                .get(&session.id)
                .filter(|(old, _)| old == &session)
                .map_or_else(Uuid::new_v4, |(_, id)| *id);
            let presence =
                if !session.user.is_empty() || matches!(session.state, ACTIVE | DISCONNECTED) {
                    Presence::Occupied
                } else if session.state == CONNECTED
                    && session.logon_time == 0
                    && session.domain.is_empty()
                    && matches!(session.flags, 0 | 1)
                {
                    Presence::Vacant
                } else {
                    Presence::Unknown
                };
            let account = if session.domain.is_empty() {
                session.user.clone()
            } else {
                format!("{}\\{}", session.domain, session.user)
            };
            result.push(OsSession {
                id: session.id,
                generation: generation.as_bytes().to_vec(),
                presence: presence as i32,
                station: session.station.clone(),
                account,
                console: session.console,
                locked: match session.flags {
                    0 => Some(true),
                    1 => Some(false),
                    _ => None,
                },
            });
            next.insert(session.id, (session, generation));
        }
        if !valid_sessions(&result) {
            self.entries.clear();
            return None;
        }
        self.entries = next;
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> NativeSession {
        NativeSession {
            id: 1,
            state: 1,
            flags: 0,
            station: "Console".into(),
            user: String::new(),
            domain: String::new(),
            logon_time: 0,
            console: true,
        }
    }

    #[test]
    fn signed_in_locked_and_disconnected_targets_remain_occupied() {
        let mut tracker = Tracker::default();
        for state in [0, 1, 4] {
            for flags in [0, 1, -1] {
                let session = NativeSession {
                    state,
                    flags,
                    user: "operator".into(),
                    logon_time: 123,
                    ..empty()
                };
                assert_eq!(
                    tracker.observe(vec![session], 0).unwrap()[0].presence,
                    Presence::Occupied as i32
                );
            }
        }
        assert_eq!(
            tracker
                .observe(
                    vec![NativeSession {
                        state: 4,
                        ..empty()
                    }],
                    0
                )
                .unwrap()[0]
                .presence,
            Presence::Occupied as i32
        );
        assert_eq!(
            tracker
                .observe(
                    vec![NativeSession {
                        state: 8,
                        ..empty()
                    }],
                    0
                )
                .unwrap()[0]
                .presence,
            Presence::Unknown as i32
        );
        assert_eq!(
            tracker.observe(vec![empty()], 0).unwrap()[0].presence,
            Presence::Vacant as i32
        );
    }

    #[test]
    fn uncertain_and_malformed_native_data_cannot_preserve_vacancy() {
        let mut tracker = Tracker::default();
        for session in [
            NativeSession {
                flags: -1,
                ..empty()
            },
            NativeSession {
                logon_time: 123,
                ..empty()
            },
            NativeSession {
                domain: "EXAMPLE".into(),
                ..empty()
            },
        ] {
            assert_eq!(
                tracker.observe(vec![session], 0).unwrap()[0].presence,
                Presence::Unknown as i32
            );
        }
        let generation = tracker.observe(vec![empty()], 0).unwrap()[0]
            .generation
            .clone();
        assert!(tracker.observe(vec![empty(), empty()], 0).is_none());
        assert_ne!(
            generation,
            tracker.observe(vec![empty()], 0).unwrap()[0].generation
        );
        assert!(
            tracker
                .observe(
                    vec![NativeSession {
                        station: "bad\nstation".into(),
                        ..empty()
                    }],
                    0
                )
                .is_none()
        );
    }

    #[test]
    fn changes_events_and_disappearance_invalidate_the_generation() {
        let mut tracker = Tracker::default();
        let first = tracker.observe(vec![empty()], 0).unwrap()[0]
            .generation
            .clone();
        assert_eq!(
            first,
            tracker.observe(vec![empty()], 0).unwrap()[0].generation
        );
        assert_ne!(
            first,
            tracker.observe(vec![empty()], 1).unwrap()[0].generation
        );
        let occupied = NativeSession {
            user: "user".into(),
            logon_time: 123,
            ..empty()
        };
        let second = tracker.observe(vec![occupied.clone()], 1).unwrap()[0]
            .generation
            .clone();
        assert_ne!(
            second,
            tracker
                .observe(
                    vec![NativeSession {
                        logon_time: 124,
                        ..occupied
                    }],
                    1
                )
                .unwrap()[0]
                .generation
        );
        tracker.observe(vec![], 1).unwrap();
        assert_ne!(
            second,
            tracker.observe(vec![empty()], 1).unwrap()[0].generation
        );
    }
}
