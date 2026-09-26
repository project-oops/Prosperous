//! Where a target is, and on which ports.
//!
//! Each service has a measured default port, and another server speaking the same protocol
//! may listen elsewhere. The address and its port overrides travel together and everything
//! that connects takes a [`Link`], so an override reaches the check and every transfer
//! alike rather than letting a check pass on one port while transfers go to another.

use std::collections::BTreeMap;

/// A target's address, and any ports it does not use the usual ones for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Link {
    /// Host or address, as written. Resolved at use.
    pub address: String,
    /// Ports this target uses instead of the compiled-in ones, by service name.
    /// Empty means every default applies.
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
    /// The caller passes the default, so a mistyped name falls back to that service's own
    /// port rather than another's.
    #[must_use]
    pub fn port(&self, service: &str, default: u16) -> u16 {
        self.ports.get(service).copied().unwrap_or(default)
    }

    /// Whether anything about this target is non-standard.
    ///
    /// A report says so, since a check against overridden ports is a different claim.
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

    /// An override applies to the one service it names and to nothing else.
    #[test]
    fn an_override_applies_only_to_what_it_names() {
        let mut link = Link::to("10.0.0.1");
        link.ports.insert("ftpsrv".to_owned(), 2122);
        assert_eq!(link.port("ftpsrv", 2121), 2122);
        assert_eq!(link.port("shsrv", 2323), 2323, "untouched");
        assert!(!link.is_plain());
    }

    /// A name nothing overrode gets its own default, not another service's port.
    #[test]
    fn an_unknown_name_gets_its_own_default() {
        let mut link = Link::to("10.0.0.1");
        link.ports.insert("ftpsrv".to_owned(), 2122);
        assert_eq!(link.port("garlic-savemgr", 8082), 8082);
    }
}
