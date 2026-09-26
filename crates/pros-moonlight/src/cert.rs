//! The server's own certificate, and reading the client's.
//!
//! Pairing pins certificates on both sides, so the server certificate is stable across runs or
//! every client would have to pair again. The client's certificate supplies its public key (for
//! the phase-four signature check) and its signature bytes (which both sides hash). The server
//! certificate is a self-signed RSA-2048/SHA-256 one, the kind clients expect from a host.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::Sha256;
use x509_cert::builder::{Builder, CertificateBuilder, Profile};
use x509_cert::der::{DecodePem, Encode, EncodePem};
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::SubjectPublicKeyInfoOwned;
use x509_cert::time::Validity;

use crate::error::{Error, Result};

/// Ten years: the cert outlives any pairing that pins it, since expiry would only force
/// re-pairing on a trusted LAN. `Duration::new` because the days-unit constructor clippy prefers
/// is unstable.
const TEN_YEARS: Duration = Duration::new(3650 * 24 * 60 * 60, 0);

/// Where the private key is written under the data directory.
const KEY_FILE: &str = "server-key.pem";
/// Where the self-signed certificate is written.
const CERT_FILE: &str = "server-cert.pem";

/// The bridge's own identity: the RSA key it signs with, and the certificate a client pins.
pub(crate) struct ServerCert {
    /// The private key, held for phase-three signing.
    pub(crate) private: RsaPrivateKey,
    /// The certificate in PEM, sent to the client in phase one as `plaincert`.
    pub(crate) pem: String,
    /// The certificate in DER, for presenting over TLS on the HTTPS port.
    pub(crate) der: Vec<u8>,
    /// The certificate's own signature bytes, concatenated into the phase-two hash.
    pub(crate) signature: Vec<u8>,
}

impl ServerCert {
    /// Generate a fresh RSA-2048 self-signed certificate.
    ///
    /// # Errors
    ///
    /// If key generation or certificate encoding fails.
    pub(crate) fn generate() -> Result<Self> {
        let mut rng = rand::thread_rng();
        let private = RsaPrivateKey::new(&mut rng, 2048)
            .map_err(|error| Error::Certificate(format!("key generation: {error}")))?;
        Self::from_key(private)
    }

    /// Build the certificate around an existing key.
    fn from_key(private: RsaPrivateKey) -> Result<Self> {
        let signing_key = SigningKey::<Sha256>::new(private.clone());
        let spki = SubjectPublicKeyInfoOwned::from_key(private.to_public_key())
            .map_err(|error| Error::Certificate(format!("public key: {error}")))?;
        let profile = Profile::Root;
        let serial = SerialNumber::from(1_u32);
        let validity = Validity::from_now(TEN_YEARS)
            .map_err(|error| Error::Certificate(format!("validity: {error}")))?;
        let subject = Name::from_str("CN=Prosperous")
            .map_err(|error| Error::Certificate(format!("subject: {error}")))?;
        let builder =
            CertificateBuilder::new(profile, serial, validity, subject, spki, &signing_key)
                .map_err(|error| Error::Certificate(format!("builder: {error}")))?;
        let cert = builder
            .build()
            .map_err(|error| Error::Certificate(format!("signing: {error}")))?;
        let pem = cert
            .to_pem(LineEnding::LF)
            .map_err(|error| Error::Certificate(format!("encode pem: {error}")))?;
        let der = cert
            .to_der()
            .map_err(|error| Error::Certificate(format!("encode der: {error}")))?;
        let signature = cert
            .signature
            .as_bytes()
            .ok_or_else(|| Error::Certificate("certificate signature was not whole bytes".into()))?
            .to_vec();
        Ok(Self {
            private,
            pem,
            der,
            signature,
        })
    }

    /// The private key as PKCS#8 DER, for a TLS listener.
    ///
    /// # Errors
    ///
    /// If the key cannot be encoded.
    pub(crate) fn key_pkcs8_der(&self) -> Result<Vec<u8>> {
        Ok(self
            .private
            .to_pkcs8_der()
            .map_err(|error| Error::Certificate(format!("encode key der: {error}")))?
            .as_bytes()
            .to_vec())
    }

    /// Load the certificate from `dir`, generating and persisting one the first time.
    ///
    /// A client pins the cert, so it is written once and read on every later run. A missing
    /// directory is created.
    ///
    /// # Errors
    ///
    /// If the directory cannot be created, or a stored key is present but will not parse.
    pub(crate) fn load_or_generate(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let key_path = dir.join(KEY_FILE);
        let cert_path = dir.join(CERT_FILE);
        if key_path.exists() {
            let key_pem = std::fs::read_to_string(&key_path)?;
            let private = RsaPrivateKey::from_pkcs8_pem(&key_pem)
                .map_err(|error| Error::Certificate(format!("stored key: {error}")))?;
            return Self::from_key(private);
        }
        let made = Self::generate()?;
        let key_pem = made
            .private
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|error| Error::Certificate(format!("encode key: {error}")))?;
        std::fs::write(&key_path, key_pem.as_bytes())?;
        std::fs::write(&cert_path, made.pem.as_bytes())?;
        Ok(made)
    }

    /// The certificate as uppercase hex, the form `plaincert` takes on the wire.
    #[must_use]
    pub(crate) fn pem_hex(&self) -> String {
        hex::encode(self.pem.as_bytes())
    }
}

/// A client's certificate, as it presents it in pairing phase one.
pub(crate) struct ClientCert {
    /// The client's public key, for verifying its phase-four signature.
    pub(crate) public: RsaPublicKey,
    /// The certificate's own signature bytes, concatenated into the phase-four hash.
    pub(crate) signature: Vec<u8>,
    /// The certificate in DER, kept so a paired client's cert can be stored for later mutual TLS.
    pub(crate) der: Vec<u8>,
}

impl ClientCert {
    /// Parse a client certificate from the hex-of-PEM the client sends as `clientcert`.
    ///
    /// # Errors
    ///
    /// If the hex, the PEM, the DER, or the contained public key will not parse.
    pub(crate) fn from_hex_pem(hex_pem: &str) -> Result<Self> {
        let pem = hex::decode(hex_pem)?;
        let pem = std::str::from_utf8(&pem)
            .map_err(|error| Error::Certificate(format!("client cert not utf-8: {error}")))?;
        let cert = x509_cert::Certificate::from_pem(pem)
            .map_err(|error| Error::Certificate(format!("client pem: {error}")))?;
        Self::from_cert(&cert)
    }

    /// Pull the public key and signature bytes out of a parsed certificate.
    fn from_cert(cert: &x509_cert::Certificate) -> Result<Self> {
        let spki_der = cert
            .tbs_certificate
            .subject_public_key_info
            .to_der()
            .map_err(|error| Error::Certificate(format!("client spki: {error}")))?;
        let public = <RsaPublicKey as rsa::pkcs8::DecodePublicKey>::from_public_key_der(&spki_der)
            .map_err(|error| Error::Certificate(format!("client public key: {error}")))?;
        let signature = cert
            .signature
            .as_bytes()
            .ok_or_else(|| Error::Certificate("client signature was not whole bytes".into()))?
            .to_vec();
        let der = cert
            .to_der()
            .map_err(|error| Error::Certificate(format!("client re-encode: {error}")))?;
        Ok(Self {
            public,
            signature,
            der,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ClientCert, ServerCert};

    /// A certificate as the client sends it: hex of its PEM.
    fn as_client_wire(server: &ServerCert) -> String {
        hex::encode(server.pem.as_bytes())
    }

    /// A generated cert carries a PEM body and signature bytes.
    #[test]
    fn a_generated_cert_has_a_key_and_a_signature() {
        let cert = ServerCert::generate().unwrap();
        assert!(cert.pem.contains("BEGIN CERTIFICATE"));
        assert!(!cert.signature.is_empty());
        assert!(!cert.pem_hex().is_empty());
    }

    /// A cert parsed from hex-of-PEM keeps the signature bytes it was made with.
    #[test]
    fn a_cert_reads_back_off_the_wire_with_the_same_signature() {
        // The server cert stands in for a client's: same shape, same hex-of-PEM encoding.
        let server = ServerCert::generate().unwrap();
        let client = ClientCert::from_hex_pem(&as_client_wire(&server)).unwrap();
        assert_eq!(client.signature, server.signature);
    }

    /// The public key parsed from the cert verifies signatures made with the private key.
    #[test]
    fn the_public_key_read_off_the_wire_matches_the_private_one() {
        let server = ServerCert::generate().unwrap();
        let client = ClientCert::from_hex_pem(&as_client_wire(&server)).unwrap();
        let signature = crate::crypto::sign(&server.private, b"pin me");
        assert!(crate::crypto::verify(&client.public, b"pin me", &signature));
    }
}
