use std::{sync::atomic::AtomicUsize, time::Duration};

use midas_remote_auth::{Claims, SigningKey};
use prost::Message;

use super::*;
use crate::remote::{DesktopKind, Worker};

struct Broker {
    observed: Observation,
    lease: Mutex<Option<Lease>>,
    calls: AtomicUsize,
    block_at: usize,
    entered: tokio::sync::Notify,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl Native for Broker {
    fn observe(&self, _: Target) -> Result<Observation, Error> {
        if self.calls.fetch_add(1, Ordering::SeqCst) + 1 == self.block_at {
            self.entered.notify_one();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
        Ok(self.observed)
    }
    fn activate(&self, lease: Lease) -> Result<(), Error> {
        lease.capture(self.observed, Worker::Privileged)?;
        *self.lease.lock().unwrap() = Some(lease);
        Ok(())
    }
}

struct Fixture {
    key: SigningKey,
    welcome: Welcome,
    broker: Arc<Broker>,
    backend: Backend,
    prepare: wire::Prepare,
    release: std::sync::mpsc::Sender<()>,
}

impl Fixture {
    fn new(block_at: usize) -> Self {
        let connection = Connection {
            tenant: Uuid::new_v4(),
            device: Uuid::new_v4(),
            id: Uuid::new_v4(),
        };
        let observed = Observation {
            target: Target {
                connection,
                session: 2,
                generation: Uuid::new_v4(),
            },
            presence: Presence::Vacant,
            desktop: DesktopKind::Secure,
            desktop_epoch: Uuid::new_v4(),
        };
        let (release, wait) = std::sync::mpsc::channel();
        let broker = Arc::new(Broker {
            observed,
            lease: Mutex::new(None),
            calls: AtomicUsize::new(0),
            block_at,
            entered: tokio::sync::Notify::new(),
            release: Mutex::new(wait),
        });
        let key = SigningKey::generate().unwrap();
        Self {
            welcome: Welcome {
                version: 1,
                keepalive_seconds: 1,
                server_version: "test".into(),
                connection_id: connection.id.as_bytes().to_vec(),
                remote: Some(wire::Context {
                    tenant: connection.tenant.as_bytes().to_vec(),
                    device: connection.device.as_bytes().to_vec(),
                    public_key: key.public_key().to_vec(),
                }),
            },
            backend: Backend::new(connection.tenant, connection.device, broker.clone()).unwrap(),
            prepare: wire::Prepare {
                request: Uuid::new_v4().as_bytes().to_vec(),
                session: 2,
                generation: observed.target.generation.as_bytes().to_vec(),
                operator: Uuid::new_v4().as_bytes().to_vec(),
                mode: wire::Mode::Administrative as i32,
                uac: true,
            },
            key,
            broker,
            release,
        }
    }
    fn connect(&self) -> Transport {
        Transport::connect(&self.backend, &self.welcome)
            .unwrap()
            .unwrap()
    }
    fn sign(&self, challenge: wire::Challenge) -> wire::Deliver {
        let grant = self
            .key
            .sign(&Claims {
                id: Uuid::new_v4(),
                challenge: id(&challenge.challenge).unwrap(),
                operator: id(&self.prepare.operator).unwrap(),
                target: self.broker.observed.target,
                rights: Rights {
                    mode: Mode::Administrative,
                    uac: true,
                },
                issued_at: 1,
                expires_at: 31,
            })
            .unwrap();
        wire::Deliver {
            request: challenge.request,
            grant: grant.encode_to_vec(),
        }
    }
    fn allowed(&self) -> bool {
        self.broker
            .lease
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|lease| {
                lease
                    .capture(self.broker.observed, Worker::Privileged)
                    .is_ok()
            })
    }
}

async fn response(transport: &mut Transport) -> Body {
    tokio::time::timeout(Duration::from_secs(5), transport.results.recv())
        .await
        .unwrap()
        .unwrap()
}

async fn challenge(f: &Fixture, transport: &mut Transport) -> wire::Deliver {
    transport.prepare(f.prepare.clone()).unwrap();
    let Body::RemoteChallenge(challenge) = response(transport).await else {
        panic!("challenge");
    };
    assert_eq!(challenge.presence, WirePresence::Vacant as i32);
    f.sign(challenge)
}

#[tokio::test]
async fn authenticated_context_must_match_the_enrolled_identity() {
    let f = Fixture::new(0);
    for field in ["tenant", "device", "key", "connection"] {
        let mut welcome = f.welcome.clone();
        match field {
            "tenant" => {
                welcome.remote.as_mut().unwrap().tenant = Uuid::new_v4().as_bytes().to_vec()
            }
            "device" => {
                welcome.remote.as_mut().unwrap().device = Uuid::new_v4().as_bytes().to_vec()
            }
            "key" => welcome
                .remote
                .as_mut()
                .unwrap()
                .public_key
                .pop()
                .map(|_| ())
                .unwrap(),
            _ => welcome.connection_id = Uuid::nil().as_bytes().to_vec(),
        }
        assert!(Transport::connect(&f.backend, &welcome).is_err());
    }
    let mut legacy = f.welcome.clone();
    legacy.remote = None;
    assert!(Transport::connect(&f.backend, &legacy).unwrap().is_none());
}

#[tokio::test]
async fn delivery_is_single_use_and_stopping_or_disconnect_revokes_the_native_lease() {
    for end in ["revoke", "disconnect", "replay"] {
        let f = Fixture::new(0);
        let mut transport = f.connect();
        let delivery = challenge(&f, &mut transport).await;
        transport.deliver(delivery.clone()).unwrap();
        assert!(matches!(
            response(&mut transport).await,
            Body::RemoteAck(wire::Ack { accepted: true, .. })
        ));
        assert!(f.allowed());
        match end {
            "revoke" => {
                transport
                    .revoke(wire::Revoke {
                        request: f.prepare.request.clone(),
                    })
                    .unwrap();
                transport.deliver(delivery).unwrap();
            }
            "disconnect" => drop(transport),
            _ => {
                transport.deliver(delivery).unwrap();
                assert!(matches!(
                    response(&mut transport).await,
                    Body::RemoteAck(wire::Ack {
                        accepted: false,
                        ..
                    })
                ));
            }
        }
        assert!(!f.allowed(), "{end}");
    }
}

#[tokio::test]
async fn revocation_while_native_observation_is_blocked_prevents_late_activation() {
    for disconnect in [false, true] {
        let f = Fixture::new(2);
        let mut transport = f.connect();
        let delivery = challenge(&f, &mut transport).await;
        transport.deliver(delivery).unwrap();
        tokio::time::timeout(Duration::from_secs(5), f.broker.entered.notified())
            .await
            .unwrap();
        if disconnect {
            drop(transport);
            f.release.send(()).unwrap();
        } else {
            transport
                .revoke(wire::Revoke {
                    request: f.prepare.request.clone(),
                })
                .unwrap();
            f.release.send(()).unwrap();
            assert!(matches!(
                response(&mut transport).await,
                Body::RemoteAck(wire::Ack {
                    accepted: false,
                    ..
                })
            ));
        }
        let _finished = tokio::time::timeout(
            Duration::from_secs(5),
            f.backend.slot.clone().acquire_owned(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!f.allowed());
    }
}

#[tokio::test]
async fn reconnect_does_not_start_another_blocked_native_probe() {
    let f = Fixture::new(1);
    let mut old = f.connect();
    old.prepare(f.prepare.clone()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), f.broker.entered.notified())
        .await
        .unwrap();
    drop(old);
    let mut next = f.connect();
    next.prepare(f.prepare.clone()).unwrap();
    assert!(matches!(
        response(&mut next).await,
        Body::RemoteChallenge(wire::Challenge { presence: 0, .. })
    ));
    assert_eq!(f.broker.calls.load(Ordering::SeqCst), 1);
    f.release.send(()).unwrap();
    assert!(!f.allowed());
}

#[tokio::test]
async fn tampering_and_old_challenges_cannot_activate_a_new_request() {
    let f = Fixture::new(0);
    let mut transport = f.connect();
    let old = challenge(&f, &mut transport).await;
    assert!(transport.prepare(f.prepare.clone()).is_err());
    transport
        .revoke(wire::Revoke {
            request: f.prepare.request.clone(),
        })
        .unwrap();
    let _new = challenge(&f, &mut transport).await;
    transport.deliver(old).unwrap();
    assert!(matches!(
        response(&mut transport).await,
        Body::RemoteAck(wire::Ack {
            accepted: false,
            ..
        })
    ));
    assert!(!f.allowed());
    transport
        .revoke(wire::Revoke {
            request: f.prepare.request.clone(),
        })
        .unwrap();
    let mut changed = challenge(&f, &mut transport).await;
    let last = changed.grant.len() - 1;
    changed.grant[last] ^= 1;
    transport.deliver(changed).unwrap();
    assert!(matches!(
        response(&mut transport).await,
        Body::RemoteAck(wire::Ack {
            accepted: false,
            ..
        })
    ));
    assert!(!f.allowed());
    assert!(
        transport
            .deliver(wire::Deliver {
                request: f.prepare.request,
                grant: vec![0; midas_remote_auth::MAX_GRANT_BYTES + 1]
            })
            .is_err()
    );
}
