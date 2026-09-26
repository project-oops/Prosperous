//! The pairing primitives: SHA-256 over concatenations, AES-128-ECB with no padding (every input
//! is whole blocks), and RSA PKCS#1 v1.5 signatures over SHA-256. They follow the server side of
//! Sunshine's `nvhttp.cpp`, so a client that pairs with Sunshine pairs with this.

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
/// `salt` is the 16 random bytes the client sent; `pin` is the digits the user typed, as ASCII.
/// Clients of generation 7 and up use SHA-256, matching the `appversion` the bridge reports.
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
/// [`Error::NotBlockAligned`] if `data` is not a multiple of [`BLOCK`].
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
/// [`Error::NotBlockAligned`] if `data` is not a multiple of [`BLOCK`].
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
/// Phase four refuses the pairing when this is false: the client has not proved it holds the
/// private key for the certificate it presented.
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

    /// ECB decrypt inverts ECB encrypt.
    #[test]
    fn a_block_round_trips_through_ecb() {
        let key = [7_u8; BLOCK];
        let clear = b"sixteen bytes...";
        let cipher = aes_ecb_encrypt(&key, clear).unwrap();
        assert_ne!(&cipher, clear);
        assert_eq!(aes_ecb_decrypt(&key, &cipher).unwrap(), clear);
    }

    /// Input that is not whole blocks is an error, not padded.
    #[test]
    fn ecb_refuses_a_partial_block() {
        let key = [0_u8; BLOCK];
        assert!(aes_ecb_encrypt(&key, b"not a block").is_err());
    }

    /// The pairing key is the first 16 bytes of `SHA-256(salt || pin)`.
    #[test]
    fn the_pairing_key_is_the_first_sixteen_bytes_of_the_salted_hash() {
        let salt = [0_u8; BLOCK];
        let key = pairing_key(&salt, "0000");
        let expected = &sha256(&[salt.as_slice(), b"0000"].concat())[..BLOCK];
        assert_eq!(&key, expected);
    }

    /// A signature verifies for its own message and fails for another.
    #[test]
    fn a_signature_verifies_and_a_tampered_one_does_not() {
        let mut rng = rand::thread_rng();
        let private = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public = private.to_public_key();
        let message = b"the server secret";
        let signature = sign(&private, message);
        assert!(verify(&public, message, &signature));
        assert!(!verify(&public, b"a different message", &signature));
    }
}
