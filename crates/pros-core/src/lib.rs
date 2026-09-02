//! What to do with a target, as opposed to how to talk to one.
//!
//! # Why this is a separate crate from the transport
//!
//! Not tidiness. `pros-link` is taken by a project that argues for each of its three
//! dependencies individually and forbids unsafe code, so its own dependency table is empty
//! and has to stay that way. Everything that genuinely needs a library - reading somebody
//! else's JSON, hashing a download before it is run - lives here, where that project never
//! sees it.
//!
//! The split is therefore **what needs a dependency**, and it falls exactly where the
//! consumers differ. See `docs/DESIGN.md`.
//!
//! # What is here
//!
//! - [`target`] - which targets this machine knows about. A name and an address, and
//!   nothing else, because anything else expires without notice.
//! - [`manifest`] - where payloads come from, in the payload manager's own schema so a
//!   target that is already configured is already described.
//! - [`checksum`] - proving a payload is the one that was described, before it is run.
//! - [`mod@check`] - what a target can currently do, and what to do about what it cannot.

/// The payload manager's own settings, and changing them.
pub mod autoload;
/// Editing what the target loads at startup.
pub mod boot;
/// Which build this is.
pub mod build;
/// What the target loads when it comes back.
pub mod catalogue;
pub mod chain;
/// What a target can currently do, and what is missing.
pub mod check;
/// Proving bytes are the bytes that were described.
pub mod checksum;
/// Health checks that say what is wrong and exactly what would put it right.
pub mod doctor;
/// Getting a payload, by asking something that already knows how.
pub mod fetch;
/// Putting one save's contents into another save's container.
pub mod graft;
/// Holding one file out for the target to fetch.
pub mod handover;
/// Starting a title on the target.
pub mod hbldr;
/// Installing a package on the target.
pub mod install;
pub mod launch;
/// What is on the target's storage.
pub mod library;
/// Which of several places a thing is actually kept.
pub mod locate;
/// Where payloads come from.
pub mod manifest;
/// Where a copied save came from, and whether it can go back as-is.
pub mod origin;
/// What is described, what is trustworthy, and what is on the target.
pub mod payloads;
/// Where things live on a target, per storage device.
pub mod places;
/// Watching the target, by starting something that already knows how.
pub mod recovery;
pub mod remove;
/// Showing a folder in the system's file browser.
pub mod reveal;
/// Where save data is.
pub mod saves;
/// Reading the parameter files beside a save.
pub mod sfo;
/// Payloads kept ready to be sent.
/// Asking a payload's own project what it has released.
pub mod sources;
pub mod staging;
/// Keeping a probe alive on a target that cannot restart it.
pub mod supervise;
/// What the target is: firmware, target, storage, and what is running.
pub mod system;
/// Which targets this machine knows about.
pub mod target;
/// What a title is called, as opposed to what its folder is called.
pub mod titles;
/// Copying a whole folder off the target, and putting one back.
pub mod transfer;
/// Watching the stand-in stream: read it, count it, pipe it to a player.
pub mod watch;

pub use chain::Chain;
pub use check::{Finding, Report, check};
pub use checksum::{Algorithm, Checksum};
pub use manifest::{Manifest, Payload};
pub use payloads::{Boot, Presence, Row, Trust, survey};
pub use target::Target;
