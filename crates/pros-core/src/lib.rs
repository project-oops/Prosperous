//! What to do with a target, as opposed to how to talk to one.
//!
//! Anything that needs a library (JSON, hashing) lives here; `pros-link`, the transport, keeps
//! its dependencies to `tracing` because obSCEne depends on it (D025).
//!
//! - [`target`] - which targets this machine knows about: a name and an address only.
//! - [`manifest`] - where payloads come from, in the payload manager's own schema.
//! - [`checksum`] - proving a payload is the one that was described, before it is run.
//! - [`mod@check`] - what a target can do, and what to do about what it cannot.

pub mod autoload;
pub mod boot;
pub mod build;
pub mod catalogue;
pub mod chain;
pub mod check;
pub mod checksum;
pub mod deployed;
pub mod doctor;
pub mod error;
pub mod fetch;
pub mod graft;
pub mod guard;
pub mod handover;
pub mod hbldr;
pub mod install;
pub mod launch;
pub mod library;
pub mod locate;
pub mod manifest;
pub mod origin;
pub mod payloads;
pub mod places;
pub mod probe;
pub mod recovery;
pub mod remove;
pub mod reveal;
pub mod saves;
pub mod sfo;
pub mod sources;
pub mod staging;
pub mod supervise;
pub mod system;
pub mod target;
pub mod titles;
pub mod transfer;
pub mod watch;

pub use chain::Chain;
pub use check::{Finding, Report, check};
pub use checksum::{Algorithm, Checksum};
pub use error::{Error, Result};
pub use manifest::{Manifest, Payload};
pub use payloads::{Boot, Presence, Row, Trust, survey};
pub use target::Target;
