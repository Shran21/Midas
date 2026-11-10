use super::*;

fn claims() -> Claims {
    Claims {
        id: Uuid::new_v4(),
        challenge: Uuid::new_v4(),
        operator: Uuid::new_v4(),
        target: Target {
            connection: Connection {
                tenant: Uuid::new_v4(),
                device: Uuid::new_v4(),
                id: Uuid::new_v4(),
            },
            session: 7,
            generation: Uuid::new_v4(),
        },
        rights: Rights {
            mode: Mode::Normal,
            uac: true,
        },
        issued_at: 100,
        expires_at: 130,
    }
}

#[test]
fn signatures_bind_every_claim_and_use_a_distinct_domain() {
    let key = SigningKey::generate().unwrap();
    let c = claims();
    let grant = key.sign(&c).unwrap();
    assert_eq!(format!("{grant:?}"), "SignedGrant { .. }");
    assert_eq!(verify(&key.public_key(), &grant).unwrap().claims(), &c);
    assert_eq!(
        SignedGrant::decode_checked(&grant.encode_to_vec()).unwrap(),
        grant
    );
    let other = SigningKey::generate().unwrap();
    assert!(matches!(
        verify(&other.public_key(), &grant),
        Err(Error::Signature)
    ));
    for index in 0..grant.payload.len() {
        let mut changed = grant.clone();
        changed.payload[index] ^= 1;
        assert!(verify(&key.public_key(), &changed).is_err());
    }
    for index in 0..grant.signature.len() {
        let mut changed = grant.clone();
        changed.signature[index] ^= 1;
        assert!(verify(&key.public_key(), &changed).is_err());
    }
    let mut wrong_domain = grant.clone();
    wrong_domain.signature = key.0.sign(&grant.payload).as_ref().to_vec();
    assert!(matches!(
        verify(&key.public_key(), &wrong_domain),
        Err(Error::Signature)
    ));
}

#[test]
fn malformed_noncanonical_and_future_contracts_fail_closed() {
    let key = SigningKey::generate().unwrap();
    let mut cases = Vec::new();
    let mut future = Wire::from(&claims());
    future.version = 2;
    cases.push(future.encode_to_vec());
    let mut mode = Wire::from(&claims());
    mode.mode = 3;
    cases.push(mode.encode_to_vec());
    let mut id = Wire::from(&claims());
    id.device = vec![1; 15];
    cases.push(id.encode_to_vec());
    let mut nil = Wire::from(&claims());
    nil.id = vec![0; 16];
    cases.push(nil.encode_to_vec());
    let mut unknown = Wire::from(&claims()).encode_to_vec();
    unknown.extend_from_slice(&[112, 1]);
    cases.push(unknown);
    let mut duplicate = Wire::from(&claims()).encode_to_vec();
    duplicate.extend_from_slice(&[8, 1]);
    cases.push(duplicate);
    let mut long = Wire::from(&claims());
    long.expires_at += 1;
    cases.push(long.encode_to_vec());
    for payload in cases {
        let signature = key.0.sign(&message(&payload)).as_ref().to_vec();
        assert!(matches!(
            verify(&key.public_key(), &SignedGrant { payload, signature }),
            Err(Error::Invalid)
        ));
    }
    let grant = key.sign(&claims()).unwrap();
    let mut bytes = grant.encode_to_vec();
    bytes.extend_from_slice(&[24, 1]);
    assert!(SignedGrant::decode_checked(&bytes).is_err());
    assert!(SignedGrant::decode_checked(&vec![0; MAX_GRANT_BYTES + 1]).is_err());
    assert!(
        verify(
            &key.public_key(),
            &SignedGrant {
                payload: vec![1; MAX_PAYLOAD_BYTES + 1],
                signature: vec![0; 64]
            }
        )
        .is_err()
    );
}

#[test]
fn signing_rejects_unbounded_or_ambiguous_authority() {
    let key = SigningKey::generate().unwrap();
    let changes: [fn(&mut Claims); 13] = [
        |c| c.id = Uuid::nil(),
        |c| c.challenge = Uuid::nil(),
        |c| c.operator = Uuid::nil(),
        |c| c.target.connection.tenant = Uuid::nil(),
        |c| c.target.connection.device = Uuid::nil(),
        |c| c.target.connection.id = Uuid::nil(),
        |c| c.target.generation = Uuid::nil(),
        |c| c.target.session = 0,
        |c| c.target.session = u32::MAX,
        |c| c.expires_at = c.issued_at,
        |c| c.expires_at = c.issued_at + 31,
        |c| c.issued_at = -1,
        |c| {
            c.issued_at = i64::MIN;
            c.expires_at = i64::MAX;
        },
    ];
    for change in changes {
        let mut c = claims();
        change(&mut c);
        assert!(key.sign(&c).is_err());
    }
}
