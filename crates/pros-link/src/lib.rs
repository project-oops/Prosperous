//! Transport for the services a prepared target runs.
//!
//! This is the one transport shared by the emulator and the conformance probe. It uses
//! `std::net` and `tracing` only, because a consumer keeps a deliberate dependency list;
//! anything that needs a library (hashing, manifests) lives in `pros-core`. See
//! `docs/DESIGN.md`.
//!
//! It holds no policy and remembers nothing between calls: the entry point does not survive
//! a power cycle, so a cached capability can expire without notice.

/// Reasons an operation could not complete, told apart.
pub mod error;
/// A target that is not a target, for tests.
pub mod fake;
/// Sending controller records to a target.
pub mod feed;
/// Browsing and moving files.
pub mod files;
/// Reading frames from a grabber on the target.
pub mod frames;
/// Where a target is, and on which ports.
pub mod link;
/// Sending a payload and running it.
pub mod loader;
/// Reading the system log.
pub mod log;
/// Reading from the payload manager's web service.
pub mod manager;
/// Controller state, on the wire.
pub mod pad;
/// Several pads at once, and what drives each one.
pub mod pads;
/// What the services are, and whether they are answering.
pub mod service;
/// What a file is, decided before it is sent.
pub mod shape;
/// Running a command on the target.
pub mod shell;
/// A stand-in payload that is not a payload, for tests.
pub mod standin;
/// Reading what is in an encoded video stream, without decoding it.
pub mod stream;

mod wire;

pub use error::{Error, Result};
pub use files::{Entry, Kind, Session};
pub use link::Link;
pub use service::{Reachability, SERVICES, Service, probe};
pub use shape::{Shape, identify};
