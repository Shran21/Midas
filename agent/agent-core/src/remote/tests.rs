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
