//! What to do with a target, as opposed to how to talk to one.
//!
//! Anything that needs a library (JSON, hashing) lives here; `pros-link`, the transport, keeps
//! an empty dependency table because obSCEne depends on it. See `docs/DESIGN.md`.
//!
//! - [`target`] - which targets this machine knows about: a name and an address only.
//! - [`manifest`] - where payloads come from, in the payload manager's own schema.
//! - [`checksum`] - proving a payload is the one that was described, before it is run.
//! - [`mod@check`] - what a target can do, and what to do about what it cannot.

/// The payload manager's own settings, and changing them.
pub mod autoload;
/// Editing what the target loads at startup.
pub mod boot;
/// Which build this is.
pub mod build;
/// What the target loads when it comes back.
pub mod catalogue;
pub mod chain;
/// What a target can do, and what is missing.
pub mod check;
/// Proving bytes are the bytes that were described.
pub mod checksum;
/// What was verified landing on a target, so a restore does not re-send an unchanged file.
pub mod deployed;
/// Health checks that say what is wrong and exactly what would put it right.
pub mod doctor;
/// Getting a payload, by asking something that already knows how.
pub mod fetch;
/// Putting one save's contents into another save's container.
pub mod graft;
/// Validating title paths and prefixes before staging.
pub mod guard;
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
/// Launching a title and following what it says: the probe loop's steps.
pub mod probe;
/// Watching the target, by starting something that already knows how.
pub mod recovery;
pub mod remove;
/// Showing a folder in the system's file browser.
pub mod reveal;
/// Where save data is.
pub mod saves;
/// Reading the parameter files beside a save.
pub mod sfo;
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
