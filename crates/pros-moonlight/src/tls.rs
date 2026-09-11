//! The TLS configuration for the HTTPS port.
//!
//! The one thing that has to be true here: the certificate presented is **the same one the client
//! pinned during pairing**. A Moonlight client verifies the HTTPS server against the cert it saw as
//! `plaincert`, so this serves `ServerCert`'s own certificate and key and no other.
//!
//! Client certificates are not required. On a trusted LAN the pairing already established who the
//! client is, and requesting its certificate here buys nothing the pairing did not; a later change
//! can pin the paired client certs if the deployment ever stops being a trusted LAN.

use std::sync::Arc;

use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use crate::cert::ServerCert;
use crate::error::{Error, Result};

/// Build a TLS server configuration that presents `cert`.
///
/// # Errors
///
/// If the key cannot be encoded or the certificate and key are not accepted together.
pub(crate) fn server_config(cert: &ServerCert) -> Result<Arc<ServerConfig>> {
    let certificate = CertificateDer::from(cert.der.clone());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.key_pkcs8_der()?));
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| Error::Certificate(format!("tls versions: {error}")))?
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)
        .map_err(|error| Error::Certificate(format!("tls cert: {error}")))?;
    Ok(Arc::new(config))
}
