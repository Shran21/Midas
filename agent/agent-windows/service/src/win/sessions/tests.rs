use std::time::Duration;

use super::*;

pub(crate) struct Fixture {
    pub observer: Arc<Observer>,
    pub target: OsSession,
    sessions: Vec<NativeSession>,
}

impl Fixture {
    pub fn new() -> Self {
        let observer = Arc::new(Observer::default());
        let sessions = vec![NativeSession {
            id: 1,
            state: 0,
            flags: 1,
            station: "Console".into(),
            user: "operator".into(),
            domain: "EXAMPLE".into(),
            logon_time: 123,
            console: true,
        }];
        let target = observer
            .collect_with(|| Some((sessions.clone(), 0)))
            .unwrap()
            .remove(0);
        Self {
            observer,
            target,
            sessions,
        }
    }

    pub fn binding(&self) -> Binding {
        self.observer
            .bind_with(&self.target, || self.collect(Some(self.sessions.clone())))
            .unwrap()
    }

    fn collect(&self, sessions: Option<Vec<NativeSession>>) -> Option<Vec<OsSession>> {
        self.observer.collect_with(|| {
            sessions.map(|sessions| (sessions, self.observer.epoch.load(Ordering::Acquire)))
        })
    }

    pub fn unavailable(&self) {
        assert!(self.collect(None).is_none());
    }
}

async fn ended(binding: &Binding) {
    tokio::time::timeout(Duration::from_secs(2), binding.ended())
        .await
        .unwrap();
    assert!(binding.check().is_err());
}

#[tokio::test]
async fn unchanged_observations_preserve_binding_and_wait_cancellation_is_harmless() {
    let fixture = Fixture::new();
    let binding = fixture.binding();
    for _ in 0..4 {
        assert_eq!(
            fixture.collect(Some(fixture.sessions.clone())).unwrap(),
            vec![fixture.target.clone()]
        );
        binding.check().unwrap();
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(10), binding.ended())
            .await
            .is_err()
    );
    binding.check().unwrap();
    fixture.observer.changed();
    ended(&binding).await;
}

#[tokio::test]
async fn service_events_wake_current_and_late_waiters_without_reviving_old_bindings() {
    let fixture = Fixture::new();
    let binding = fixture.binding();
    let first = tokio::spawn({
        let binding = binding.clone();
        async move { binding.ended().await }
    });
    let second = tokio::spawn({
        let binding = binding.clone();
        async move { binding.ended().await }
    });
    tokio::task::yield_now().await;
    fixture.observer.changed();
    for task in [first, second] {
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }
    ended(&binding).await;
    ended(&binding).await;
    let current = fixture
        .collect(Some(fixture.sessions.clone()))
        .unwrap()
        .remove(0);
    assert_ne!(current.generation, fixture.target.generation);
    assert!(
        fixture
            .observer
            .bind_with(&fixture.target, || Some(vec![current.clone()]))
            .is_err()
    );
    let new_binding = fixture
        .observer
        .bind_with(&current, || fixture.collect(Some(fixture.sessions.clone())))
        .unwrap();
    new_binding.check().unwrap();
    assert!(binding.check().is_err());
}

#[tokio::test]
async fn failed_or_changed_native_reads_invalidate_without_a_service_event() {
    for change in 0..6 {
        let fixture = Fixture::new();
        let binding = fixture.binding();
        let mut sessions = fixture.sessions.clone();
        match change {
            0 => {
                fixture.unavailable();
            }
            1 => {
                sessions[0].flags = 0;
                fixture.collect(Some(sessions));
            }
            2 => {
                sessions[0].logon_time += 1;
                fixture.collect(Some(sessions));
            }
            3 => {
                fixture.collect(Some(vec![]));
            }
            4 => {
                sessions.push(sessions[0].clone());
                assert!(fixture.collect(Some(sessions)).is_none());
            }
            5 => {
                sessions[0].console = false;
                fixture.collect(Some(sessions));
            }
            _ => unreachable!(),
        }
        ended(&binding).await;
        fixture.collect(Some(fixture.sessions.clone())).unwrap();
        assert!(binding.check().is_err());
    }
}

#[tokio::test]
async fn events_during_binding_or_native_read_are_not_lost() {
    let fixture = Fixture::new();
    let binding = fixture.binding();
    assert!(
        fixture
            .observer
            .bind_with(&fixture.target, || {
                fixture.observer.changed();
                Some(vec![fixture.target.clone()])
            })
            .is_err()
    );
    ended(&binding).await;
    assert!(
        fixture
            .observer
            .collect_with(|| {
                let epoch = fixture.observer.epoch.load(Ordering::Acquire);
                fixture.observer.changed();
                Some((fixture.sessions.clone(), epoch))
            })
            .is_none()
    );
}

#[tokio::test]
async fn concurrent_notification_and_wait_registration_always_ends() {
    for _ in 0..64 {
        let fixture = Fixture::new();
        let binding = fixture.binding();
        let observer = fixture.observer.clone();
        let thread = std::thread::spawn(move || observer.changed());
        ended(&binding).await;
        thread.join().unwrap();
    }
}

#[tokio::test]
async fn stopped_observer_cannot_create_new_bindings_or_probe_native_state() {
    let fixture = Fixture::new();
    let binding = fixture.binding();
    fixture.observer.stop();
    ended(&binding).await;
    assert!(
        fixture
            .observer
            .collect_with(|| panic!("stopped observer read WTS"))
            .is_none()
    );
    assert!(fixture.observer.bind(&fixture.target).is_err());
    fixture.observer.changed();
    assert!(fixture.collect(Some(fixture.sessions.clone())).is_none());
    ended(&binding).await;
}

#[tokio::test]
async fn observer_loss_and_poisoned_state_end_bindings() {
    let fixture = Fixture::new();
    let binding = fixture.binding();
    drop(fixture);
    ended(&binding).await;

    let fixture = Fixture::new();
    let binding = fixture.binding();
    let observer = fixture.observer.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = observer.state.lock().unwrap();
            panic!("poison observation state");
        })
        .join()
        .is_err()
    );
    assert!(fixture.observer.collect().is_none());
    ended(&binding).await;
}
