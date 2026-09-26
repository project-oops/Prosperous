//! Running the bridge: advertise over mDNS, then serve HTTP, HTTPS and RTSP until stopped.
//!
//! HTTP and HTTPS share one router: the pairing state, not the socket, decides what a client is
//! allowed.

use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;
use std::sync::Arc;
use std::thread;

use crate::apps::Apps;
use crate::cert::ServerCert;
use crate::discovery;
use crate::error::Result;
use crate::host::{HTTP_PORT, HTTPS_PORT, Host, RTSP_PORT};
use crate::http::{Bridge, serve_one};
use crate::pairing::Pairing;
use crate::session::Sessions;
use crate::tls;

/// Run the bridge: load or make the certificate under `data_dir`, then serve until stopped.
///
/// The crate's entry point. `target` is the address serving Porthole's 9805/9806 (a real target
/// or the fake one); `data_dir` holds the certificate.
///
/// # Errors
///
/// If the certificate cannot be prepared, a port cannot be bound, or TLS cannot be configured.
pub fn run(
    hostname: String,
    local_ip: Ipv4Addr,
    apps: Apps,
    target: String,
    data_dir: &Path,
) -> Result<()> {
    let cert = ServerCert::load_or_generate(data_dir)?;
    serve(Host::new(hostname, local_ip), apps, cert, target)
}

/// Serve the bridge until the process is stopped.
///
/// Binds the HTTP, HTTPS and RTSP ports, advertises over mDNS, and handles connections on each,
/// one per thread. Blocks.
fn serve(host: Host, apps: Apps, cert: ServerCert, target: String) -> Result<()> {
    let tls_config = tls::server_config(&cert)?;
    let bridge = Bridge {
        host,
        pairing: Pairing::new(cert),
        apps,
        sessions: Sessions::new(target),
    };

    let http = TcpListener::bind(("0.0.0.0", HTTP_PORT))?;
    let https = TcpListener::bind(("0.0.0.0", HTTPS_PORT))?;
    let rtsp = TcpListener::bind(("0.0.0.0", RTSP_PORT))?;
    let advertisement = discovery::advertise(&bridge.host);
    if let Err(error) = &advertisement {
        tracing::warn!(%error, "mDNS advertising failed; the client can still be given the address");
    }
    tracing::info!(
        http = HTTP_PORT,
        https = HTTPS_PORT,
        rtsp = RTSP_PORT,
        "bridge serving"
    );

    thread::scope(|scope| {
        let bridge = &bridge;
        scope.spawn(move || {
            for stream in http.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let peer = peer_ip(&stream);
                        scope.spawn(move || {
                            if let Err(error) = serve_one(&mut stream, bridge, peer) {
                                tracing::debug!(%error, "http connection ended");
                            }
                        });
                    }
                    Err(error) => tracing::debug!(%error, "http accept failed"),
                }
            }
        });
        scope.spawn(move || {
            for stream in rtsp.incoming() {
                match stream {
                    Ok(mut stream) => {
                        scope.spawn(move || bridge.sessions.serve_rtsp(&mut stream));
                    }
                    Err(error) => tracing::debug!(%error, "rtsp accept failed"),
                }
            }
        });
        // The TLS acceptor runs on this thread.
        for stream in https.incoming() {
            match stream {
                Ok(mut stream) => {
                    let peer = peer_ip(&stream);
                    let config = Arc::clone(&tls_config);
                    scope.spawn(move || match rustls::ServerConnection::new(config) {
                        Ok(mut connection) => {
                            let mut secured = rustls::Stream::new(&mut connection, &mut stream);
                            if let Err(error) = serve_one(&mut secured, bridge, peer) {
                                tracing::debug!(%error, "https connection ended");
                            }
                        }
                        Err(error) => tracing::debug!(%error, "tls setup failed"),
                    });
                }
                Err(error) => tracing::debug!(%error, "https accept failed"),
            }
        }
    });
    drop(advertisement);
    Ok(())
}

/// The peer's IP, or the unspecified address if the socket cannot report it. Only a launch uses
/// the peer, and a launch always has one.
fn peer_ip(stream: &std::net::TcpStream) -> std::net::IpAddr {
    stream
        .peer_addr()
        .map_or(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), |addr| {
            addr.ip()
        })
}

#[cfg(test)]
mod tests {
    use crate::apps::Apps;
    use crate::cert::{ClientCert, ServerCert};
    use crate::crypto::{self, BLOCK};
    use crate::host::Host;
    use crate::http::{Bridge, serve_one};
    use crate::pairing::Pairing;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener, TcpStream};
    use std::sync::Arc;
    use std::thread;

    /// One HTTP GET against `addr`, returning the response body (everything after the blank line).
    fn get(addr: std::net::SocketAddr, target: &str) -> String {
        let mut stream = TcpStream::connect(addr).unwrap();
        write!(
            stream,
            "GET {target} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
            .split_once("\r\n\r\n")
            .map_or(response.clone(), |(_head, body)| body.to_owned())
    }

    /// Pull the hex text out of `<tag>...</tag>`.
    fn field<'a>(xml: &'a str, tag: &str) -> &'a str {
        let start = xml.find(&format!("<{tag}>")).unwrap() + tag.len() + 2;
        let end = xml[start..].find(&format!("</{tag}>")).unwrap() + start;
        &xml[start..end]
    }

    /// A client's GameStream requests over real sockets pair it through the HTTP router.
    #[test]
    fn a_client_discovers_and_pairs_over_http() {
        let bridge = Arc::new(Bridge {
            host: Host::new("prosperous-itest".into(), Ipv4Addr::LOCALHOST),
            pairing: Pairing::new(ServerCert::generate().unwrap()),
            apps: Apps::from_titles(["ps5 on the bench"]),
            sessions: crate::session::Sessions::new("127.0.0.1".to_owned()),
        });
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let serving = Arc::clone(&bridge);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let bridge = Arc::clone(&serving);
                thread::spawn(move || {
                    let _ = serve_one(&mut stream, &bridge, Ipv4Addr::LOCALHOST.into());
                });
            }
        });

        let info = get(addr, "/serverinfo?uniqueid=itest");
        assert!(info.contains("<PairStatus>0</PairStatus>"), "{info}");
        assert!(info.contains("<hostname>prosperous-itest</hostname>"));

        let client = ServerCert::generate().unwrap();
        let salt = rand::random::<[u8; BLOCK]>();
        let key = crypto::pairing_key(&salt, "4321");

        // The PIN goes in before phase one, which blocks until it arrives.
        get(addr, "/pin?pin=4321");

        // Phase 1: getservercert.
        let p1 = get(
            addr,
            &format!(
                "/pair?uniqueid=itest&devicename=test&phrase=getservercert&salt={}&clientcert={}",
                hex::encode(salt),
                hex::encode(client.pem.as_bytes()),
            ),
        );
        let server_pinned = ClientCert::from_hex_pem(field(&p1, "plaincert")).unwrap();

        // Phase 2: clientchallenge.
        let client_challenge = rand::random::<[u8; BLOCK]>();
        let p2 = get(
            addr,
            &format!(
                "/pair?uniqueid=itest&clientchallenge={}",
                hex::encode(crypto::aes_ecb_encrypt(&key, &client_challenge).unwrap()),
            ),
        );
        let decrypted =
            crypto::aes_ecb_decrypt(&key, &hex::decode(field(&p2, "challengeresponse")).unwrap())
                .unwrap();
        let (_server_hash, server_challenge) = decrypted.split_at(32);

        // Phase 3: serverchallengeresp.
        let client_secret = rand::random::<[u8; BLOCK]>();
        let mut commit = server_challenge.to_vec();
        commit.extend_from_slice(&client.signature);
        commit.extend_from_slice(&client_secret);
        let p3 = get(
            addr,
            &format!(
                "/pair?uniqueid=itest&serverchallengeresp={}",
                hex::encode(crypto::aes_ecb_encrypt(&key, &crypto::sha256(&commit)).unwrap()),
            ),
        );
        let pairing_secret = hex::decode(field(&p3, "pairingsecret")).unwrap();
        let (server_secret, server_sign) = pairing_secret.split_at(BLOCK);
        // The client checks the server's signature against the pinned certificate.
        assert!(crypto::verify(
            &server_pinned.public,
            server_secret,
            server_sign
        ));

        // Phase 4: clientpairingsecret.
        let client_sign = crypto::sign(&client.private, &client_secret);
        let mut reveal = client_secret.to_vec();
        reveal.extend_from_slice(&client_sign);
        let p4 = get(
            addr,
            &format!(
                "/pair?uniqueid=itest&clientpairingsecret={}",
                hex::encode(crypto::aes_ecb_encrypt(&key, &reveal).unwrap()),
            ),
        );
        assert!(p4.contains("<paired>1</paired>"), "{p4}");

        assert!(get(addr, "/serverinfo?uniqueid=itest").contains("<PairStatus>1</PairStatus>"));
        assert!(get(addr, "/applist").contains("<AppTitle>ps5 on the bench</AppTitle>"));
    }

    /// A server-cert verifier that accepts any cert and records the one presented.
    #[derive(Debug)]
    struct RecordPresented {
        seen: std::sync::Mutex<Option<Vec<u8>>>,
    }

    impl rustls::client::danger::ServerCertVerifier for RecordPresented {
        fn verify_server_cert(
            &self,
            end_entity: &rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[rustls::pki_types::CertificateDer<'_>],
            _server_name: &rustls::pki_types::ServerName<'_>,
            _ocsp: &[u8],
            _now: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            *self.seen.lock().unwrap() = Some(end_entity.to_vec());
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    /// The HTTPS listener presents the pinned certificate and serves `serverinfo` over TLS.
    #[test]
    fn the_https_listener_presents_the_pinned_cert() {
        let cert = ServerCert::generate().unwrap();
        let expected_der = cert.der.clone();
        let tls_config = crate::tls::server_config(&cert).unwrap();
        let bridge = Arc::new(Bridge {
            host: Host::new("prosperous-tls".into(), Ipv4Addr::LOCALHOST),
            pairing: Pairing::new(cert),
            apps: Apps::default(),
            sessions: crate::session::Sessions::new("127.0.0.1".to_owned()),
        });

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let serving = Arc::clone(&bridge);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let config = Arc::clone(&tls_config);
                let bridge = Arc::clone(&serving);
                thread::spawn(move || {
                    if let Ok(mut connection) = rustls::ServerConnection::new(config) {
                        let mut tls = rustls::Stream::new(&mut connection, &mut stream);
                        let _ = serve_one(&mut tls, &bridge, Ipv4Addr::LOCALHOST.into());
                    }
                });
            }
        });

        let verifier = Arc::new(RecordPresented {
            seen: std::sync::Mutex::new(None),
        });
        let as_verifier: Arc<dyn rustls::client::danger::ServerCertVerifier> = verifier.clone();
        let client_config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(as_verifier)
        .with_no_client_auth();

        let server_name = rustls::pki_types::ServerName::try_from("prosperous").unwrap();
        let mut connection =
            rustls::ClientConnection::new(Arc::new(client_config), server_name).unwrap();
        let mut tcp = TcpStream::connect(addr).unwrap();
        let mut tls = rustls::Stream::new(&mut connection, &mut tcp);
        write!(
            tls,
            "GET /serverinfo?uniqueid=tls HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        // The server closes without a TLS close_notify, which rustls reports as an EOF error
        // after the whole reply is read.
        let _ = tls.read_to_string(&mut response);

        assert!(
            response.contains("<hostname>prosperous-tls</hostname>"),
            "{response}"
        );
        let seen = verifier
            .seen
            .lock()
            .unwrap()
            .clone()
            .expect("a cert was presented");
        assert_eq!(
            seen, expected_der,
            "the TLS listener served the pinned certificate"
        );
    }
}
