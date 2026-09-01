//! Where a target is, and on which ports.
//!
//! # Why an address is not enough any more
//!
//! Every service here had one port, compiled in, measured against a target. That is a fine
//! default and a bad requirement: the five payloads this program knows are not the only ones
//! that speak their protocols, and somebody running a different FTP server on a different port
//! has no way to say so.
//!
//! # Why it is one type rather than a port argument
//!
//! Because the alternative was tried on paper and is the bug. Overrides that reach only the
//! **check** would let a file say *ftpsrv is on 2122*, the check probe 2122 and go green, and
//! every transfer still go to 2121. The config would be believed and disobeyed at the same
//! time, which is worse than not having it - a wrong answer wearing the shape of a right one.
//!
//! So the address and its ports travel together, and everything that connects takes this. A
//! function that takes a `&str` cannot honour an override, and now there are none.

use std::collections::BTreeMap;

/// A target's address, and any ports it does not use the usual ones for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Link {
    /// Host or address, as written. Resolved at use.
    pub address: String,
    /// Ports this target uses instead of the compiled-in ones, by service name.
    ///
    /// **Empty is the normal case** and means every default applies.
    pub ports: BTreeMap<String, u16>,
}

impl Link {
    /// A link to an address using every default.
    #[must_use]
    pub fn to(address: &str) -> Self {
        Self {
            address: address.to_owned(),
            ports: BTreeMap::new(),
        }
    }

    /// The port to use for a service: the override if there is one, otherwise `default`.
    ///
    /// **The default is passed in rather than looked up** so that the caller which knows
    /// which service it is talking to is the one that says. A lookup by name here would let a
    /// typo silently fall through to some other service's port.
    #[must_use]
    pub fn port(&self, service: &str, default: u16) -> u16 {
        self.ports.get(service).copied().unwrap_or(default)
    }

    /// Whether anything about this target is non-standard.
    ///
    /// Worth saying out loud in a report: a check that passes against overridden ports is a
    /// different claim from one that passes against the usual ones.
    #[must_use]
    pub fn is_plain(&self) -> bool {
        self.ports.is_empty()
    }
}

impl From<&str> for Link {
    fn from(address: &str) -> Self {
        Self::to(address)
    }
}

#[cfg(test)]
mod tests {
    use super::Link;

    /// With nothing overridden, every service gets the port it was built with.
    #[test]
    fn a_plain_link_uses_every_default() {
        let link = Link::to("10.0.0.1");
        assert_eq!(link.port("ftpsrv", 2121), 2121);
        assert_eq!(link.port("shsrv", 2323), 2323);
        assert!(link.is_plain());
    }

    /// **An override applies to the one service it names, and to nothing else.**
    #[test]
    fn an_override_applies_only_to_what_it_names() {
        let mut link = Link::to("10.0.0.1");
        link.ports.insert("ftpsrv".to_owned(), 2122);
        assert_eq!(link.port("ftpsrv", 2121), 2122);
        assert_eq!(link.port("shsrv", 2323), 2323, "untouched");
        assert!(!link.is_plain());
    }

    /// A name nothing overrode falls through to the default, rather than to some other
    /// service's port.
    #[test]
    fn an_unknown_name_gets_its_own_default() {
        let mut link = Link::to("10.0.0.1");
        link.ports.insert("ftpsrv".to_owned(), 2122);
        assert_eq!(link.port("garlic-savemgr", 8082), 8082);
    }
}
