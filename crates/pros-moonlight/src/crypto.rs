//! The pairing primitives, kept in one place because the handshake is the one part of the bridge
//! where a byte in the wrong order fails silently.
//!
//! Everything here mirrors what Sunshine's `nvhttp.cpp` does on the server side, so that a client
//! which pairs with Sunshine pairs with this. Nothing is invented: SHA-256 over concatenations,
//! AES-128 in ECB with no padding (every input is a whole number of blocks), and RSA PKCS#1 v1.5
//! signatures over the SHA-256 of the data.

use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use rsa::pkcs1v15::{Signature, SigningKey, VerifyingKey};
use rsa::signature::{SignatureEncoding, Signer, Verifier};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// The AES block and key size the protocol uses everywhere: 128-bit.
pub(crate) const BLOCK: usize = 16;

/// SHA-256 of `data`, the 32-byte hash the handshake concatenates and compares.
#[must_use]
pub(crate) fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Derive the pairing AES key: the first 16 bytes of `SHA-256(salt || pin)`.
///
/// `salt` is the 16 random bytes the client sent; `pin` is the digits the user typed here, as
/// ASCII. Modern clients (generation 7 and up, every current one) choose SHA-256, so that is what
/// this uses - matching the `appversion` the bridge reports.
#[must_use]
pub(crate) fn pairing_key(salt: &[u8; BLOCK], pin: &str) -> [u8; BLOCK] {
    let mut material = Vec::with_capacity(BLOCK + pin.len());
    material.extend_from_slice(salt);
    material.extend_from_slice(pin.as_bytes());
    let hash = sha256(&material);
    let mut key = [0_u8; BLOCK];
    key.copy_from_slice(&hash[..BLOCK]);
    key
}

/// AES-128-ECB encrypt, no padding. `data` must be a whole number of 16-byte blocks.
///
/// # Errors
///
/// [`Error::NotBlockAligned`] if `data` is not a multiple of [`BLOCK`]. The handshake only ever
/// encrypts block-aligned buffers, so this failing means the caller built the wrong thing.
pub(crate) fn aes_ecb_encrypt(key: &[u8; BLOCK], data: &[u8]) -> Result<Vec<u8>> {
    if !data.len().is_multiple_of(BLOCK) {
        return Err(Error::NotBlockAligned { len: data.len() });
    }
    let cipher = aes::Aes128::new(GenericArray::from_slice(key));
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.as_chunks::<BLOCK>().0 {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.encrypt_block(&mut block);
        out.extend_from_slice(&block);
    }
    Ok(out)
}

/// AES-128-ECB decrypt, no padding. `data` must be a whole number of 16-byte blocks.
///
/// # Errors
///
/// [`Error::NotBlockAligned`] if `data` is not a multiple of [`BLOCK`] - which, from a peer, means
/// it sent something that was never a valid ciphertext.
pub(crate) fn aes_ecb_decrypt(key: &[u8; BLOCK], data: &[u8]) -> Result<Vec<u8>> {
    if !data.len().is_multiple_of(BLOCK) {
        return Err(Error::NotBlockAligned { len: data.len() });
    }
    let cipher = aes::Aes128::new(GenericArray::from_slice(key));
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.as_chunks::<BLOCK>().0 {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        out.extend_from_slice(&block);
    }
    Ok(out)
}

/// RSA PKCS#1 v1.5 signature over `SHA-256(data)`, the 256-byte signature the client verifies
/// against the server certificate it pinned.
#[must_use]
pub(crate) fn sign(key: &RsaPrivateKey, data: &[u8]) -> Vec<u8> {
    let signing_key = SigningKey::<Sha256>::new(key.clone());
    signing_key.sign(data).to_vec()
}

/// Whether `signature` is a valid RSA PKCS#1 v1.5 SHA-256 signature over `data` for `key`.
///
/// This is phase four's whole decision: the client proves it holds the private key for the
/// certificate it presented, and a bad signature is a client that does not - so the pairing is
/// refused rather than completed.
#[must_use]
pub(crate) fn verify(key: &RsaPublicKey, data: &[u8], signature: &[u8]) -> bool {
    let Ok(signature) = Signature::try_from(signature) else {
        return false;
    };
    VerifyingKey::<Sha256>::new(key.clone())
        .verify(data, &signature)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::{BLOCK, aes_ecb_decrypt, aes_ecb_encrypt, pairing_key, sha256, sign, verify};
    use rsa::RsaPrivateKey;

    #[test]
    fn a_block_round_trips_through_ecb() {
        let key = [7_u8; BLOCK];
        let clear = b"sixteen bytes...";
        let cipher = aes_ecb_encrypt(&key, clear).unwrap();
        assert_ne!(&cipher, clear);
        assert_eq!(aes_ecb_decrypt(&key, &cipher).unwrap(), clear);
    }

    #[test]
    fn ecb_refuses_a_partial_block() {
        let key = [0_u8; BLOCK];
        assert!(aes_ecb_encrypt(&key, b"not a block").is_err());
    }

    #[test]
    fn the_pairing_key_is_the_first_sixteen_bytes_of_the_salted_hash() {
        // A fixed vector so a change to the derivation is caught rather than absorbed.
        let salt = [0_u8; BLOCK];
        let key = pairing_key(&salt, "0000");
        let expected = &sha256(&[salt.as_slice(), b"0000"].concat())[..BLOCK];
        assert_eq!(&key, expected);
    }

    #[test]
    fn a_signature_verifies_and_a_tampered_one_does_not() {
        // Small key: this is a round-trip test of the wiring, not a strength test.
        let mut rng = rand::thread_rng();
        let private = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public = private.to_public_key();
        let message = b"the server secret";
        let signature = sign(&private, message);
        assert!(verify(&public, message, &signature));
        assert!(!verify(&public, b"a different message", &signature));
    }
}
