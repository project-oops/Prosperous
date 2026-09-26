//! A Moonlight/GameStream host bridge in front of Porthole's two ports.
//!
//! Porthole serves encoded video on 9805 and reads controller records on 9806. This crate is a
//! second consumer of those ports that presents them as the GameStream protocol a Moonlight
//! client speaks, with no re-encode. It never decodes a frame: it finds NAL boundaries and
//! keyframes with [`pros_link::stream`] and packetises. It implements the behaviour the public
//! codebases describe and copies no code from them (`ACKNOWLEDGEMENTS.md`). The design is in
//! `docs/VIDEO.md`. The [`fake`] target stands in for Porthole, so the whole bridge runs against
//! a stock client on one machine.

/// What can go wrong in the bridge, told apart.
pub mod error;

/// A stand-in for Porthole, for driving the bridge without hardware.
pub mod fake;

/// The apps a client sees, one per target.
pub mod apps;

/// What the bridge tells a client about itself: ports, identity, and `serverinfo`.
pub mod host;

/// The pairing primitives: hashing, AES-128-ECB, and RSA signatures.
mod crypto;

/// The server's own certificate, and reading the client's.
mod cert;

/// The four-phase pairing handshake.
mod pairing;

/// Grouping an Annex-B stream into frames for the packetiser.
mod nal;

/// Turning encoded frames into the RTP packets a Moonlight client expects.
mod video;

/// Turning a Moonlight controller packet into a target pad.
mod input;

/// A streaming session: the requested mode and the video pipeline.
mod session;

/// The RTSP handshake that sets a stream up.
mod rtsp;

/// The ENet control channel that carries input.
mod control;

/// The little HTTP server the GameStream endpoints are served over.
mod http;

/// The TLS configuration for the HTTPS port.
mod tls;

/// Advertising the bridge over mDNS.
mod discovery;

/// Running the bridge: discovery, HTTP and HTTPS.
mod serve;

pub use apps::{App, Apps};
pub use serve::run;
