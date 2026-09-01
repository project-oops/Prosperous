//! The services a target may be running, and whether it currently is.
//!
//! # A port number is not a capability
//!
//! A reader told that 3232 is open has been given a worse tool than one told that the
//! kernel log is readable. The third column of the table below is the point of having a
//! table at all: it says what each service *buys*, so a check reads as a list of things
//! that are and are not possible rather than a list of numbers.
//!
//! # Required and optional fail differently
//!
//! Without a loader and somewhere for a report to come back, there is no workflow at all.
//! Without the log, the shell or the dashboard there is still a workflow - it is just
//! harder to see what went wrong inside it, which is a different kind of important and is
//! marked as such rather than blended in.
//!
//! # Why nothing here is cached
//!
//! A jailbreak does not survive a power cycle, and the chain that comes back depends on a
//! text file somebody edited weeks ago. Any stored answer about what a target can do is a
//! claim that expires without notice. So this is asked every time, and a registration
//! holds an address and a name and nothing else.

use std::borrow::Cow;
use std::net::{TcpStream, ToSocketAddrs as _};
use std::time::{Duration, Instant};

/// A service a target may be running, and what its presence buys.
///
/// # Why the strings are borrowed-or-owned
///
/// The five below are compiled in and stay `&'static str`. But a payload list is a **file**,
/// and an entry in it that declares a port describes a service just as much as these do - it
/// simply was not known when this was built.
///
/// Before this, such an entry could only be probed, and its result was kept in a separate map
/// that the verdict never read. So a payload somebody had declared **required** could be
/// missing while the check said *ready*: the tool knew, and the answer did not.
///
/// [`Cow`] is what lets one type carry both without the compiled-in five paying for it. That
/// costs `Copy`, which is why this is `Clone` - the only real consequence of the change.
///
/// # Four flags, and clippy is right to notice
///
/// They are four independent yes-or-no facts about one service - whether it blocks a check,
/// whether it is a way back in, whether it runs startup lists, and whether this program was
/// built knowing about it. Folding them into an enum would make a service pick one role when
/// the manager genuinely has three of them at once.
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
    /// **The property a startup list is audited against.** A chain that leaves none of these
    /// answering leaves a machine nobody can load anything onto - recoverable only by
    /// re-running the jailbreak, and not even that if the chain hung it.
    ///
    /// Moving files is not enough on its own: a file service can put an ELF on the disk and
    /// has no way to run it.
    pub recovers: bool,
    /// Whether this is what runs a startup list once it is up.
    ///
    /// **The failure this catches has happened twice on a real target.** An autoloader list
    /// that does not name this never starts it - and the whole list *this* one would have run
    /// then silently does nothing. Everything somebody configured is simply absent, with no
    /// error anywhere, because the thing that would have reported it never ran either.
    pub runs_lists: bool,
    /// Whether this came from a list rather than from this program.
    ///
    /// **Kept because the two deserve different treatment when they are wrong.** A compiled-in
    /// port was measured against a target; a declared one is somebody's typing, and a wrong one
    /// reports another listener's state under this name. Anything explaining a surprising
    /// finding should be able to say which kind it is looking at.
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
/// # Why it is first rather than alphabetical
///
/// The payload manager launches everything through this, **including itself** - so when
/// this dies, nothing can bring anything back, and the dashboard that would have said so
/// keeps answering because it is a separate listener already running. The only recovery is
/// re-running the jailbreak.
///
/// That makes it the one failure with a different remedy from every other failure here,
/// and a check that reports it last has buried the finding that changes what the reader
/// does next.
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
    /// **Carried rather than discarded, because the two kinds of no are different.** A port
    /// that refuses instantly is a machine saying no; one that takes a second and a half is
    /// usually a network deciding, and a reader who cannot tell them apart will blame the
    /// wrong thing. What counts as slow is the reporting layer business, not this one.
    pub took: Duration,
}

/// Tries to connect, briefly.
///
/// A short timeout on purpose: this is asked of five ports in a row, and a target that is
/// switched off should say so in a couple of seconds rather than a couple of minutes.
///
/// **A refusal is the normal answer** for a payload that is not loaded, so this returns a
/// finding rather than an error - there is nothing here for a caller to handle.
#[must_use]
pub fn probe(address: &str, port: u16, timeout: Duration) -> Reachability {
    let started = Instant::now();
    let open = (address, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .is_some_and(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok());
    let took = started.elapsed();
    // `trace`: one of these runs per service on every check, so at any louder level a single
    // `pros check` would bury whatever else the run had to say.
    tracing::trace!(%address, port, open, ?took, "probed");
    Reachability { open, took }
}

#[cfg(test)]
mod tests {
    use super::{LOADER, SERVICES};

    /// The loader is checked first, because its failure has a different remedy from every
    /// other failure here - re-run the jailbreak, rather than reload a payload.
    #[test]
    fn the_loader_is_first() {
        assert_eq!(SERVICES.first().map(|s| &s.name), Some(&LOADER.name));
    }

    /// Every service says what it unlocks. A table that only carried ports would be a
    /// worse tool than the numbers written down somewhere.
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

    /// There is at least one required service, or a check could report a target fit for
    /// nothing as entirely healthy.
    #[test]
    fn something_is_required() {
        assert!(SERVICES.iter().any(|s| s.required));
    }
}
