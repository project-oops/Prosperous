//! A Moonlight/GameStream host bridge in front of Porthole's two ports.
//!
//! # What this is
//!
//! `docs/VIDEO.md` part four states the design in full. In one breath: the target's Porthole
//! payload serves encoded video on 9805 and reads controller records on 9806, and this crate is a
//! *second consumer* of those two ports that re-presents them as the NVIDIA GameStream protocol
//! every Moonlight client already speaks. One LAN hop, no re-encode, and a phone, a Steam Deck or a
//! TV becomes a screen for a target that never learned their protocol.
//!
//! # It invents nothing, and it decodes nothing
//!
//! The protocol has no published specification; it is defined by three GPLv3 codebases -
//! moonlight-common-c, Sunshine and Wolf - and one BSD-2-Clause one, `moonshine`, which proved it
//! can be spoken in pure Rust. This crate implements the **behaviour** those describe and copies no
//! code from any of them (`ACKNOWLEDGEMENTS.md`). It never decodes a video frame: it finds NAL
//! boundaries and keyframes with [`pros_link::stream`] - the same reader Porthole's own `watch`
//! uses for its counts - and packetises, exactly as far as "[reading is not
//! decoding](../pros_link/stream/index.html)" allows.
//!
//! # The parts, and the order they can be built and tested
//!
//! Every part below is verifiable on one machine against a stock Moonlight client, with **no
//! console involved** - which is how the mesh wants it, obSCEne alone touching hardware. The
//! [`fake`] target is what makes that true: it stands in for the payload, serving a canned
//! Annex-B clip on 9805 and printing the controller records that arrive on 9806.
//!
//! - [`fake`] - the stand-in target, so the bridge has something to bridge without a console.
//!
//! The discovery, pairing, session and streaming parts land on top of this foundation; each is a
//! port the client talks to, and each is added only once the one below it answers.

/// What can go wrong in the bridge, told apart.
pub mod error;

/// A target that is not a target, for driving the bridge without a console.
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
