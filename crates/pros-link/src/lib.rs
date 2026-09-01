//! Transport for the services a prepared target runs.
//!
//! # What this crate is for
//!
//! Two projects need to talk to a target and neither is a target tool: an emulator that
//! can only settle some questions by asking real target, and a conformance probe whose
//! entire delivery problem is getting itself onto the machine. Both had started building
//! the same transport. This is that transport, once.
//!
//! # Why it has no dependencies, and must not acquire any
//!
//! One of those consumers carries three dependencies, each argued for in its own manifest,
//! and forbids unsafe code. A transport crate with a runtime or a serialisation framework
//! inside it could not be taken by that project without breaking a policy it holds
//! deliberately.
//!
//! So the line is drawn at **what needs a dependency**. Hashing a downloaded payload and
//! reading a manifest live one layer up, in a crate that consumer does not take. See
//! `docs/DESIGN.md`.
//!
//! # What it does not decide
//!
//! Nothing here holds policy. It does not choose which target to talk to, does not decide
//! what a slow refusal means, and does not remember anything between calls - a jailbreak
//! does not survive a power cycle, so a cached capability is a claim that expires without
//! notice.

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
