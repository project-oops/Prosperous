//! The four-phase pairing handshake, following Sunshine's `nvhttp.cpp` so a client that pairs with
//! Sunshine pairs with the bridge.
//!
//! A commit-reveal on both sides over plain HTTP, keyed by an AES key derived from the PIN the
//! client displays: `getservercert` exchanges certificates once the PIN is entered,
//! `clientchallenge` has the server commit to a secret, `serverchallengeresp` has the client
//! commit and the server reveal, and `clientpairingsecret` has the client reveal. Each side then
//! checks the other's commitment and signature, and has pinned the other's certificate.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use crate::cert::{ClientCert, ServerCert};
use crate::crypto::{self, BLOCK};
use crate::error::{Error, Result};

/// How long phase one waits for the PIN to be entered before giving up.
const PIN_WAIT: Duration = Duration::from_secs(60);

/// One client's in-progress pairing, carried between phases.
struct Session {
    /// The AES key derived from salt and PIN in phase one.
    key: [u8; BLOCK],
    /// The client's certificate, captured in phase one for the phase-four checks.
    client: ClientCert,
    /// The server secret generated in phase two, revealed in phase three.
    server_secret: [u8; BLOCK],
    /// The server challenge generated in phase two, checked in phase four.
    server_challenge: [u8; BLOCK],
    /// The client's committed hash from phase three, verified in phase four.
    client_hash: Vec<u8>,
}

/// The pairing service: the server certificate, the PIN gate, and the in-progress sessions.
pub(crate) struct Pairing {
    /// The bridge's own certificate and signing key.
    cert: ServerCert,
    /// In-progress sessions, keyed by the client's `uniqueid`.
    sessions: Mutex<HashMap<String, Session>>,
    /// The PIN, once entered, and a condition to wake phase one when it is.
    pin: Mutex<Option<String>>,
    /// Notified when the PIN is submitted.
    pin_ready: Condvar,
    /// Clients that finished pairing: their id and certificate DER.
    paired: Mutex<HashMap<String, Vec<u8>>>,
}

impl Pairing {
    /// Build the pairing service around a server certificate.
    pub(crate) fn new(cert: ServerCert) -> Self {
        Self {
            cert,
            sessions: Mutex::new(HashMap::new()),
            pin: Mutex::new(None),
            pin_ready: Condvar::new(),
            paired: Mutex::new(HashMap::new()),
        }
    }

    /// Whether a client id has completed pairing.
    pub(crate) fn is_paired(&self, id: &str) -> bool {
        self.paired
            .lock()
            .is_ok_and(|paired| paired.contains_key(id))
    }

    /// Submit the PIN the client displays, waking phase one.
    ///
    /// One pending PIN is held, since a person pairs one client at a time.
    pub(crate) fn submit_pin(&self, pin: &str) {
        if let Ok(mut held) = self.pin.lock() {
            *held = Some(pin.to_owned());
            self.pin_ready.notify_all();
        }
    }

    /// Phase one: capture the client's cert, wait for the PIN, derive the key, return `plaincert`.
    ///
    /// # Errors
    ///
    /// If the salt or client certificate will not parse, or the PIN is not entered within
    /// [`PIN_WAIT`].
    pub(crate) fn get_server_cert(
        &self,
        id: &str,
        salt_hex: &str,
        client_cert_hex: &str,
    ) -> Result<String> {
        let salt = decode_block(salt_hex)?;
        let client = ClientCert::from_hex_pem(client_cert_hex)?;
        let pin = self.wait_for_pin()?;
        let key = crypto::pairing_key(&salt, &pin);
        let mut sessions = self.lock_sessions()?;
        sessions.insert(
            id.to_owned(),
            Session {
                key,
                client,
                server_secret: [0; BLOCK],
                server_challenge: [0; BLOCK],
                client_hash: Vec::new(),
            },
        );
        Ok(paired_root(&format!(
            "<plaincert>{}</plaincert>",
            self.cert.pem_hex()
        )))
    }

    /// Phase two: answer the client's challenge with a hash and a server challenge.
    ///
    /// # Errors
    ///
    /// If the session is unknown or the challenge is not a whole cipher block.
    pub(crate) fn client_challenge(&self, id: &str, challenge_hex: &str) -> Result<String> {
        let challenge = hex::decode(challenge_hex)?;
        let server_secret = rand::random::<[u8; BLOCK]>();
        let server_challenge = rand::random::<[u8; BLOCK]>();
        let mut sessions = self.lock_sessions()?;
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| Error::Pairing("challenge for an unknown session".into()))?;

        let decrypted = crypto::aes_ecb_decrypt(&session.key, &challenge)?;
        // hash( decrypted-challenge || server-cert-signature || server-secret )
        let mut material = decrypted;
        material.extend_from_slice(&self.cert.signature);
        material.extend_from_slice(&server_secret);
        let hash = crypto::sha256(&material);

        let mut plaintext = hash.to_vec();
        plaintext.extend_from_slice(&server_challenge);
        let response = crypto::aes_ecb_encrypt(&session.key, &plaintext)?;

        session.server_secret = server_secret;
        session.server_challenge = server_challenge;
        Ok(paired_root(&format!(
            "<challengeresponse>{}</challengeresponse>",
            hex::encode(response)
        )))
    }

    /// Phase three: store the client's committed hash and reveal the signed server secret.
    ///
    /// # Errors
    ///
    /// If the session is unknown or the response is not a whole cipher block.
    pub(crate) fn server_challenge_resp(&self, id: &str, resp_hex: &str) -> Result<String> {
        let resp = hex::decode(resp_hex)?;
        let mut sessions = self.lock_sessions()?;
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| Error::Pairing("challenge response for an unknown session".into()))?;

        session.client_hash = crypto::aes_ecb_decrypt(&session.key, &resp)?;

        let signature = crypto::sign(&self.cert.private, &session.server_secret);
        let mut pairing_secret = session.server_secret.to_vec();
        pairing_secret.extend_from_slice(&signature);
        Ok(paired_root(&format!(
            "<pairingsecret>{}</pairingsecret>",
            hex::encode(pairing_secret)
        )))
    }

    /// Phase four: check the commitment and the client's signature, and pair if both hold.
    ///
    /// # Errors
    ///
    /// If the session is unknown or the secret is malformed. A commitment or signature that does
    /// not check out is a `paired=0` answer, not an error: a wrong PIN is not a fault.
    pub(crate) fn client_pairing_secret(&self, id: &str, secret_hex: &str) -> Result<String> {
        let blob = hex::decode(secret_hex)?;

        let sessions = self.lock_sessions()?;
        let session = sessions
            .get(id)
            .ok_or_else(|| Error::Pairing("pairing secret for an unknown session".into()))?;

        // Encrypted like the earlier phases, so decrypt before splitting.
        let secret = crypto::aes_ecb_decrypt(&session.key, &blob)?;
        if secret.len() < BLOCK {
            return Err(Error::Pairing(
                "pairing secret shorter than one block".into(),
            ));
        }
        let (client_secret, client_sign) = secret.split_at(BLOCK);

        // hash( server-challenge || client-cert-signature || client-secret ), compared to phase 3.
        let mut material = session.server_challenge.to_vec();
        material.extend_from_slice(&session.client.signature);
        material.extend_from_slice(client_secret);
        let hash = crypto::sha256(&material);

        let commitment_holds = hash.as_slice() == session.client_hash.as_slice();
        let signature_holds = crypto::verify(&session.client.public, client_secret, client_sign);

        if commitment_holds && signature_holds {
            let der = session.client.der.clone();
            drop(sessions);
            if let Ok(mut paired) = self.paired.lock() {
                paired.insert(id.to_owned(), der);
            }
            Ok(paired_root(""))
        } else {
            tracing::warn!(id, commitment_holds, signature_holds, "pairing refused");
            Ok(refused_root())
        }
    }

    /// Phase five, over HTTPS: the client confirms the pairing against the pinned server
    /// certificate. Nothing is computed; the answer is whether phase four paired this client.
    pub(crate) fn pair_challenge(&self, id: &str) -> String {
        if self.is_paired(id) {
            paired_root("")
        } else {
            refused_root()
        }
    }

    /// Block until the PIN is submitted, up to [`PIN_WAIT`].
    fn wait_for_pin(&self) -> Result<String> {
        let mut held = self
            .pin
            .lock()
            .map_err(|_| Error::Pairing("pin lock poisoned".into()))?;
        loop {
            if let Some(pin) = held.take() {
                return Ok(pin);
            }
            let (next, timeout) = self
                .pin_ready
                .wait_timeout(held, PIN_WAIT)
                .map_err(|_| Error::Pairing("pin wait poisoned".into()))?;
            held = next;
            if timeout.timed_out() && held.is_none() {
                return Err(Error::Pairing("no PIN was entered in time".into()));
            }
        }
    }

    /// Lock the session map, turning a poisoned lock into a pairing error.
    fn lock_sessions(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Session>>> {
        self.sessions
            .lock()
            .map_err(|_| Error::Pairing("session lock poisoned".into()))
    }
}

/// Decode a hex field that must be exactly one 16-byte block.
fn decode_block(hex_field: &str) -> Result<[u8; BLOCK]> {
    let bytes = hex::decode(hex_field)?;
    bytes
        .try_into()
        .map_err(|_| Error::Pairing("a field that had to be 16 bytes was not".into()))
}

/// A `paired=1` response wrapping `inner`.
fn paired_root(inner: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<root status_code=\"200\"><paired>1</paired>{inner}</root>"
    )
}

/// A `paired=0` response: the pairing did not complete.
fn refused_root() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<root status_code=\"200\"><paired>0</paired></root>"
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::Pairing;
    use crate::cert::{ClientCert, ServerCert};
    use crate::crypto::{self, BLOCK};

    /// Pull the hex text out of `<tag>...</tag>`.
    fn field<'a>(xml: &'a str, tag: &str) -> &'a str {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let start = xml.find(&open).expect("tag present") + open.len();
        let end = xml[start..].find(&close).expect("tag closed") + start;
        &xml[start..end]
    }

    /// Play a correct client through all four phases against `server`, using `pin`. Returns the
    /// phase-four response and whether the server considers the client paired.
    fn run_client(server: &Pairing, id: &str, pin: &str) -> (String, bool) {
        let client = ServerCert::generate().unwrap();
        let salt = rand::random::<[u8; BLOCK]>();
        let key = crypto::pairing_key(&salt, pin);

        // Phase 1. The PIN is submitted first, so phase one does not block.
        server.submit_pin(pin);
        let p1 = server
            .get_server_cert(id, &hex::encode(salt), &client.pem_hex())
            .unwrap();
        let server_pinned = ClientCert::from_hex_pem(field(&p1, "plaincert")).unwrap();

        // Phase 2: an encrypted challenge in, a hash and a server challenge back.
        let client_challenge = rand::random::<[u8; BLOCK]>();
        let p2 = server
            .client_challenge(
                id,
                &hex::encode(crypto::aes_ecb_encrypt(&key, &client_challenge).unwrap()),
            )
            .unwrap();
        let decrypted =
            crypto::aes_ecb_decrypt(&key, &hex::decode(field(&p2, "challengeresponse")).unwrap())
                .unwrap();
        let (server_hash, server_challenge) = decrypted.split_at(32);

        // Phase 3: the client's commitment in, the signed server secret back.
        let client_secret = rand::random::<[u8; BLOCK]>();
        let mut commit = server_challenge.to_vec();
        commit.extend_from_slice(&client.signature);
        commit.extend_from_slice(&client_secret);
        let client_hash = crypto::sha256(&commit);
        let p3 = server
            .server_challenge_resp(
                id,
                &hex::encode(crypto::aes_ecb_encrypt(&key, &client_hash).unwrap()),
            )
            .unwrap();
        let pairing_secret = hex::decode(field(&p3, "pairingsecret")).unwrap();
        let (server_secret, server_sign) = pairing_secret.split_at(BLOCK);

        // The client checks the phase-two hash and the server's signature against the pinned cert.
        let mut recomputed = client_challenge.to_vec();
        recomputed.extend_from_slice(&server_pinned.signature);
        recomputed.extend_from_slice(server_secret);
        let server_ok = crypto::sha256(&recomputed) == server_hash
            && crypto::verify(&server_pinned.public, server_secret, server_sign);

        // Phase 4: the client reveals and signs its secret.
        let client_sign = crypto::sign(&client.private, &client_secret);
        let mut reveal = client_secret.to_vec();
        reveal.extend_from_slice(&client_sign);
        let p4 = server
            .client_pairing_secret(
                id,
                &hex::encode(crypto::aes_ecb_encrypt(&key, &reveal).unwrap()),
            )
            .unwrap();

        assert!(server_ok, "the client could not verify the server");
        (p4, server.is_paired(id))
    }

    /// A client with the right PIN pairs, and phase five confirms it.
    #[test]
    fn a_correct_pin_pairs() {
        let server = Pairing::new(ServerCert::generate().unwrap());
        let (p4, paired) = run_client(&server, "client-a", "1234");
        assert!(p4.contains("<paired>1</paired>"), "{p4}");
        assert!(paired);
        assert!(
            server
                .pair_challenge("client-a")
                .contains("<paired>1</paired>")
        );
    }

    /// A wrong PIN ends as `paired=0`, not as an error.
    #[test]
    fn a_wrong_pin_is_refused_not_errored() {
        // The server is given 1234 and the client keys with 9999.
        let server = Pairing::new(ServerCert::generate().unwrap());
        let client = ServerCert::generate().unwrap();
        let salt = rand::random::<[u8; BLOCK]>();
        let server_key = crypto::pairing_key(&salt, "1234");
        let wrong_key = crypto::pairing_key(&salt, "9999");
        server.submit_pin("1234");
        server
            .get_server_cert("client-b", &hex::encode(salt), &client.pem_hex())
            .unwrap();
        let p2 = server
            .client_challenge(
                "client-b",
                &hex::encode(crypto::aes_ecb_encrypt(&wrong_key, &[0_u8; BLOCK]).unwrap()),
            )
            .unwrap();
        let decrypted = crypto::aes_ecb_decrypt(
            &wrong_key,
            &hex::decode(field(&p2, "challengeresponse")).unwrap(),
        )
        .unwrap();
        let (_hash, server_challenge) = decrypted.split_at(32);
        let client_secret = [1_u8; BLOCK];
        let mut commit = server_challenge.to_vec();
        commit.extend_from_slice(&client.signature);
        commit.extend_from_slice(&client_secret);
        server
            .server_challenge_resp(
                "client-b",
                &hex::encode(
                    crypto::aes_ecb_encrypt(&wrong_key, &crypto::sha256(&commit)).unwrap(),
                ),
            )
            .unwrap();
        let client_sign = crypto::sign(&client.private, &client_secret);
        let mut reveal = client_secret.to_vec();
        reveal.extend_from_slice(&client_sign);
        let p4 = server
            .client_pairing_secret(
                "client-b",
                &hex::encode(crypto::aes_ecb_encrypt(&wrong_key, &reveal).unwrap()),
            )
            .unwrap();
        let _ = server_key;
        assert!(p4.contains("<paired>0</paired>"), "{p4}");
        assert!(!server.is_paired("client-b"));
    }
}
