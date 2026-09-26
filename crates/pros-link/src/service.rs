//! The services a target may be running, and whether each is answering.
//!
//! Each service records what its presence makes possible, so a check reads as a list of
//! capabilities rather than port numbers. Required services are the ones without which there
//! is no workflow; optional ones only cost visibility.
//!
//! Nothing is cached: the entry point does not survive a power cycle and the chain that
//! comes back depends on an editable list, so every answer is asked for fresh.

use std::borrow::Cow;
use std::net::{TcpStream, ToSocketAddrs as _};
use std::time::{Duration, Instant};

/// A service a target may be running, and what its presence buys.
///
/// The strings are [`Cow`] so compiled-in services and ones declared in a payload list share
/// one type and one verdict. The four flags are independent facts; the manager holds three
/// roles at once, so they are not an enum.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// The payload name, as its own project spells it.
    pub name: Cow<'static, str>,
    /// The port it listens on when loaded.
    pub port: u16,
    /// What becomes possible once it answers.
    pub unlocks: Cow<'static, str>,
    /// Whether there is no workflow without it.
    pub required: bool,
    /// Whether having this running is a way to put a payload on the target.
    ///
    /// A startup list is audited against this: a chain that leaves none of these answering
    /// can only be recovered by re-running the entry point. A file service alone does not
    /// count, since it can place an ELF but not run it.
    pub recovers: bool,
    /// Whether this is what runs a startup list once it is up.
    ///
    /// An autoloader list that does not start this leaves every list it would run silently
    /// unrun, with nothing left to report the absence.
    pub runs_lists: bool,
    /// Whether this came from a list rather than from this program.
    ///
    /// A compiled-in port was measured on a target; a declared one was typed, and a wrong one
    /// reports another listener's state under this name.
    pub declared: bool,
}

impl Service {
    /// A service described by a list rather than by this program.
    #[must_use]
    pub fn declared(
        name: String,
        port: u16,
        unlocks: String,
        required: bool,
        recovers: bool,
        runs_lists: bool,
    ) -> Self {
        Self {
            name: Cow::Owned(name),
            port,
            unlocks: Cow::Owned(unlocks),
            required,
            recovers,
            runs_lists,
            declared: true,
        }
    }
}

/// The loader, and the first thing to check.
///
/// The payload manager launches everything through this, itself included, while its own
/// dashboard keeps answering as a separate listener. When the loader is down the only
/// recovery is re-running the entry point, a remedy unlike any other, so it is checked first.
pub const LOADER: Service = Service {
    name: Cow::Borrowed("elfldr"),
    port: 9021,
    unlocks: Cow::Borrowed("send a payload to the target and run it"),
    required: true,
    recovers: true,
    runs_lists: false,
    declared: false,
};

/// Every service this crate knows how to use, loader first.
pub const SERVICES: &[Service] = &[
    LOADER,
    Service {
        name: Cow::Borrowed("ftpsrv"),
        port: 2121,
        unlocks: Cow::Borrowed("retrieve reports, stage payloads and packages"),
        required: true,
        recovers: false,
        runs_lists: false,
        declared: false,
    },
    Service {
        name: Cow::Borrowed("klogsrv"),
        port: 3232,
        unlocks: Cow::Borrowed(
            "read the system's own log - why a payload died, not just that it did",
        ),
        required: false,
        recovers: false,
        runs_lists: false,
        declared: false,
    },
    Service {
        name: Cow::Borrowed("shsrv"),
        port: 2323,
        recovers: true,
        runs_lists: false,
        unlocks: Cow::Borrowed("run commands on the target without loading a payload"),
        required: false,
        declared: false,
    },
    Service {
        name: Cow::Borrowed("pldmgr"),
        port: 8084,
        recovers: true,
        runs_lists: true,
        unlocks: Cow::Borrowed("inspect and reload the payload chain"),
        required: false,
        declared: false,
    },
];

/// What a single probe found.
#[derive(Debug, Clone, Copy)]
pub struct Reachability {
    /// Whether anything accepted.
    pub open: bool,
    /// How long the answer took.
    ///
    /// An instant refusal is the machine saying no; a slow one is usually the network. The
    /// reporting layer decides what counts as slow.
    pub took: Duration,
}

/// Tries to connect, briefly.
///
/// Asked of every service in a row, so a switched-off target answers in seconds. A refusal
/// is the normal answer for a payload that is not loaded, so this returns a finding, not an
/// error.
#[must_use]
pub fn probe(address: &str, port: u16, timeout: Duration) -> Reachability {
    let started = Instant::now();
    let open = (address, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .is_some_and(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok());
    let took = started.elapsed();
    // `trace`: one runs per service on every check, which would drown a louder level.
    tracing::trace!(%address, port, open, ?took, "probed");
    Reachability { open, took }
}

#[cfg(test)]
mod tests {
    use super::{LOADER, SERVICES};

    /// The loader, whose failure alone needs the entry point re-run, is checked first.
    #[test]
    fn the_loader_is_first() {
        assert_eq!(SERVICES.first().map(|s| &s.name), Some(&LOADER.name));
    }

    /// Every service has a port and says what it unlocks.
    #[test]
    fn every_service_says_what_it_buys() {
        for service in SERVICES {
            assert!(
                !service.unlocks.is_empty(),
                "{} unlocks nothing",
                service.name
            );
            assert!(service.port > 0, "{} has no port", service.name);
        }
    }

    /// Ports are distinct, so a probe result can be attributed.
    #[test]
    fn no_two_services_share_a_port() {
        let mut ports: Vec<u16> = SERVICES.iter().map(|s| s.port).collect();
        ports.sort_unstable();
        let before = ports.len();
        ports.dedup();
        assert_eq!(ports.len(), before, "two services claim the same port");
    }

    /// At least one service is required, so a useless target cannot read as healthy.
    #[test]
    fn something_is_required() {
        assert!(SERVICES.iter().any(|s| s.required));
    }
}
