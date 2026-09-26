//! The TLS configuration for the HTTPS port.
//!
//! A client verifies the HTTPS server against the certificate it pinned as `plaincert` during
//! pairing, so this serves `ServerCert`'s certificate and key and no other. Client certificates
//! are not requested: on a trusted LAN the pairing has already identified the client.

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
