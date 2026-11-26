//! A software passkey authenticator for tests: ES256 keys and `none` attestation,
//! answering the options the server sends the way a browser and authenticator would.
//!
//! Like a hardware security key, it stores the user handle, and so can be found
//! by a discoverable sign-in, only when registration requires a resident key.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ciborium::Value as Cbor;
use p256::ecdsa::{Signature, SigningKey, signature::Signer as _};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

/// Flags in authenticator data.
const USER_PRESENT: u8 = 0x01;
const USER_VERIFIED: u8 = 0x04;
const ATTESTED_CREDENTIAL: u8 = 0x40;

/// One authenticator holding one credential.
pub struct SoftAuthenticator {
    origin: String,
    rp_id: String,
    key: SigningKey,
    /// The credential id; random per authenticator.
    pub credential_id: Vec<u8>,
    /// The signature counter, raised on every use. Tests may wind it back.
    pub counter: u32,
    /// The user handle the credential stores, when it is discoverable.
    pub user_handle: Option<Vec<u8>>,
}

impl SoftAuthenticator {
    /// A new authenticator used from `origin` (`scheme://host[:port]`).
    pub fn new(origin: &str) -> Self {
        let host = origin
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split(':').next())
            .expect("origin host");
        Self {
            origin: origin.to_owned(),
            rp_id: host.to_owned(),
            key: random_key(),
            credential_id: random_bytes::<16>().to_vec(),
            counter: 0,
            user_handle: None,
        }
    }

    /// Answers `navigator.credentials.create` for `options` (the server's
    /// `CreationChallengeResponse`).
    pub fn register(&mut self, options: &Value) -> Value {
        let public_key = &options["publicKey"];
        assert_eq!(public_key["rp"]["id"], json!(self.rp_id), "relying party");
        let resident_key = &public_key["authenticatorSelection"]["residentKey"];
        self.user_handle = (resident_key == "required")
            .then(|| decode(public_key["user"]["id"].as_str().expect("user id")));
        let client_data = self.client_data("webauthn.create", public_key);
        self.counter += 1;
        let mut auth_data = self.auth_data(USER_PRESENT | USER_VERIFIED | ATTESTED_CREDENTIAL);
        auth_data.extend_from_slice(&[0; 16]); // AAGUID
        let id_len = u16::try_from(self.credential_id.len()).expect("short id");
        auth_data.extend_from_slice(&id_len.to_be_bytes());
        auth_data.extend_from_slice(&self.credential_id);
        auth_data.extend_from_slice(&self.cose_key());
        let attestation = cbor(&Cbor::Map(vec![
            (Cbor::Text("fmt".into()), Cbor::Text("none".into())),
            (Cbor::Text("attStmt".into()), Cbor::Map(vec![])),
            (Cbor::Text("authData".into()), Cbor::Bytes(auth_data)),
        ]));
        json!({
            "id": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "rawId": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "type": "public-key",
            "response": {
                "attestationObject": URL_SAFE_NO_PAD.encode(attestation),
                "clientDataJSON": URL_SAFE_NO_PAD.encode(client_data),
            },
        })
    }

    /// Answers `navigator.credentials.get` for `options` (the server's
    /// `RequestChallengeResponse`), as a discoverable credential does.
    pub fn sign_in(&mut self, options: &Value) -> Value {
        let public_key = &options["publicKey"];
        let client_data = self.client_data("webauthn.get", public_key);
        self.counter += 1;
        let auth_data = self.auth_data(USER_PRESENT | USER_VERIFIED);
        let mut signed = auth_data.clone();
        signed.extend_from_slice(&Sha256::digest(&client_data));
        let signature: Signature = self.key.sign(&signed);
        json!({
            "id": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "rawId": URL_SAFE_NO_PAD.encode(&self.credential_id),
            "type": "public-key",
            "response": {
                "authenticatorData": URL_SAFE_NO_PAD.encode(auth_data),
                "clientDataJSON": URL_SAFE_NO_PAD.encode(client_data),
                "signature": URL_SAFE_NO_PAD.encode(signature.to_der().as_bytes()),
                "userHandle": self.user_handle.as_ref().map(|handle| URL_SAFE_NO_PAD.encode(handle)),
            },
        })
    }

    fn client_data(&self, kind: &str, public_key: &Value) -> Vec<u8> {
        let challenge = public_key["challenge"].as_str().expect("challenge");
        serde_json::to_vec(&json!({
            "type": kind,
            "challenge": challenge,
            "origin": self.origin,
            "crossOrigin": false,
        }))
        .expect("client data")
    }

    fn auth_data(&self, flags: u8) -> Vec<u8> {
        let mut data = Sha256::digest(self.rp_id.as_bytes()).to_vec();
        data.push(flags);
        data.extend_from_slice(&self.counter.to_be_bytes());
        data
    }

    fn cose_key(&self) -> Vec<u8> {
        let point = self.key.verifying_key().to_sec1_point(false);
        let int = |value: i64| Cbor::Integer(value.into());
        cbor(&Cbor::Map(vec![
            (int(1), int(2)),  // kty: EC2
            (int(3), int(-7)), // alg: ES256
            (int(-1), int(1)), // crv: P-256
            (int(-2), Cbor::Bytes(point.x().expect("x").to_vec())),
            (int(-3), Cbor::Bytes(point.y().expect("y").to_vec())),
        ]))
    }
}

fn cbor(value: &Cbor) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(value, &mut out).expect("cbor");
    out
}

fn decode(text: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(text).expect("base64url")
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).expect("random");
    bytes
}

fn random_key() -> SigningKey {
    loop {
        if let Ok(key) = SigningKey::from_bytes(&random_bytes::<32>().into()) {
            return key;
        }
    }
}
