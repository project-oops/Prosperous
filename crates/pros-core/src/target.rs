//! Which targets this machine knows about.
//!
//! # A registration is where a target is, and nothing about what it can do
//!
//! The distinction is deliberate and load-bearing. What a target can do depends on which
//! payloads happen to be loaded; that set does not survive a power cycle, and what comes back
//! depends on a text file somebody edited weeks ago. **Anything stored about a target's
//! capabilities is a claim that expires without notice**, and it expires silently, which is
//! the worst kind.
//!
//! So capability is asked every time - see [`mod@crate::check`] - and this file holds only
//! facts that do not change on their own.
//!
//! # The one extension, and why it is not a violation of the above
//!
//! A registration may also carry **ports**, and the rule above is what says when that is
//! allowed. *Is ftpsrv loaded* expires on every reboot and is never stored. *Which port this
//! target's FTP server listens on* does not: it is a property of how somebody set the machine
//! up, in the same class as the address itself, and it changes when they change it.
//!
//! The test is whether a power cycle can make the stored answer wrong without anybody doing
//! anything. For a capability, yes. For an address or a port, no.
//!
//! Empty is the normal case and the built-in ports apply.
//!
//! # Not under the per-user application data directory on Windows
//!
//! A tool running inside a packaged container has its writes there redirected into a
//! per-package cache, invisible to the same user running the same tool from an ordinary
//! shell. **A configuration file the user cannot find is worse than no configuration file**,
//! so this uses a plain dotted directory under the home directory on every platform.
//!
//! That argument now lives in `oops_paths`, where it settled the default for the whole
//! collection - this was the project that had met the problem, so this is where the reasoning
//! came from. Using the shared crate also brings portable mode: a `.portable` directory beside
//! the binary, or `PROSPEROUS_DATA_DIR`, moves everything below.
//!
//! # Parsed by hand, deliberately
//!
//! One line per target: a name, an address, and any port overrides. This crate carries a JSON reader
//! for somebody else's document, and using it here would still be the wrong call - splitting
//! a line on whitespace is simpler to test than a format is to justify, and the file is meant
//! to be edited by a person.

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
    /// **Empty is the normal case.** The five built-in ports were measured, and a registration
    /// that repeated them would be a copy of a fact rather than a correction to it.
    ///
    /// This exists because the five payloads this program knows are not the only ones that
    /// speak their protocols - there are three FTP servers in the shipped payload list alone -
    /// and somebody running one on a different port had no way to say so. See
    /// `docs/CAPABILITIES.md`.
    pub ports: BTreeMap<String, u16>,
    /// Which startup chain this target is meant to be running, by preset name.
    ///
    /// # Why a target remembers it
    ///
    /// **Because the right advice depends on it.** A console brought up by etaHEN already has
    /// the loader, FTP and the kernel log running - etaHEN starts them - so a check that
    /// reports those as missing is reporting a correct configuration as broken, and the fix it
    /// offers would put a second copy of each beside the first.
    ///
    /// `None` is *nobody has said*, and the shipped chain answers for it. That is not the same
    /// as choosing that chain, and it is why this is an `Option` rather than a default written
    /// into every registration.
    pub chain: Option<String>,
}

impl Target {
    /// Where this target is and on what ports, for everything that connects.
    ///
    /// **Everything goes through this**, which is the point: an override that reached only the
    /// check would let the file say one thing while every transfer did another.
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
/// `None` when the home directory cannot be determined, which is not a failure worth
/// propagating as an error: it means this machine has nowhere to keep the file, and the
/// caller should say so plainly.
#[must_use]
pub fn path() -> Option<PathBuf> {
    let mut path = directory()?;
    path.push("targets.txt");
    Some(path)
}

/// Where this project keeps what it knows.
///
/// One directory for the registry, the manifest and any staged payload, so that a person
/// looking for any of them finds all of them.
#[must_use]
pub fn cache_directory() -> Option<PathBuf> {
    // Downloads, not settings. The manifest names a URL and a digest for every one of them, so
    // anything here can be fetched again and verified again - which is exactly the test for
    // what belongs beside the machine rather than in a profile that follows the user.
    oops_paths::Paths::resolve_with_options("prosperous", oops_paths::Options::new().refusing())
        .map(|paths| paths.cache_root().to_path_buf())
}

/// Where registrations and settings are kept.
#[must_use]
pub fn directory() -> Option<PathBuf> {
    // Refusing rather than falling back, because this is a file a person goes looking for:
    // writing the registry beside wherever they happened to be standing is how it becomes
    // unfindable. The policy is a parameter, so the reason is at the call site.
    oops_paths::Paths::resolve_with_options("prosperous", oops_paths::Options::new().refusing())
        .map(|paths| paths.data_root().to_path_buf())
}

/// Every registration. A missing file is an empty list, not a failure.
///
/// # Errors
///
/// Propagates a read failure that is not "no such file" - a directory that cannot be read is
/// worth reporting, an absent one is just a machine where nothing has been registered yet.
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
            // Not a warning. Nobody has registered anything yet, which is where everyone
            // starts, and a warning on first run trains people to ignore warnings.
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
/// Returns where it was written, so a caller can say. **This does not print**: a library that
/// writes to the terminal decides the interface of every tool that uses it.
///
/// # Errors
///
/// Propagates the write, and reports a machine with no home directory as an error here
/// because at this point somebody has asked for something that cannot be done.
pub fn register(name: &str, address: &str) -> std::io::Result<PathBuf> {
    let Some(path) = path() else {
        return Err(std::io::Error::other(
            "no home directory, so there is nowhere to keep registrations",
        ));
    };
    let mut targets = load()?;
    // **Kept across a re-registration.** Registering again is how somebody corrects an address,
    // and losing their port overrides for it would be a silent edit to a file they did not open
    // - discovered later, as transfers going to the wrong place.
    // **Kept across a re-registration.** Both of these are things somebody established about
    // this target and neither is part of *where it is*, so changing an address must not quietly
    // discard the ports it answers on or the chain it was set up with.
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
/// **Its own function rather than a field on `register`**, because it is not part of *where a
/// target is*. Somebody changing an address should not have to restate how the console was set
/// up, and somebody choosing a chain should not have to retype an address.
///
/// # Errors
///
/// A registry that cannot be read or written. A name that is not registered is `Ok(false)`
/// rather than an error - there is nothing to record it against and nothing went wrong.
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
/// [`Ambiguous`], which distinguishes *none registered* from *several, say which* - two
/// situations with different remedies that a single "could not resolve" would flatten.
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
/// A line with a name and no address is **not half a registration**, it is not one, and it
/// is skipped rather than stored with an empty address that fails later somewhere less
/// obvious.
fn parse(text: &str) -> Vec<Target> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (name, rest) = line.split_once(char::is_whitespace)?;
            let mut words = rest.split_whitespace();
            let address = words.next()?;
            // **Anything that is not `service=port` is skipped, not guessed at.** A malformed
            // override silently becoming a port is the one failure this whole feature exists
            // to avoid: it would report some other listener's state under this name.
            let words: Vec<&str> = words.collect();
            // **Taken out before the ports are read**, because it is the one `key=value` here
            // whose value is not a number - and the port parser's job is to skip anything that
            // is not one, which would silently swallow this.
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
         #   ps5 192.168.1.211 ftpsrv=2122\n\
         #\n\
         # They are used everywhere, not only by the check, so an override changes where\n\
         # files actually go. A wrong one talks to whatever else is listening there.\n\n",
    );
    for target in targets {
        let _ = write!(out, "{} {}", target.name, target.address);
        for (service, port) in &target.ports {
            let _ = write!(out, " {service}={port}");
        }
        // Last, so a line reads as *this target, here, on these ports* and then the one thing
        // that is about how it was set up rather than how it is reached.
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

    /// **A chain survives the round trip**, beside the ports and told apart from them.
    ///
    /// The registry line is whitespace-delimited and `chain=` is the one `key=value` on it
    /// whose value is not a number - which is exactly what the port reader is written to skip.
    #[test]
    fn a_chain_is_written_and_read_back_beside_the_ports() {
        let mut one = target("ps5", "192.168.1.211");
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
        let again = parse(
            "ps5 192.168.1.211
",
        );
        assert_eq!(again[0].chain, None);
    }

    /// **A malformed chain is no chain**, the same as a malformed port is no port.
    #[test]
    fn an_empty_chain_is_not_a_chain() {
        let again = parse(
            "ps5 192.168.1.211 chain=
",
        );
        assert_eq!(again[0].chain, None);
    }

    #[test]
    fn a_registration_round_trips() {
        let found = parse("# comment\n\nliving-room 192.168.1.206\ndesk  10.0.0.4\n");
        assert_eq!(found.len(), 2);
        assert_eq!(found.first().map(|c| c.name.as_str()), Some("living-room"));
        assert_eq!(found.get(1).map(|c| c.address.as_str()), Some("10.0.0.4"));
    }

    /// Half a line is not half a registration.
    #[test]
    fn a_line_with_no_address_is_skipped_rather_than_stored_empty() {
        assert!(parse("solo\n").is_empty());
        assert!(parse("name   \n").is_empty());
    }

    /// What is written can be read, including the header.
    #[test]
    fn rendering_is_parseable_again() {
        let targets = vec![target("ps5", "192.168.1.206")];
        assert_eq!(parse(&render(&targets)), targets);
    }

    /// Naming nothing is only unambiguous when there is one thing to mean, and the two ways
    /// that fails have different remedies.
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

    /// A name that matches nothing says which name, because it is usually a typo.
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

    /// A line with no overrides reads exactly as it always did.
    #[test]
    fn a_plain_registration_is_unchanged() {
        let targets = parse("ps5 192.168.1.211\n");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].address, "192.168.1.211");
        assert!(targets[0].ports.is_empty());
        assert!(targets[0].link().is_plain());
    }

    /// **An override reaches the link**, which is what everything connects through.
    #[test]
    fn an_override_is_read_and_reaches_the_link() {
        let targets = parse("ps5 192.168.1.211 ftpsrv=2122 klogsrv=3300\n");
        let link = targets[0].link();
        assert_eq!(link.port("ftpsrv", 2121), 2122);
        assert_eq!(link.port("klogsrv", 3232), 3300);
        assert_eq!(link.port("shsrv", 2323), 2323, "untouched");
    }

    /// **Nonsense is dropped, never guessed at.**
    ///
    /// A malformed pair silently becoming a port is the one failure this feature could
    /// introduce: it would connect to whatever else is listening there and report that as this
    /// service. Better to ignore the word and use the measured default.
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
