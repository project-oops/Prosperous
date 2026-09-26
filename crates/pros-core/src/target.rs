//! Which targets this machine knows about.
//!
//! A registration stores only what a power cycle cannot change: the address, port overrides
//! and the intended startup chain. What a target can do depends on which payloads are loaded,
//! so it is asked every time (see [`mod@crate::check`]) and never stored.
//!
//! The registry lives in the collection's shared data directory, resolved through
//! `oops_paths`, so sibling projects reach the same targets. It is one hand-edited line per
//! target, `<name> <address> [service=port ...] [chain=<name>]`, split on whitespace.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

/// A target somebody has registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// A short label, used to pick one when several are registered.
    pub name: String,
    /// Host or address. Stored as written, resolved at use.
    pub address: String,
    /// Ports this target uses instead of the compiled-in ones, by service name.
    ///
    /// Empty is the normal case: the built-in ports are measured, and an override exists for
    /// another payload speaking the same protocol on a different port.
    pub ports: BTreeMap<String, u16>,
    /// Which startup chain this target is meant to be running, by preset name.
    ///
    /// The right advice depends on it: a chain that starts the loader and file service itself
    /// must not be told they are missing. `None` means nobody has said, and the shipped chain
    /// answers for it without being recorded as chosen.
    pub chain: Option<String>,
}

impl Target {
    /// Where this target is and on what ports, for everything that connects.
    ///
    /// Every connection goes through this, so a port override applies to transfers as well as
    /// checks.
    #[must_use]
    pub fn link(&self) -> pros_link::Link {
        pros_link::Link {
            address: self.address.clone(),
            ports: self.ports.clone(),
        }
    }
}

/// Where registrations are kept.
///
/// `None` when this machine has nowhere to keep the file; the caller says so.
#[must_use]
pub fn path() -> Option<PathBuf> {
    let mut path = directory()?;
    path.push("targets.txt");
    Some(path)
}

/// Where downloads and staged packages are kept.
#[must_use]
pub fn cache_directory() -> Option<PathBuf> {
    // A cache, not settings: everything here can be fetched again and verified by digest.
    oops_paths::Paths::resolve_with_options("prosperous", oops_paths::Options::new().refusing())
        .map(|paths| paths.cache_root().to_path_buf())
}

/// Where registrations and settings are kept.
#[must_use]
pub fn directory() -> Option<PathBuf> {
    // Refusing rather than falling back to the working directory, where a person would never
    // find the registry.
    oops_paths::Paths::resolve_with_options("prosperous", oops_paths::Options::new().refusing())
        .map(|paths| paths.data_root().to_path_buf())
}

/// Every registration. A missing file is an empty list, not a failure.
///
/// # Errors
///
/// Propagates a read failure other than "no such file".
pub fn load() -> std::io::Result<Vec<Target>> {
    let Some(path) = path() else {
        return Ok(Vec::new());
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let targets = parse(&text);
            tracing::debug!(count = targets.len(), path = %path.display(), "read registrations");
            Ok(targets)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Not a warning: this is every first run.
            tracing::debug!(path = %path.display(), "no registrations yet");
            Ok(Vec::new())
        }
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "cannot read registrations");
            Err(error)
        }
    }
}

/// Adds a registration, or replaces the one with that name.
///
/// Returns where it was written, for the caller to report.
///
/// # Errors
///
/// Propagates the write, and reports a machine with no home directory.
pub fn register(name: &str, address: &str) -> std::io::Result<PathBuf> {
    let Some(path) = path() else {
        return Err(std::io::Error::other(
            "no home directory, so there is nowhere to keep registrations",
        ));
    };
    let mut targets = load()?;
    // Ports and chain survive a re-registration: correcting an address must not discard them.
    let known = targets.iter().find(|target| target.name == name);
    let ports = known.map(|target| target.ports.clone()).unwrap_or_default();
    let chain = known.and_then(|target| target.chain.clone());
    targets.retain(|target| target.name != name);
    targets.push(Target {
        name: name.to_owned(),
        address: address.to_owned(),
        ports,
        chain,
    });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, render(&targets))?;
    Ok(path)
}

/// Records which startup chain a target is meant to be running.
///
/// Separate from [`register`] so changing an address and choosing a chain are independent.
///
/// # Errors
///
/// A registry that cannot be read or written. A name that is not registered is `Ok(false)`.
pub fn remember_chain(name: &str, chain: Option<&str>) -> std::io::Result<bool> {
    let path = path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no home directory"))?;
    let mut targets = load()?;
    let Some(one) = targets.iter_mut().find(|target| target.name == name) else {
        return Ok(false);
    };
    one.chain = chain.map(ToOwned::to_owned);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, render(&targets))?;
    Ok(true)
}

/// Removes a registration. Answers whether there was one.
///
/// # Errors
///
/// Propagates the write.
pub fn forget(name: &str) -> std::io::Result<bool> {
    let Some(path) = path() else {
        return Ok(false);
    };
    let mut targets = load()?;
    let before = targets.len();
    targets.retain(|target| target.name != name);
    if targets.len() == before {
        return Ok(false);
    }
    std::fs::write(&path, render(&targets))?;
    Ok(true)
}

/// Picks a target by name, or the only one when no name is given.
///
/// # Errors
///
/// [`Ambiguous`], which keeps none registered, an unknown name, and several registered apart
/// because each has a different remedy.
pub fn resolve(targets: Vec<Target>, wanted: Option<&str>) -> Result<Target, Ambiguous> {
    if let Some(name) = wanted {
        return targets
            .into_iter()
            .find(|target| target.name == name)
            .ok_or_else(|| Ambiguous::NoSuchName {
                name: name.to_owned(),
            });
    }
    let names: Vec<String> = targets.iter().map(|target| target.name.clone()).collect();
    let mut only = targets.into_iter();
    match (only.next(), names.len()) {
        (Some(target), 1) => Ok(target),
        (None, _) => Err(Ambiguous::NoneRegistered),
        _ => Err(Ambiguous::Several { names }),
    }
}

/// Why a target could not be picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ambiguous {
    /// Nothing has ever been registered on this machine.
    NoneRegistered,
    /// A name was given and matches nothing.
    NoSuchName {
        /// The name that was asked for.
        name: String,
    },
    /// No name was given and there is more than one to mean.
    Several {
        /// What is registered, so the message can list them rather than say "several".
        names: Vec<String>,
    },
}

impl fmt::Display for Ambiguous {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoneRegistered => write!(f, "no targets are registered"),
            Self::NoSuchName { name } => write!(f, "no target is registered as {name:?}"),
            Self::Several { names } => {
                write!(f, "several targets are registered: {}", names.join(", "))
            }
        }
    }
}

impl std::error::Error for Ambiguous {}

/// Reads the file. One target per line, `#` comments and blank lines ignored.
///
/// A line with a name and no address is skipped rather than stored with an empty address
/// that fails later.
fn parse(text: &str) -> Vec<Target> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (name, rest) = line.split_once(char::is_whitespace)?;
            let mut words = rest.split_whitespace();
            let address = words.next()?;
            // Anything that is not `service=port` is skipped, not guessed at: a malformed
            // override would connect to some other listener under this service's name.
            let words: Vec<&str> = words.collect();
            // `chain=` is the one non-numeric `key=value`, so it is read before the port
            // parser skips it.
            let chain = words.iter().find_map(|word| {
                let rest = word.strip_prefix("chain=")?;
                (!rest.is_empty()).then(|| rest.to_owned())
            });
            let ports = words
                .iter()
                .filter(|word| !word.starts_with("chain="))
                .filter_map(|word| {
                    let (service, port) = word.split_once('=')?;
                    let port: u16 = port.parse().ok()?;
                    (!service.is_empty() && port > 0).then(|| (service.to_owned(), port))
                })
                .collect();
            (!address.is_empty()).then(|| Target {
                name: name.to_owned(),
                address: address.to_owned(),
                ports,
                chain,
            })
        })
        .collect()
}

/// Writes the file, header and all.
fn render(targets: &[Target]) -> String {
    use std::fmt::Write as _;
    let mut out = String::from(
        "# Targets this machine knows about, one per line:\n\
         #\n\
         #   <name> <address> [service=port ...]\n\
         #\n\
         # A line here is an address, not a promise. Whether a target is reachable or\n\
         # prepared is established by a check, every time, because the answer changes\n\
         # on every reboot.\n\
         #\n\
         # The trailing pairs are only for a target that does NOT use the usual ports -\n\
         # a different FTP server, say, on 2122 rather than 2121:\n\
         #\n\
         #   prospero 192.168.1.211 ftpsrv=2122\n\
         #\n\
         # They are used everywhere, not only by the check, so an override changes where\n\
         # files actually go. A wrong one talks to whatever else is listening there.\n\n",
    );
    for target in targets {
        let _ = write!(out, "{} {}", target.name, target.address);
        for (service, port) in &target.ports {
            let _ = write!(out, " {service}={port}");
        }
        if let Some(chain) = &target.chain {
            let _ = write!(out, " chain={chain}");
        }
        let _ = writeln!(out);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Ambiguous, Target, parse, render, resolve};

    fn target(name: &str, address: &str) -> Target {
        Target {
            name: name.to_owned(),
            address: address.to_owned(),
            ports: BTreeMap::new(),
            chain: None,
        }
    }

    /// A chain survives the round trip beside the ports and is not mistaken for one.
    #[test]
    fn a_chain_is_written_and_read_back_beside_the_ports() {
        let mut one = target("prospero", "192.168.1.211");
        one.ports.insert("ftpsrv".to_owned(), 2122);
        one.chain = Some("etaHEN".to_owned());

        let again = parse(&render(std::slice::from_ref(&one)));
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].chain.as_deref(), Some("etaHEN"));
        assert_eq!(again[0].ports.get("ftpsrv"), Some(&2122));
    }

    /// A registration that says nothing about a chain has not chosen one.
    #[test]
    fn no_chain_is_absent_rather_than_a_default() {
        let again = parse("prospero 192.168.1.211\n");
        assert_eq!(again[0].chain, None);
    }

    /// An empty chain is no chain.
    #[test]
    fn an_empty_chain_is_not_a_chain() {
        let again = parse("prospero 192.168.1.211 chain=\n");
        assert_eq!(again[0].chain, None);
    }

    /// Comments and blank lines are skipped and each target line is read.
    #[test]
    fn a_registration_round_trips() {
        let found = parse("# comment\n\nliving-room 192.168.1.206\ndesk  10.0.0.4\n");
        assert_eq!(found.len(), 2);
        assert_eq!(found.first().map(|c| c.name.as_str()), Some("living-room"));
        assert_eq!(found.get(1).map(|c| c.address.as_str()), Some("10.0.0.4"));
    }

    /// A line with no address is not a registration.
    #[test]
    fn a_line_with_no_address_is_skipped_rather_than_stored_empty() {
        assert!(parse("solo\n").is_empty());
        assert!(parse("name   \n").is_empty());
    }

    /// What is written can be read, including the header.
    #[test]
    fn rendering_is_parseable_again() {
        let targets = vec![target("prospero", "192.168.1.206")];
        assert_eq!(parse(&render(&targets)), targets);
    }

    /// Resolving without a name needs exactly one target, and the two failures differ.
    #[test]
    fn resolving_without_a_name_needs_exactly_one() {
        assert!(resolve(vec![target("a", "1")], None).is_ok());
        assert_eq!(resolve(Vec::new(), None), Err(Ambiguous::NoneRegistered));

        let two = vec![target("a", "1"), target("b", "2")];
        match resolve(two, None) {
            Err(Ambiguous::Several { names }) => assert_eq!(names, vec!["a", "b"]),
            other => panic!("expected the names, got {other:?}"),
        }
    }

    /// An unknown name is reported as that name.
    #[test]
    fn an_unknown_name_is_reported_as_that_name() {
        assert_eq!(
            resolve(vec![target("a", "1")], Some("b")),
            Err(Ambiguous::NoSuchName {
                name: "b".to_owned()
            })
        );
    }
}

#[cfg(test)]
mod port_tests {
    use super::{parse, render};

    /// A line with no overrides gives a plain link.
    #[test]
    fn a_plain_registration_is_unchanged() {
        let targets = parse("ps5 192.168.1.211\n");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].address, "192.168.1.211");
        assert!(targets[0].ports.is_empty());
        assert!(targets[0].link().is_plain());
    }

    /// An override reaches the link that every connection goes through.
    #[test]
    fn an_override_is_read_and_reaches_the_link() {
        let targets = parse("ps5 192.168.1.211 ftpsrv=2122 klogsrv=3300\n");
        let link = targets[0].link();
        assert_eq!(link.port("ftpsrv", 2121), 2122);
        assert_eq!(link.port("klogsrv", 3232), 3300);
        assert_eq!(link.port("shsrv", 2323), 2323, "untouched");
    }

    /// A malformed override is dropped and the measured default applies.
    #[test]
    fn a_malformed_override_is_ignored_rather_than_interpreted() {
        let targets = parse("ps5 192.168.1.211 ftpsrv nonsense= =2122 shsrv=0 shsrv=notanumber\n");
        assert_eq!(targets[0].address, "192.168.1.211");
        assert!(
            targets[0].ports.is_empty(),
            "none of those is a port: {:?}",
            targets[0].ports
        );
    }

    /// What is written reads back the same, overrides and all.
    #[test]
    fn a_registration_survives_being_written_and_read() {
        let before = parse("ps5 192.168.1.211 ftpsrv=2122\nspare 10.0.0.9\n");
        let after = parse(&render(&before));
        assert_eq!(before, after);
    }
}
