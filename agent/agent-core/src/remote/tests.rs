use super::*;
use midas_remote_auth::SigningKey;

struct Fixture {
    key: SigningKey,
    channel: Channel,
    target: Target,
    operator: Uuid,
    rights: Rights,
    observation: Observation,
}
impl Fixture {
    fn new(mode: Mode, uac: bool) -> Self {
        let key = SigningKey::generate().unwrap();
        let connection = Connection {
            tenant: Uuid::new_v4(),
            device: Uuid::new_v4(),
            id: Uuid::new_v4(),
        };
        let target = Target {
            connection,
            session: 7,
            generation: Uuid::new_v4(),
        };
        Self {
            channel: Channel::from_authenticated_server(connection, key.public_key()).unwrap(),
            key,
            target,
            operator: Uuid::new_v4(),
            rights: Rights { mode, uac },
            observation: Observation {
                target,
                presence: Presence::Vacant,
                desktop: DesktopKind::User,
                desktop_epoch: Uuid::new_v4(),
            },
        }
    }
    fn prepare(&self) -> (Pending, Claims) {
        let pending = self
            .channel
            .prepare(self.target, self.operator, self.rights)
            .unwrap();
        let claims = Claims {
            id: Uuid::new_v4(),
            challenge: pending.challenge(),
            operator: self.operator,
            target: self.target,
            rights: self.rights,
            issued_at: 100,
            expires_at: 130,
        };
        (pending, claims)
    }
    fn lease(&self) -> Lease {
        let (pending, claims) = self.prepare();
        pending
            .accept(&self.key.sign(&claims).unwrap(), self.observation)
            .unwrap()
    }
}

#[tokio::test]
async fn revocation_wakes_all_current_and_late_waiters() {
    let f = Fixture::new(Mode::Administrative, true);
    let lease = f.lease();
    let first = lease.ended();
    let second = lease.ended();
    tokio::pin!(first, second);
    assert!(futures_util::poll!(first.as_mut()).is_pending());
    assert!(futures_util::poll!(second.as_mut()).is_pending());
    lease.cancellation().revoke();
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(first, second, lease.ended());
    })
    .await
    .unwrap();
    assert_eq!(
        lease.capture(f.observation, Worker::Privileged),
        Err(Error::Revoked)
    );
}

#[tokio::test]
async fn channel_loss_wakes_a_waiter_that_retains_the_channel_state() {
    let f = Fixture::new(Mode::Normal, false);
    let lease = f.lease();
    let companion = f.lease();
    let ended = lease.ended();
    let other = companion.ended();
    tokio::pin!(ended, other);
    assert!(futures_util::poll!(ended.as_mut()).is_pending());
    assert!(futures_util::poll!(other.as_mut()).is_pending());
    drop(f.channel);
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(ended, other, companion.ended());
    })
    .await
    .unwrap();
    assert_eq!(
        lease.capture(f.observation, Worker::User),
        Err(Error::Revoked)
    );
}

#[tokio::test]
async fn validation_failure_notifies_supervision_without_another_operation() {
    let f = Fixture::new(Mode::Normal, false);
    let lease = f.lease();
    let ended = lease.ended();
    tokio::pin!(ended);
    assert!(futures_util::poll!(ended.as_mut()).is_pending());
    let changed = Observation {
        presence: Presence::Occupied,
        ..f.observation
    };
    assert_eq!(
        lease.capture(changed, Worker::User),
        Err(Error::ConsentRequired)
    );
    tokio::time::timeout(Duration::from_secs(2), ended)
        .await
        .unwrap();
}

#[tokio::test]
async fn idle_lease_expires_at_its_existing_deadline() {
    let f = Fixture::new(Mode::Normal, false);
    let lease = f.lease();
    let deadline = Instant::now() + Duration::from_millis(20);
    lease.life.current.lock().unwrap().deadline = deadline;
    tokio::time::timeout(Duration::from_secs(2), lease.ended())
        .await
        .unwrap();
    assert!(Instant::now() >= deadline);
    assert_eq!(
        lease.capture(f.observation, Worker::User),
        Err(Error::Revoked)
    );
    tokio::time::timeout(Duration::from_secs(2), lease.ended())
        .await
        .unwrap();
}

#[tokio::test]
async fn cancelling_a_wait_does_not_cancel_or_extend_its_lease() {
    let f = Fixture::new(Mode::Normal, false);
    let lease = f.lease();
    let deadline = lease.life.deadline();
    {
        let ended = lease.ended();
        tokio::pin!(ended);
        assert!(futures_util::poll!(ended.as_mut()).is_pending());
    }
    assert_eq!(lease.life.deadline(), deadline);
    assert_eq!(lease.capture(f.observation, Worker::User), Ok(()));
    f.channel.revoke();
    tokio::time::timeout(Duration::from_secs(2), lease.ended())
        .await
        .unwrap();
}

#[tokio::test]
async fn revocation_racing_wait_registration_is_not_lost() {
    let f = Fixture::new(Mode::Normal, false);
    for _ in 0..32 {
        let lease = f.lease();
        let cancel = lease.cancellation();
        let thread = std::thread::spawn(move || cancel.revoke());
        tokio::time::timeout(Duration::from_secs(2), lease.ended())
            .await
            .unwrap();
        thread.join().unwrap();
    }
}

#[test]
fn a_grant_is_consumed_once_for_its_exact_request() {
    let f = Fixture::new(Mode::Normal, false);
    let (pending, claims) = f.prepare();
    let grant = f.key.sign(&claims).unwrap();
    let lease = pending.accept(&grant, f.observation).unwrap();
    assert_eq!(lease.capture(f.observation, Worker::User), Ok(()));
    assert_eq!(
        lease.capture(f.observation, Worker::Privileged),
        Err(Error::Desktop)
    );
    let (next, _) = f.prepare();
    assert!(matches!(
        next.accept(&grant, f.observation),
        Err(Error::Target)
    ));
    let changes: [fn(&mut Claims); 9] = [
        |c| c.challenge = Uuid::new_v4(),
        |c| c.operator = Uuid::new_v4(),
        |c| c.target.connection.tenant = Uuid::new_v4(),
        |c| c.target.connection.device = Uuid::new_v4(),
        |c| c.target.connection.id = Uuid::new_v4(),
        |c| c.target.session += 1,
        |c| c.target.generation = Uuid::new_v4(),
        |c| c.rights.uac = true,
        |c| c.rights.mode = Mode::Administrative,
    ];
    for change in changes {
        let (pending, mut c) = f.prepare();
        change(&mut c);
        assert!(matches!(
            pending.accept(&f.key.sign(&c).unwrap(), f.observation),
            Err(Error::Target)
        ));
    }
}

#[test]
fn local_elapsed_time_bounds_delayed_delivery_without_wall_clock_trust() {
    let f = Fixture::new(Mode::Normal, false);
    for seconds in [1, 30] {
        let (pending, mut claims) = f.prepare();
        claims.issued_at = 253_402_300_700;
        claims.expires_at = claims.issued_at + seconds;
        let now = pending.started + Duration::from_secs(seconds as u64);
        assert!(matches!(
            pending.accept_at(&f.key.sign(&claims).unwrap(), f.observation, now),
            Err(Error::Expired)
        ));
    }
    let (pending, claims) = f.prepare();
    let now = pending.started - Duration::from_secs(1);
    assert!(matches!(
        pending.accept_at(&f.key.sign(&claims).unwrap(), f.observation, now),
        Err(Error::Expired)
    ));
    let (pending, claims) = f.prepare();
    let started = pending.started;
    let lease = pending
        .accept(&f.key.sign(&claims).unwrap(), f.observation)
        .unwrap();
    assert_eq!(
        lease.validate(f.observation, started + Duration::from_secs(30)),
        Err(Error::Expired)
    );
    assert_eq!(
        lease.capture(f.observation, Worker::User),
        Err(Error::Revoked)
    );
}

#[test]
fn only_the_privileged_worker_with_separate_uac_permission_sees_secure_desktop() {
    for mode in [Mode::Normal, Mode::Administrative] {
        for uac in [false, true] {
            let f = Fixture::new(mode, uac);
            let lease = f.lease();
            let secure = Observation {
                desktop: DesktopKind::Secure,
                desktop_epoch: Uuid::new_v4(),
                ..f.observation
            };
            assert_eq!(lease.capture(secure, Worker::User), Err(Error::Desktop));
            assert_eq!(
                lease.capture(secure, Worker::Privileged),
                if uac { Ok(()) } else { Err(Error::Desktop) }
            );
            assert_eq!(lease.capture(f.observation, Worker::User), Ok(()));
        }
    }
}

#[test]
fn desktop_switch_discards_queued_input_even_if_an_epoch_was_reused() {
    let f = Fixture::new(Mode::Administrative, true);
    let lease = f.lease();
    for changed in [
        Observation {
            desktop_epoch: Uuid::new_v4(),
            ..f.observation
        },
        Observation {
            desktop: DesktopKind::Secure,
            ..f.observation
        },
    ] {
        let permit = lease
            .prepare_input(f.observation, Worker::Privileged)
            .unwrap();
        assert_eq!(lease.commit_input(permit, changed), Err(Error::Desktop));
    }
    let permit = lease.prepare_input(f.observation, Worker::User).unwrap();
    assert_eq!(lease.commit_input(permit, f.observation), Ok(()));
    let another = f.lease();
    let permit = lease.prepare_input(f.observation, Worker::User).unwrap();
    assert_eq!(
        another.commit_input(permit, f.observation),
        Err(Error::Desktop)
    );
}

#[test]
fn native_presence_or_target_changes_permanently_stop_an_unattended_lease() {
    let f = Fixture::new(Mode::Normal, false);
    for presence in [Presence::Occupied, Presence::Unknown] {
        let (pending, claims) = f.prepare();
        let changed = Observation {
            presence,
            ..f.observation
        };
        assert!(
            pending
                .accept(&f.key.sign(&claims).unwrap(), changed)
                .is_err()
        );
        let lease = f.lease();
        assert!(lease.capture(changed, Worker::User).is_err());
        assert_eq!(
            lease.capture(f.observation, Worker::User),
            Err(Error::Revoked)
        );
    }
    let lease = f.lease();
    let changed = Observation {
        target: Target {
            generation: Uuid::new_v4(),
            ..f.target
        },
        ..f.observation
    };
    assert_eq!(lease.capture(changed, Worker::User), Err(Error::Target));
    assert_eq!(
        lease.capture(f.observation, Worker::User),
        Err(Error::Revoked)
    );
}

#[test]
fn cancellation_and_transport_loss_invalidate_active_and_pending_work() {
    let f = Fixture::new(Mode::Normal, false);
    let lease = f.lease();
    let permit = lease.prepare_input(f.observation, Worker::User).unwrap();
    lease.cancellation().revoke();
    assert_eq!(
        lease.commit_input(permit, f.observation),
        Err(Error::Revoked)
    );
    let live_lease = f.lease();
    let (pending, claims) = f.prepare();
    let grant = f.key.sign(&claims).unwrap();
    f.channel.revoke();
    assert!(matches!(
        pending.accept(&grant, f.observation),
        Err(Error::Revoked)
    ));
    assert_eq!(
        live_lease.capture(f.observation, Worker::User),
        Err(Error::Revoked)
    );
    assert!(matches!(
        f.channel.prepare(f.target, f.operator, f.rights),
        Err(Error::Revoked)
    ));

    let f = Fixture::new(Mode::Normal, false);
    let lease = f.lease();
    let (pending, claims) = f.prepare();
    let grant = f.key.sign(&claims).unwrap();
    drop(f.channel);
    assert_eq!(
        lease.capture(f.observation, Worker::User),
        Err(Error::Revoked)
    );
    assert!(matches!(
        pending.accept(&grant, f.observation),
        Err(Error::Revoked)
    ));
}

#[test]
fn a_consented_lease_serves_only_the_occupied_desktop_that_answered() {
    let f = Fixture::new(Mode::Normal, true);
    let occupied = Observation {
        presence: Presence::Occupied,
        ..f.observation
    };
    let (pending, _) = f.prepare();
    let before = pending.challenge();
    let pending = pending.consented();
    assert_ne!(pending.challenge(), before);
    let claims = Claims {
        id: Uuid::new_v4(),
        challenge: before,
        operator: f.operator,
        target: f.target,
        rights: f.rights,
        issued_at: 100,
        expires_at: 130,
    };
    assert!(matches!(
        pending.accept(&f.key.sign(&claims).unwrap(), occupied),
        Err(Error::Target)
    ));

    let (pending, mut claims) = f.prepare();
    let pending = pending.consented();
    claims.challenge = pending.challenge();
    let lease = pending
        .accept(&f.key.sign(&claims).unwrap(), occupied)
        .unwrap();
    assert!(lease.attended());
    assert_eq!(lease.capture(occupied, Worker::User), Ok(()));
    assert_eq!(
        lease.capture(f.observation, Worker::User),
        Err(Error::Target)
    );
    assert_eq!(lease.capture(occupied, Worker::User), Err(Error::Revoked));

    let (pending, mut claims) = f.prepare();
    let pending = pending.consented();
    claims.challenge = pending.challenge();
    assert!(matches!(
        pending.accept(&f.key.sign(&claims).unwrap(), f.observation),
        Err(Error::Target)
    ));
    let lease = f.lease();
    assert!(!lease.attended());
    assert_eq!(
        lease.capture(occupied, Worker::User),
        Err(Error::ConsentRequired)
    );
}

fn renewal(f: &Fixture, previous: Uuid) -> (SignedGrant, Uuid) {
    let id = Uuid::new_v4();
    let claims = Claims {
        id,
        challenge: previous,
        operator: f.operator,
        target: f.target,
        rights: f.rights,
        issued_at: 200,
        expires_at: 230,
    };
    (f.key.sign(&claims).unwrap(), id)
}

#[tokio::test]
async fn a_renewal_extends_only_the_last_grant_of_its_own_lease() {
    let f = Fixture::new(Mode::Normal, true);
    let lease = f.lease();
    let renewer = lease.renewer();
    let short = Instant::now() + Duration::from_millis(50);
    lease.life.current.lock().unwrap().deadline = short;
    let (first, next) = renewal(&f, lease.id());
    assert_eq!(renewer.renew(&first, f.observation), Ok(()));
    assert!(lease.life.deadline() > Instant::now() + Duration::from_secs(25));
    // The same renewal, or one chained to an older grant, cannot be applied again.
    assert_eq!(renewer.renew(&first, f.observation), Err(Error::Target));
    let (stale, _) = renewal(&f, lease.id());
    assert_eq!(renewer.renew(&stale, f.observation), Err(Error::Target));
    let (second, _) = renewal(&f, next);
    assert_eq!(renewer.renew(&second, f.observation), Ok(()));
    // Another key, another target or more rights do not extend it.
    let other = Fixture::new(Mode::Normal, true);
    let (foreign, _) = renewal(&other, lease.id());
    assert!(renewer.renew(&foreign, f.observation).is_err());
    let wider = Claims {
        id: Uuid::new_v4(),
        challenge: lease.life.current.lock().unwrap().id,
        operator: f.operator,
        target: f.target,
        rights: Rights {
            mode: Mode::Administrative,
            uac: true,
        },
        issued_at: 200,
        expires_at: 230,
    };
    assert_eq!(
        renewer.renew(&f.key.sign(&wider).unwrap(), f.observation),
        Err(Error::Target)
    );
    assert_eq!(lease.capture(f.observation, Worker::User), Ok(()));
}

#[tokio::test]
async fn without_renewal_the_lease_ends_and_says_why() {
    let f = Fixture::new(Mode::Normal, false);
    let lease = f.lease();
    let renewer = lease.renewer();
    lease.life.current.lock().unwrap().deadline = Instant::now() + Duration::from_millis(30);
    tokio::time::timeout(Duration::from_secs(2), lease.ended())
        .await
        .unwrap();
    assert_eq!(renewer.ended().await, Ending::Expired);
    let (late, _) = renewal(&f, lease.id());
    assert_eq!(renewer.renew(&late, f.observation), Err(Error::Revoked));
}

#[tokio::test]
async fn a_local_end_stops_at_once_and_a_changed_desktop_ends_the_lease() {
    let f = Fixture::new(Mode::Normal, false);
    let lease = f.lease();
    let renewer = lease.renewer();
    lease.cancellation().end_locally();
    assert_eq!(
        lease.capture(f.observation, Worker::User),
        Err(Error::Revoked)
    );
    assert_eq!(renewer.ended().await, Ending::Local);

    let lease = f.lease();
    let renewer = lease.renewer();
    let elsewhere = Observation {
        desktop_epoch: Uuid::nil(),
        ..f.observation
    };
    let (renewal, _) = renewal(&f, lease.id());
    assert_eq!(renewer.renew(&renewal, elsewhere), Err(Error::Target));
    assert_eq!(renewer.ended().await, Ending::Desktop);
}
