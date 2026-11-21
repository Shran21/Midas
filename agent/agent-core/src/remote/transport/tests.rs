use std::{
    sync::atomic::{AtomicBool, AtomicUsize},
    time::Duration,
};

use midas_remote_auth::{Claims, SigningKey};
use prost::Message;

use super::*;
use crate::remote::{DesktopKind, Worker};

/// The transfer the fixture's request asks for; the grant must carry exactly this.
const TRANSFER: Transfer = Transfer {
    clipboard: true,
    upload: false,
    download: true,
};

struct Broker {
    observed: Observation,
    occupied: AtomicBool,
    /// The user's desktop changes while the consent window is open.
    move_on_answer: AtomicBool,
    moved: AtomicBool,
    /// `None` keeps the window open until the request is revoked, then answers Allowed late.
    answer: Mutex<Option<Consent>>,
    asked: Mutex<Vec<ConsentRequest>>,
    lease: Mutex<Option<Lease>>,
    calls: AtomicUsize,
    block_at: usize,
    entered: tokio::sync::Notify,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
    transfers: Mutex<Vec<Report>>,
    shown: Mutex<Vec<String>>,
    said: Mutex<Vec<String>>,
}

impl Broker {
    fn current(&self) -> Observation {
        let mut observed = self.observed;
        if self.occupied.load(Ordering::SeqCst) {
            observed.presence = Presence::Occupied;
            observed.desktop = DesktopKind::User;
        }
        if self.moved.load(Ordering::SeqCst) {
            observed.desktop_epoch = observed.target.generation;
        }
        observed
    }
}

fn worker(lease: &Lease) -> Worker {
    if lease.attended() {
        Worker::User
    } else {
        Worker::Privileged
    }
}

impl Native for Broker {
    fn transfers(&self) -> Vec<Report> {
        std::mem::take(&mut *self.transfers.lock().unwrap())
    }
    fn chat(&self, text: &str) {
        self.shown.lock().unwrap().push(text.to_owned());
    }
    fn chats(&self) -> Vec<String> {
        std::mem::take(&mut *self.said.lock().unwrap())
    }
    fn observe(&self, _: Target) -> Result<Observation, Error> {
        if self.calls.fetch_add(1, Ordering::SeqCst) + 1 == self.block_at {
            self.entered.notify_one();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
        Ok(self.current())
    }
    fn activate(&self, lease: Lease) -> Result<(), Error> {
        lease.capture(self.current(), worker(&lease))?;
        *self.lease.lock().unwrap() = Some(lease);
        Ok(())
    }
    fn consent(&self, request: &ConsentRequest, stopped: &dyn Fn() -> bool) -> Consent {
        self.asked.lock().unwrap().push(request.clone());
        if self.move_on_answer.load(Ordering::SeqCst) {
            self.moved.store(true, Ordering::SeqCst);
        }
        if let Some(answer) = *self.answer.lock().unwrap() {
            return answer;
        }
        self.entered.notify_one();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !stopped() {
            assert!(std::time::Instant::now() < deadline, "never revoked");
            std::thread::sleep(Duration::from_millis(5));
        }
        Consent::Allowed
    }
    fn media(&self, offer: &str, relays: &[Relay]) -> Result<String, Error> {
        Ok(format!(
            "answer:{offer}{}",
            relays
                .iter()
                .map(|relay| format!(" via {}", relay.urls.join(",")))
                .collect::<String>()
        ))
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
            occupied: AtomicBool::new(false),
            move_on_answer: AtomicBool::new(false),
            moved: AtomicBool::new(false),
            answer: Mutex::new(Some(Consent::Allowed)),
            asked: Mutex::new(Vec::new()),
            lease: Mutex::new(None),
            calls: AtomicUsize::new(0),
            block_at,
            entered: tokio::sync::Notify::new(),
            release: Mutex::new(wait),
            transfers: Mutex::new(Vec::new()),
            shown: Mutex::new(Vec::new()),
            said: Mutex::new(Vec::new()),
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
                consent: None,
                clipboard: TRANSFER.clipboard,
                upload: TRANSFER.upload,
                download: TRANSFER.download,
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
                    transfer: TRANSFER,
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
            .is_some_and(|lease| lease.capture(self.broker.current(), worker(lease)).is_ok())
    }
    /// An attended request for the occupied desktop, answered with `answer`.
    fn attended(answer: Option<Consent>) -> Self {
        let mut f = Self::new(0);
        f.broker.occupied.store(true, Ordering::SeqCst);
        *f.broker.answer.lock().unwrap() = answer;
        f.prepare.consent = Some(wire::ConsentRequest {
            operator_name: "Kovács\u{202e}Péter".into(),
            customer_name: "  Norda   Kft.\u{200b} ".into(),
            timeout_seconds: 60,
        });
        f
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

async fn attended_challenge(transport: &mut Transport, f: &Fixture) -> wire::Challenge {
    transport.prepare(f.prepare.clone()).unwrap();
    let Body::RemoteChallenge(challenge) = response(transport).await else {
        panic!("challenge");
    };
    assert_eq!(challenge.presence, WirePresence::Occupied as i32);
    challenge
}

#[tokio::test]
async fn an_occupied_desktop_gets_a_challenge_only_after_the_local_user_allows() {
    let f = Fixture::attended(Some(Consent::Allowed));
    let mut transport = f.connect();
    let challenge = attended_challenge(&mut transport, &f).await;
    assert_eq!(challenge.consent, wire::Consent::Allowed as i32);
    let asked = f.broker.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].operator_name, "KovácsPéter");
    assert_eq!(asked[0].customer_name, "Norda Kft.");
    assert_eq!(asked[0].timeout, Duration::from_secs(60));
    assert_eq!(asked[0].target, f.broker.observed.target);
    assert_eq!(asked[0].operator, id(&f.prepare.operator).unwrap());
    assert_eq!(
        asked[0].rights,
        Rights {
            mode: Mode::Administrative,
            uac: true,
            transfer: TRANSFER,
        }
    );
    transport.deliver(f.sign(challenge)).unwrap();
    assert!(matches!(
        response(&mut transport).await,
        Body::RemoteAck(wire::Ack { accepted: true, .. })
    ));
    assert!(f.allowed());
    let lease = f.broker.lease.lock().unwrap().take().unwrap();
    assert!(lease.attended());
    assert_eq!(
        lease.capture(f.broker.observed, Worker::User),
        Err(Error::Target)
    );
}

#[tokio::test]
async fn declined_expired_or_unavailable_answers_leave_nothing_to_deliver() {
    for answer in [Consent::Declined, Consent::Expired, Consent::Unavailable] {
        let f = Fixture::attended(Some(answer));
        let mut transport = f.connect();
        let challenge = attended_challenge(&mut transport, &f).await;
        assert!(challenge.challenge.is_empty(), "{answer:?}");
        let expected = match answer {
            Consent::Declined => wire::Consent::Declined,
            Consent::Expired => wire::Consent::Expired,
            _ => wire::Consent::Unavailable,
        };
        assert_eq!(challenge.consent, expected as i32);
        let late = f.sign(wire::Challenge {
            challenge: Uuid::new_v4().as_bytes().to_vec(),
            ..challenge
        });
        transport.deliver(late).unwrap();
        assert!(matches!(
            response(&mut transport).await,
            Body::RemoteAck(wire::Ack {
                accepted: false,
                ..
            })
        ));
        assert!(!f.allowed(), "{answer:?}");
    }
}

#[tokio::test]
async fn an_answer_after_revocation_or_from_a_changed_desktop_is_discarded() {
    let f = Fixture::attended(None);
    let mut transport = f.connect();
    transport.prepare(f.prepare.clone()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), f.broker.entered.notified())
        .await
        .unwrap();
    transport
        .revoke(wire::Revoke {
            request: f.prepare.request.clone(),
        })
        .unwrap();
    let Body::RemoteChallenge(challenge) = response(&mut transport).await else {
        panic!("challenge");
    };
    assert!(challenge.challenge.is_empty());
    assert_eq!(challenge.consent, wire::Consent::Cancelled as i32);

    let f = Fixture::attended(Some(Consent::Allowed));
    f.broker.move_on_answer.store(true, Ordering::SeqCst);
    let mut transport = f.connect();
    let challenge = attended_challenge(&mut transport, &f).await;
    assert!(challenge.challenge.is_empty());
    assert_eq!(challenge.consent, wire::Consent::Cancelled as i32);
    assert!(!f.allowed());
}

#[tokio::test]
async fn nobody_is_asked_without_a_signed_in_user_or_for_an_unattended_request() {
    let f = Fixture::attended(Some(Consent::Allowed));
    f.broker.occupied.store(false, Ordering::SeqCst);
    let mut transport = f.connect();
    transport.prepare(f.prepare.clone()).unwrap();
    let Body::RemoteChallenge(challenge) = response(&mut transport).await else {
        panic!("challenge");
    };
    assert_eq!(challenge.presence, WirePresence::Vacant as i32);
    assert!(challenge.challenge.is_empty());
    assert_eq!(challenge.consent, wire::Consent::Unavailable as i32);

    let mut f = Fixture::attended(Some(Consent::Allowed));
    f.prepare.consent = None;
    let mut transport = f.connect();
    let challenge = attended_challenge(&mut transport, &f).await;
    assert!(challenge.challenge.is_empty());
    assert_eq!(challenge.consent, wire::Consent::Unspecified as i32);
    assert!(f.broker.asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn consent_requests_need_a_bounded_answer_time_and_readable_names() {
    for change in [
        |c: &mut wire::ConsentRequest| c.timeout_seconds = 0,
        |c: &mut wire::ConsentRequest| c.timeout_seconds = 301,
        |c: &mut wire::ConsentRequest| c.operator_name = " \u{202e}\u{2066} ".into(),
        |c: &mut wire::ConsentRequest| c.customer_name = "x".repeat(201),
    ] {
        let mut f = Fixture::attended(Some(Consent::Allowed));
        change(f.prepare.consent.as_mut().unwrap());
        let mut transport = f.connect();
        assert_eq!(transport.prepare(f.prepare.clone()), Err(Error::Invalid));
    }
    assert_eq!(display_name("a\tb\n c").unwrap(), "a b c");
    assert_eq!(display_name(&"é".repeat(200)).unwrap().chars().count(), 200);
}

fn media_answer(body: Body) -> String {
    let Body::MediaAnswer(answer) = body else {
        panic!("media answer");
    };
    answer.sdp
}

#[tokio::test]
async fn media_offers_reach_the_backend_only_for_the_accepted_request() {
    let f = Fixture::new(0);
    let mut transport = f.connect();
    let offer = |request: &[u8]| wire::MediaOffer {
        request: request.to_vec(),
        sdp: "v=0".into(),
        relays: Vec::new(),
    };
    assert_eq!(
        transport.media(offer(&f.prepare.request)),
        Err(Error::Revoked)
    );
    let delivery = challenge(&f, &mut transport).await;
    transport.media(offer(&f.prepare.request)).unwrap();
    assert_eq!(
        media_answer(response(&mut transport).await),
        "",
        "not delivered yet"
    );
    transport.deliver(delivery).unwrap();
    assert!(matches!(
        response(&mut transport).await,
        Body::RemoteAck(wire::Ack { accepted: true, .. })
    ));
    transport.media(offer(&f.prepare.request)).unwrap();
    assert_eq!(media_answer(response(&mut transport).await), "answer:v=0");
    assert_eq!(
        transport.media(offer(Uuid::new_v4().as_bytes())),
        Err(Error::Target)
    );
    assert_eq!(
        transport.media(wire::MediaOffer {
            request: f.prepare.request.clone(),
            sdp: "x".repeat(wire::MAX_SDP + 1),
            relays: Vec::new(),
        }),
        Err(Error::Invalid)
    );
    // Relays from the server are bounded; an oversized list refuses the offer.
    let relay = |url: &str| wire::Relay {
        urls: vec![url.into()],
        username: "1:request".into(),
        credential: "secret".into(),
    };
    assert_eq!(
        transport.media(wire::MediaOffer {
            request: f.prepare.request.clone(),
            sdp: "v=0".into(),
            relays: vec![relay("turn:relay:3478"); 5],
        }),
        Err(Error::Invalid)
    );
    assert_eq!(
        transport.media(wire::MediaOffer {
            request: f.prepare.request.clone(),
            sdp: "v=0".into(),
            relays: vec![relay(
                "turn:relay
:3478"
            )],
        }),
        Err(Error::Invalid)
    );
    transport
        .media(wire::MediaOffer {
            request: f.prepare.request.clone(),
            sdp: "v=0".into(),
            relays: vec![relay("turn:relay:3478")],
        })
        .unwrap();
    assert_eq!(
        media_answer(response(&mut transport).await),
        "answer:v=0 via turn:relay:3478"
    );
    transport
        .revoke(wire::Revoke {
            request: f.prepare.request.clone(),
        })
        .unwrap();
    assert_eq!(
        transport.media(offer(&f.prepare.request)),
        Err(Error::Revoked)
    );
}

#[tokio::test]
async fn the_active_sessions_transfers_reach_the_server_for_its_request() {
    use crate::media::transfer::{Kind, Outcome};
    let f = Fixture::attended(Some(Consent::Allowed));
    let mut transport = f.connect();
    let challenge = attended_challenge(&mut transport, &f).await;
    transport.deliver(f.sign(challenge)).unwrap();
    assert!(matches!(
        response(&mut transport).await,
        Body::RemoteAck(wire::Ack { accepted: true, .. })
    ));
    f.broker.transfers.lock().unwrap().push(Report {
        kind: Kind::ClipboardToDevice,
        name: None,
        size: None,
        sha256: None,
        outcome: Outcome::Completed,
    });
    let Body::RemoteTransfer(report) = response(&mut transport).await else {
        panic!("transfer report");
    };
    assert_eq!(report.request, f.prepare.request);
    assert_eq!(report.kind, wire::TransferKind::ClipboardToDevice as i32);
    assert_eq!(report.outcome, wire::TransferOutcome::Completed as i32);
}

#[tokio::test]
async fn chat_goes_both_ways_only_for_the_active_request() {
    let f = Fixture::attended(Some(Consent::Allowed));
    let mut transport = f.connect();
    let challenge = attended_challenge(&mut transport, &f).await;
    transport.deliver(f.sign(challenge)).unwrap();
    assert!(matches!(
        response(&mut transport).await,
        Body::RemoteAck(wire::Ack { accepted: true, .. })
    ));
    let chat = |request: &[u8], text: &str| wire::Chat {
        request: request.to_vec(),
        text: text.into(),
    };
    assert_eq!(
        transport.chat(chat(Uuid::new_v4().as_bytes(), "más kérés")),
        Err(Error::Target)
    );
    assert_eq!(
        transport.chat(chat(&f.prepare.request, "a\u{7}")),
        Err(Error::Invalid)
    );
    transport
        .chat(chat(&f.prepare.request, "Mindjárt nézem."))
        .unwrap();
    f.broker.said.lock().unwrap().push("Köszönöm!".into());
    let Body::RemoteChat(said) = response(&mut transport).await else {
        panic!("the user's chat");
    };
    assert_eq!(said, chat(&f.prepare.request, "Köszönöm!"));
    assert_eq!(*f.broker.shown.lock().unwrap(), ["Mindjárt nézem."]);
}
