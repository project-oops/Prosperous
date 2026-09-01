//! Whether a startup list leaves you a way back in.
//!
//! # The question this exists to answer
//!
//! Not *what will be running* - the check already says that. **Whether, after this chain has
//! run, anything on the target can still be given a payload.** A chain that answers no is one
//! where the only recovery is re-running the exploit, and if it hung the machine on the way
//! there, not even that.
//!
//! This has cost a real target its jailbreak twice. It is not a hypothetical.
//!
//! # The mechanism, read from the manager's source
//!
//! `pldmgr` does not load payloads itself. `ps5_launch_elf` opens a socket to
//! **127.0.0.1:9021** and hands the bytes to `elfldr`:
//!
//! ```text
//! server_addr.sin_port = htons(ELFLDR_PORT);
//! server_addr.sin_addr.s_addr = inet_addr("127.0.0.1");
//! ```
//!
//! Two consequences follow, and both are load-bearing:
//!
//! 1. **Every entry after a broken loader fails.** The manager logs *Connection to elfldr
//!    failed* and carries on down the list, achieving nothing.
//! 2. **The loader must never be in the manager's own list.** Sending `elfldr` to `elfldr`
//!    means the running loader spawns a second one, which finds 9021 already bound. Whatever
//!    the outcome, it is at best pointless and at worst the end of the chain - and everything
//!    listed after it is what pays.
//!
//! # The loader's lifetime is the boot chain, not the session
//!
//! This is the piece that was missing, and its absence produced a whole page of wrong reasoning
//! here. **`elfldr` exists to bring the startup payloads up, and then it goes.** Once the chain
//! has run there is nothing on 9021, because the thing that served it has exited.
//!
//! Everything measured on 2026-08-31 falls out of that with nothing left over. A console that
//! had just been brought up by `y2jb`:
//!
//! - loaded seven payloads from the manager's list - `elfldr` was up while that happened;
//! - answered on 8084, 2121, 3232 and 2323 afterwards - those payloads are still running;
//! - **refused 9021, with `elfldr` named nowhere** - it did its job during the chain and closed,
//!   and nothing restarts it.
//!
//! The reading of the manager's source was right. What was wrong was reading a **dependency**
//! where there is a **lifetime**: *the manager loads entries through 9021* is true at the moment
//! the chain runs and says nothing about a minute later.
//!
//! # So what listing the loader in the manager's own list actually does
//!
//! It starts a second one, after the first has gone, and **keeps 9021 open**. That is not a
//! repair and it is not a hazard - it is a choice, and a narrow one:
//!
//! - on an ordinary console it buys nothing. The chain has already run; nothing else is going
//!   to be sent;
//! - on a machine somebody develops against it is the whole point. It is what lets a payload be
//!   sent, run, changed and sent again without re-running the exploit between each attempt.
//!
//! This project is a development tool, so the chains here list it and say why. The one case
//! where it is genuinely wrong is listing it while 9021 is **already** answering - then a second
//! copy finds the port bound - and that is the only condition
//! [`crate::recovery::can_work_in`] refuses on.
//!
//! # Why this is not a lint
//!
//! A lint is advice. This is the difference between a machine you can fix from your desk and
//! one you have to walk over to with a USB stick, so it is stated as a hazard, in the check,
//! in red, before the write rather than after it.

use pros_link::service::Service;

use crate::catalogue::Catalogue;
use crate::chain::Chain;

/// Whether an entry can do anything at all in a list of this kind.
///
/// # The payload manager, in its own list
///
/// Never. The list **is** the thing it reads; an entry telling it to load itself is a second
/// copy fighting the first for a port. That holds whatever else turns out to be true, because
/// it does not depend on how entries are loaded.
///
/// # The loader, in the manager's list - and why this changed
///
/// This used to refuse outright, on the reading that the manager loads every entry by
/// connecting to the loader: listing it would need it already running, pointless if it is and
/// impossible if it is not.
///
/// That mistook a **lifetime** for a **dependency** - see this module's opening. The loader
/// brings the chain up and then exits, so by the time anything asks, the port it served is
/// closed and listing it starts a fresh one rather than colliding with anything.
///
/// So the only condition that matters is the one that can be measured:
///
/// - **it is answering**: an entry for it would send the loader to itself while a copy is
///   already bound to 9021. Refused, and the audit reports it as a hazard.
/// - **it is not answering, or nobody looked**: allowed, and on a development machine wanted -
///   it is what keeps 9021 open so a payload can be sent, run, changed and sent again without
///   re-running the exploit each time.
#[must_use]
pub fn can_work_in(name: &str, kind: Kind, known: &Catalogue, loader_up: Option<bool>) -> bool {
    if kind == Kind::Autoloader {
        return true;
    }
    let is = |other: &str| Chain::parse(name).position(other).is_some();
    if is(pros_link::service::LOADER.name.as_ref()) {
        return loader_up != Some(true);
    }
    !known
        .services()
        .iter()
        .any(|one| one.runs_lists && is(one.name.as_ref()))
}

/// How much trouble a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Gravity {
    /// Worth knowing, costs visibility rather than access.
    Warning,
    /// **This chain can leave the target unreachable.**
    Critical,
}

/// Something about a startup list that is worth saying out loud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hazard {
    /// The list re-loads the loader that is loading the list.
    ///
    /// Carries the position, because everything **after** it is what is at risk.
    ReloadsTheLoader {
        /// What the loader is called on this machine.
        loader: String,
        /// Where in the list it sits.
        at: usize,
        /// How many entries come after it and therefore depend on it surviving.
        after: usize,
    },
    /// Nothing left standing can accept a payload.
    NoWayBack {
        /// What would fix it, named and described, from the catalogue.
        candidates: Vec<(String, String)>,
    },
    /// Nothing in this list starts the thing that runs the *other* list.
    ///
    /// **The measured failure.** An autoloader list that does not name the manager never
    /// starts it, so the manager's own list never runs - and nothing reports that, because the
    /// thing that would have reported it never ran either. Everything somebody configured is
    /// simply absent, silently.
    ChainNeverRuns {
        /// What runs lists on this machine, from the catalogue.
        runner: String,
    },
    /// An entry names a file the manager cannot resolve, or can only resolve sometimes.
    ///
    /// **Measured from the manager's source.** `payload_mgr_resolve_path` searches
    /// `/data/pldmgr` and `/mnt/usbN/pldmgr` only, while its *listing* also walks the root of
    /// every stick - so it shows payloads it can never load, and a list naming one has an entry
    /// that fails at every boot with only a log line to say so.
    OnRemovable {
        /// The entry, as the list spells it.
        entry: String,
        /// Where its file actually is.
        storage: crate::payloads::Where,
    },
    /// A service the catalogue knows about is not in this list.
    Missing {
        /// Which one.
        service: String,
        /// What its absence costs.
        unlocks: String,
        /// How much that matters here.
        gravity: Gravity,
    },
}

impl Hazard {
    /// How much trouble this is.
    #[must_use]
    pub const fn gravity(&self) -> Gravity {
        match self {
            // Each of these ends with a target that is not what somebody configured, and no
            // error anywhere saying so.
            Self::ReloadsTheLoader { .. }
            | Self::NoWayBack { .. }
            | Self::ChainNeverRuns { .. } => Gravity::Critical,
            // Never resolvable is a broken entry; removable is one that works only while a
            // stick is in, which somebody may have chosen on purpose.
            Self::OnRemovable { storage, .. } => {
                if storage.can_autoload() {
                    Gravity::Warning
                } else {
                    Gravity::Critical
                }
            }
            Self::Missing { gravity, .. } => *gravity,
        }
    }

    /// What is wrong, in one line.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::ReloadsTheLoader { loader, at, after } => format!(
                "{loader} is entry {} of this list, and the manager loads every entry through \
                 it - the {after} after it depend on it surviving being sent to itself",
                at + 1
            ),
            Self::NoWayBack { .. } => {
                "nothing left running afterwards can accept a payload".to_owned()
            }
            Self::ChainNeverRuns { runner } => format!(
                "{runner} is not in this list, so it never starts - and the whole list it \
                 would have run does nothing, silently"
            ),
            Self::OnRemovable { entry, storage } => {
                format!("{entry} is {}", storage.means())
            }
            Self::Missing {
                service, unlocks, ..
            } => format!("{service} is not in this list, so afterwards you cannot {unlocks}"),
        }
    }

    /// What to do about it.
    #[must_use]
    pub fn remedy(&self) -> String {
        match self {
            Self::ReloadsTheLoader { loader, .. } => format!(
                "remove {loader} from this list. It is already running - it is what loads \
                 everything else here, and it cannot load itself"
            ),
            Self::NoWayBack { candidates } if candidates.is_empty() => {
                "no service is marked as a way back, so this cannot be checked - mark one in \
                 services.json"
                    .to_owned()
            }
            Self::NoWayBack { candidates } => format!(
                "add at least one of these, and any one is enough:\n{}\nWithout one, the only \
                 way back into this target is re-running the jailbreak",
                candidates
                    .iter()
                    .map(|(name, gives)| format!("  {name} - {gives}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            Self::ChainNeverRuns { runner } => {
                format!("add {runner} to this list, last")
            }
            Self::OnRemovable { entry, storage } if storage.can_autoload() => format!(
                "copy {entry} onto the target's own disk, or accept that this list only works with that storage plugged in"
            ),
            Self::OnRemovable { entry, .. } => format!(
                "remove {entry}, or move its file into the manager's own folder, where the manager can resolve it"
            ),
            Self::Missing { service, .. } => format!("add {service}"),
        }
    }
}

/// The edit that would put a hazard right.
///
/// **Every hazard here is fixable by adding or removing one entry**, and the payloads are
/// already on the target - so telling somebody to go and do it themselves is asking them to
/// retype what this already knows. A finding that can be acted on should come with the action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// Take this service out of the list.
    Remove(String),
    /// Put this service into the list, at the end.
    Add(String),
}

impl Hazard {
    /// The one edit that answers this, when there is one.
    ///
    /// `None` for [`Hazard::NoWayBack`], which is not one edit: **any** of several would do,
    /// and picking for somebody would be choosing how their target boots. The other findings
    /// name exactly one thing.
    #[must_use]
    pub fn fix(&self) -> Option<Fix> {
        match self {
            Self::ReloadsTheLoader { loader, .. } => Some(Fix::Remove(loader.clone())),
            Self::ChainNeverRuns { runner } => Some(Fix::Add(runner.clone())),
            Self::Missing { service, .. } => Some(Fix::Add(service.clone())),
            // An entry the manager can never resolve is dead weight and comes out.
            Self::OnRemovable { entry, storage } if !storage.can_autoload() => {
                Some(Fix::Remove(entry.clone()))
            }
            // One on a stick's own manager folder works while that stick is in, which may be
            // deliberate. Removing somebody's deliberate choice is not a fix.
            Self::OnRemovable { .. } | Self::NoWayBack { .. } => None,
        }
    }
}

/// Which list this is, because the rules differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The manager's own list, which the manager runs after it is already up.
    ///
    /// The manager does not need to be in it. Whether the **loader** may be depends on whether
    /// the loader is already answering - see [`crate::recovery::can_work_in`].
    Manager,
    /// An autoloader's list, run by the jailbreak before anything else exists.
    ///
    /// This one has to bring up everything, the manager included - and if it does not name the
    /// manager, the manager never runs. That is the failure that costs a jailbreak.
    Autoloader,
}

impl Kind {
    /// What to call this kind of list, in a sentence about it.
    ///
    /// **Because there is more than one.** A finding that said *this list* read as a verdict on
    /// the target while being a verdict on one file of several.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Manager => "the manager's own list",
            Self::Autoloader => "the autoloader's list",
        }
    }
}

/// Everything worth saying about a startup list, worst first.
///
/// `kind` decides three of the rules and is required rather than guessed: the same text is
/// safe in one place and fatal in the other. `known` is where every service name comes from -
/// **nothing here has a payload's name written into it.**
#[must_use]
pub fn audit(
    chain: &Chain,
    known: &Catalogue,
    on_target: &[crate::payloads::There],
    kind: Kind,
    preset: &baseline::Preset,
    loader_up: Option<bool>,
) -> Vec<Hazard> {
    let mut found = Vec::new();
    let entries = chain.order().len();
    let named = |service: &Service| chain.position(&service.name);
    let loader_name = pros_link::service::LOADER.name.as_ref();

    // Taken from the catalogue, so a machine running a different loader is audited against
    // that one rather than against the one this was built with.
    let loader = known.get(loader_name);
    // **Only when it is answering, and only when something comes after it.**
    //
    // The risk is a second copy finding 9021 already bound and the entries *after* it paying
    // for the disruption. Both halves are needed and each was wrong on its own:
    //
    // - with nothing answering there is no first copy to collide with;
    // - **with nothing after it, nothing can pay.** Last in the list is the deliberate way to
    //   run it - everything else has loaded by then, and it stays up afterwards holding 9021
    //   open. This reported that configuration as a hazard and said *the 0 after it depend on
    //   it*, a sentence its own carried count disproves.
    let mut reloads = false;
    if kind == Kind::Manager
        && loader_up == Some(true)
        && let Some(loader) = loader
        && let Some(at) = named(loader)
        && entries.saturating_sub(at + 1) > 0
    {
        reloads = true;
        found.push(Hazard::ReloadsTheLoader {
            loader: loader.name.to_string(),
            at,
            after: entries.saturating_sub(at + 1),
        });
    }

    // **The failure that was confirmed on a real target.** An autoloader list that does not
    // start whatever runs lists means the other list never runs, and nothing says so.
    if kind == Kind::Autoloader {
        for runner in known.services().iter().filter(|one| one.runs_lists) {
            if named(runner).is_none() {
                found.push(Hazard::ChainNeverRuns {
                    runner: runner.name.to_string(),
                });
            }
        }
    }

    // **What can still take a payload once this has run.** For an autoloader's list that is
    // only what the list names. For the manager's, the loader is already up - unless the list
    // is about to break it.
    let survives = known.ways_back().into_iter().any(|way| match kind {
        Kind::Autoloader => named(way).is_some(),
        Kind::Manager => named(way).is_some() || (way.name == loader_name && !reloads),
    });
    if !survives {
        found.push(Hazard::NoWayBack {
            candidates: known
                .ways_back()
                .into_iter()
                .map(|way| (way.name.to_string(), way.unlocks.to_string()))
                .collect(),
        });
    }

    // **Every entry checked against where its file actually is.** The manager lists payloads
    // it cannot resolve, so a list can name one and look perfectly reasonable.
    for entry in chain.order() {
        if let Some(one) = on_target
            .iter()
            .find(|one| Chain::parse(&one.name).position(entry).is_some())
            && one.storage != crate::payloads::Where::Internal
        {
            found.push(Hazard::OnRemovable {
                entry: entry.clone(),
                storage: one.storage,
            });
        }
    }

    for service in known.services() {
        // **Nothing is reported missing that could not work if it were there.** The loader is
        // also already reported above where it matters, and saying it twice makes the louder
        // finding easier to miss.
        if !can_work_in(service.name.as_ref(), kind, known, loader_up) {
            continue;
        }
        // **Nothing is reported missing that this target was never meant to run separately.**
        //
        // The catalogue is everything this program knows how to probe. A chain is what somebody
        // decided their console should bring up, and the two differ on purpose: a target set up
        // with etaHEN has the loader, FTP and the kernel log running because etaHEN starts
        // them, and reporting those as missing calls a correct configuration broken - then
        // offers to put a second copy of each beside the first.
        if !preset.entries.iter().any(|placed| {
            Chain::parse(service.name.as_ref())
                .position(&placed.name)
                .is_some()
        }) {
            continue;
        }
        if named(service).is_some() {
            continue;
        }
        found.push(Hazard::Missing {
            service: service.name.to_string(),
            unlocks: service.unlocks.to_string(),
            // In an autoloader's list nothing else will provide it. In the manager's, whatever
            // launched the manager may already have.
            gravity: if service.required && kind == Kind::Autoloader {
                Gravity::Critical
            } else {
                Gravity::Warning
            },
        });
    }

    found.sort_by_key(|hazard| std::cmp::Reverse(hazard.gravity()));
    found
}

/// Whether anything found would leave the target unreachable.
#[must_use]
pub fn is_dangerous(hazards: &[Hazard]) -> bool {
    hazards
        .iter()
        .any(|hazard| hazard.gravity() == Gravity::Critical)
}

#[cfg(test)]
mod tests {
    use super::{Gravity, Hazard, Kind, audit, is_dangerous};
    use crate::catalogue::{Catalogue, Entry};
    use crate::chain::Chain;

    /// The list measured on a real target, which had cost it its jailbreak.
    const BROKEN: &str = "!3000\nkstuff-lite_v1.09.elf\n!3000\nnanodns.elf\n!3000\n\
                          elfldr_v0.24.elf\n!3000\nShadowMountPlus_1.6beta16.elf\n!3000\n\
                          ps5upload-4.1.2.elf\n!3000\nftpsrv_v0.21.elf\n";

    /// The list from a USB that boots correctly.
    const WORKING: &str = "etaHEN_2.5B.bin\n!2000\nftpsrv_v0.21.1.elf\nshsrv_v0.20.elf\n\
                           elfldr_v0.25.elf\nklogsrv_v0.9.elf\nps5debug-NG_1.3.0.elf\n\
                           pldmgr_v0.5.1.elf\n";

    /// **The real list is called dangerous, and says why.**
    ///
    /// The manager sends every entry to the loader on 127.0.0.1:9021, so the loader appearing
    /// in the manager's own list puts everything after it at risk - here, the file service
    /// that is the only way to fix any of it.
    #[test]
    fn the_list_that_broke_a_target_is_reported_as_dangerous() {
        let hazards = audit(
            &Chain::parse(BROKEN),
            &Catalogue::builtin(),
            &[],
            Kind::Manager,
            &super::baseline::first(),
            Some(true),
        );
        assert!(is_dangerous(&hazards), "{hazards:?}");

        let Some(Hazard::ReloadsTheLoader { loader, at, after }) = hazards
            .iter()
            .find(|hazard| matches!(hazard, Hazard::ReloadsTheLoader { .. }))
            .cloned()
        else {
            panic!("the loader reload is the finding: {hazards:?}");
        };
        assert_eq!(loader, "elfldr", "named from the catalogue, not hardcoded");
        assert_eq!(at, 2, "third entry");
        assert_eq!(after, 3, "three entries depend on it surviving");
        // The shell and the log are absent too, and are said as warnings rather than buried.
        for wanted in ["shsrv", "klogsrv"] {
            assert!(
                hazards.iter().any(|one| matches!(
                    one,
                    Hazard::Missing { service, .. } if service == wanted
                )),
                "{wanted} is missing and unreported: {hazards:?}"
            );
        }
    }

    /// **The failure that was confirmed on a real target**: an autoloader list with no
    /// manager in it means the manager never starts, so the manager's own list - everything
    /// somebody configured - never runs, and nothing anywhere says so.
    #[test]
    fn an_autoloader_list_without_the_list_runner_is_critical() {
        let chain = Chain::parse(
            "etaHEN_2.5B.bin\nftpsrv_v0.21.elf\nshsrv_v0.20.elf\n\
                                  elfldr_v0.25.elf\nklogsrv_v0.9.elf\n",
        );
        let hazards = audit(
            &chain,
            &Catalogue::builtin(),
            &[],
            Kind::Autoloader,
            &super::baseline::first(),
            Some(true),
        );
        let never = hazards
            .iter()
            .find(|one| matches!(one, Hazard::ChainNeverRuns { .. }))
            .expect("the manager is absent, so its list never runs");
        assert_eq!(never.gravity(), Gravity::Critical);
        assert!(never.describe().contains("silently"));

        // And the same list *with* it is fine, which is what makes the finding actionable.
        let fixed = Chain::parse(
            "etaHEN_2.5B.bin\nftpsrv_v0.21.elf\nshsrv_v0.20.elf\n\
                                  elfldr_v0.25.elf\nklogsrv_v0.9.elf\npldmgr_v0.5.1.elf\n",
        );
        assert!(!is_dangerous(&audit(
            &fixed,
            &Catalogue::builtin(),
            &[],
            Kind::Autoloader,
            &super::baseline::first(),
            Some(true),
        )));
    }

    /// **A rival named only in the catalogue can satisfy the audit**, which is the property
    /// that keeps these rules from being welded to the five this was built with.
    #[test]
    fn a_rival_named_only_in_the_catalogue_can_make_a_chain_safe() {
        let chain = Chain::parse("someldr_v2.elf\n");
        assert!(is_dangerous(&audit(
            &chain,
            &Catalogue::builtin(),
            &[],
            Kind::Autoloader,
            &super::baseline::first(),
            Some(true),
        )));

        let mut widened = Catalogue::builtin();
        widened.absorb(Entry {
            name: "someldr".to_owned(),
            port: Some(9021),
            unlocks: Some("send a payload and run it".to_owned()),
            recovers: Some(true),
            ..Entry::default()
        });
        let hazards = audit(
            &chain,
            &widened,
            &[],
            Kind::Autoloader,
            &super::baseline::first(),
            Some(true),
        );
        assert!(
            !hazards
                .iter()
                .any(|one| matches!(one, Hazard::NoWayBack { .. })),
            "the catalogue says this is a way back: {hazards:?}"
        );
    }

    /// The worst thing is first, because the one that changes what somebody does must not be
    /// third in a list of six.
    #[test]
    fn the_gravest_hazard_is_reported_first() {
        let hazards = audit(
            &Chain::parse(BROKEN),
            &Catalogue::builtin(),
            &[],
            Kind::Manager,
            &super::baseline::first(),
            Some(true),
        );
        assert_eq!(hazards[0].gravity(), Gravity::Critical);
    }

    /// **The working list raises nothing critical**, which is what makes the check worth
    /// having - a rule that flags everything is a rule nobody reads.
    #[test]
    fn the_list_that_works_is_not_called_dangerous() {
        let hazards = audit(
            &Chain::parse(WORKING),
            &Catalogue::builtin(),
            &[],
            Kind::Autoloader,
            &super::baseline::first(),
            Some(true),
        );
        assert!(!is_dangerous(&hazards), "{hazards:?}");
    }

    /// The loader belongs in an autoloader's list and must not be in the manager's. **Same
    /// text, opposite verdicts**, which is why the kind is a parameter and not a guess.
    #[test]
    fn the_loader_is_required_in_one_list_and_forbidden_in_the_other() {
        let one = "elfldr_v0.25.elf\nftpsrv_v0.21.elf\nshsrv_v0.20.elf\nklogsrv_v0.9.elf\n\
                   pldmgr_v0.5.1.elf\n";
        let chain = Chain::parse(one);
        assert!(!is_dangerous(&audit(
            &chain,
            &Catalogue::builtin(),
            &[],
            Kind::Autoloader,
            &super::baseline::first(),
            Some(true),
        )));
        assert!(is_dangerous(&audit(
            &chain,
            &Catalogue::builtin(),
            &[],
            Kind::Manager,
            &super::baseline::first(),
            Some(true),
        )));
    }

    /// **Every hazard that names one thing carries the edit that fixes it.**
    ///
    /// The payloads are already on the target; making somebody retype what this already knows
    /// is the difference between a tool and a lecture.
    #[test]
    fn the_findings_carry_the_edit_that_fixes_them() {
        use super::Fix;

        let hazards = audit(
            &Chain::parse(BROKEN),
            &Catalogue::builtin(),
            &[],
            Kind::Manager,
            &super::baseline::first(),
            Some(true),
        );
        let fixes: Vec<Fix> = hazards.iter().filter_map(Hazard::fix).collect();
        assert!(
            fixes.contains(&Fix::Remove("elfldr".to_owned())),
            "the loader comes out: {fixes:?}"
        );
        assert!(
            fixes.contains(&Fix::Add("shsrv".to_owned())),
            "the shell goes in: {fixes:?}"
        );
    }

    /// **Except the one where several answers would do.** Picking between them would be
    /// choosing how somebody's target boots, which is not this program's decision.
    #[test]
    fn no_way_back_offers_no_single_fix() {
        let chain = Chain::parse("nanodns.elf\n");
        let hazards = audit(
            &chain,
            &Catalogue::builtin(),
            &[],
            Kind::Autoloader,
            &super::baseline::first(),
            Some(true),
        );
        let no_way = hazards
            .iter()
            .find(|one| matches!(one, Hazard::NoWayBack { .. }))
            .expect("there is no way back");
        assert!(no_way.fix().is_none());
    }

    /// A list with no door left open at all is the worst case, and names the doors.
    #[test]
    fn a_chain_leaving_no_door_open_says_so() {
        let chain = Chain::parse("nanodns.elf\nShadowMountPlus_1.6beta16.elf\n");
        let hazards = audit(
            &chain,
            &Catalogue::builtin(),
            &[],
            Kind::Autoloader,
            &super::baseline::first(),
            Some(true),
        );
        let no_way = hazards
            .iter()
            .find(|one| matches!(one, Hazard::NoWayBack { .. }))
            .expect("there is no way back");
        assert!(no_way.remedy().contains("re-running the jailbreak"));
        assert!(
            no_way.remedy().contains("elfldr"),
            "the remedy names what would fix it, from the catalogue"
        );
    }

    /// An empty manager list is harmless: nothing runs, and everything already up stays up.
    #[test]
    fn an_empty_manager_list_is_not_dangerous() {
        let hazards = audit(
            &Chain::parse(""),
            &Catalogue::builtin(),
            &[],
            Kind::Manager,
            &super::baseline::first(),
            Some(true),
        );
        assert!(!is_dangerous(&hazards), "{hazards:?}");
    }
}

#[cfg(test)]
mod applying {
    use super::{Fix, Hazard, Kind, audit};
    use crate::boot::Boot;
    use crate::catalogue::Catalogue;
    use crate::chain::Chain;

    /// The list measured on the target that lost its jailbreak, verbatim.
    const REAL: &str = "!3000\nkstuff-lite_v1.09.elf\n!3000\nnanodns.elf\n!3000\n\
                        elfldr_v0.24.elf\n!3000\nShadowMountPlus_1.6beta16.elf\n!3000\n\
                        ps5upload-4.1.2.elf\n!3000\nftpsrv_v0.21.elf\n";

    /// The files actually on that target, as its own payload folders hold them.
    const THERE: [&str; 9] = [
        "elfldr_v0.24.elf",
        "ftpsrv_v0.21.elf",
        "klogsrv_v0.9.elf",
        "kstuff-lite_v1.09.elf",
        "nanodns.elf",
        "ps5-app-dumper_v1.11.elf",
        "ps5upload-4.1.2.elf",
        "ShadowMountPlus_1.6beta16.elf",
        "shsrv_v0.20.elf",
    ];

    /// **Every fix actually changes the list.**
    ///
    /// A button that navigates somewhere and silently does nothing is worse than no button:
    /// it spends somebody's trust and leaves the target exactly as dangerous as it was.
    #[test]
    fn applying_every_fix_changes_the_list() {
        let mut boot = Boot::parse(REAL);
        let before = boot.to_text();
        let hazards = audit(
            &Chain::parse(REAL),
            &Catalogue::builtin(),
            &[],
            Kind::Manager,
            &super::baseline::first(),
            Some(true),
        );
        let fixes: Vec<Fix> = hazards.iter().filter_map(Hazard::fix).collect();
        assert!(!fixes.is_empty(), "there is something to fix");

        let mut applied = 0;
        for fix in &fixes {
            match fix {
                Fix::Remove(service) => {
                    let at = Chain::parse(&boot.to_text()).position(service);
                    let at = at.unwrap_or_else(|| panic!("{service} should be found in the list"));
                    assert!(boot.remove(at), "{service} should come out");
                    applied += 1;
                }
                Fix::Add(service) => {
                    let file = THERE
                        .iter()
                        .find(|name| Chain::parse(name).position(service).is_some());
                    if let Some(file) = file {
                        assert!(boot.add(file), "{service} should go in as {file}");
                        applied += 1;
                    }
                }
            }
        }

        assert!(applied > 0, "nothing was applied from {fixes:?}");
        assert_ne!(boot.to_text(), before, "the list is unchanged");

        // And the result is no longer dangerous, which is the point of the button.
        let after = Chain::parse(&boot.to_text());
        let left = audit(
            &after,
            &Catalogue::builtin(),
            &[],
            Kind::Manager,
            &super::baseline::first(),
            Some(true),
        );
        assert!(
            !super::is_dangerous(&left),
            "still dangerous after fixing: {left:?}"
        );
    }
}

#[cfg(test)]
mod storage_tests {
    use super::{Fix, Gravity, Hazard, Kind, audit, is_dangerous};
    use crate::catalogue::Catalogue;
    use crate::chain::Chain;
    use crate::payloads::{There, Where};

    fn at(name: &str, storage: Where) -> There {
        There {
            name: name.to_owned(),
            path: format!("/wherever/{name}"),
            about: None,
            storage,
        }
    }

    /// **An entry the manager can never resolve is critical, and comes out.**
    ///
    /// Measured from the manager's source: it lists payloads from the root of a stick when
    /// `SCAN_USB_PAYLOADS` is on, and resolves only from its own folders. So a list can name
    /// one, look perfectly reasonable, and fail at every boot with a log line nobody reads.
    #[test]
    fn an_entry_the_manager_cannot_resolve_is_critical() {
        let chain = Chain::parse("ftpsrv_v0.21.elf\nshsrv_v0.20.elf\nsomething_v1.elf\n");
        let there = [
            at("ftpsrv_v0.21.elf", Where::Internal),
            at("shsrv_v0.20.elf", Where::Internal),
            at("something_v1.elf", Where::Unreachable),
        ];
        let hazards = audit(
            &chain,
            &Catalogue::builtin(),
            &there,
            Kind::Manager,
            &super::baseline::first(),
            Some(true),
        );
        let found = hazards
            .iter()
            .find(|one| matches!(one, Hazard::OnRemovable { .. }))
            .expect("the unreachable entry is reported");
        assert_eq!(found.gravity(), Gravity::Critical);
        assert_eq!(
            found.fix(),
            Some(Fix::Remove("something_v1".to_owned())),
            "dead weight comes out"
        );
        assert!(is_dangerous(&hazards));
    }

    /// **A payload in a stick's own manager folder is a warning, not a verdict**, and carries
    /// no fix: it works while that stick is in, which somebody may have chosen deliberately.
    #[test]
    fn a_payload_on_a_stick_is_a_warning_with_no_fix() {
        let chain = Chain::parse("ftpsrv_v0.21.elf\nshsrv_v0.20.elf\n");
        let there = [
            at("ftpsrv_v0.21.elf", Where::Internal),
            at("shsrv_v0.20.elf", Where::Removable),
        ];
        let hazards = audit(
            &chain,
            &Catalogue::builtin(),
            &there,
            Kind::Manager,
            &super::baseline::first(),
            Some(true),
        );
        let found = hazards
            .iter()
            .find(|one| matches!(one, Hazard::OnRemovable { .. }))
            .expect("the removable entry is reported");
        assert_eq!(found.gravity(), Gravity::Warning);
        assert!(found.fix().is_none(), "not this program's choice to make");
    }

    /// Everything on the target's own disk says nothing at all, which is what keeps the
    /// warning worth reading.
    #[test]
    fn an_internal_list_raises_nothing_about_storage() {
        let chain = Chain::parse("ftpsrv_v0.21.elf\nshsrv_v0.20.elf\n");
        let there = [
            at("ftpsrv_v0.21.elf", Where::Internal),
            at("shsrv_v0.20.elf", Where::Internal),
        ];
        let hazards = audit(
            &chain,
            &Catalogue::builtin(),
            &there,
            Kind::Manager,
            &super::baseline::first(),
            Some(true),
        );
        assert!(
            !hazards
                .iter()
                .any(|one| matches!(one, Hazard::OnRemovable { .. })),
            "{hazards:?}"
        );
    }
}

/// The recommended startup order, and why each entry is where it is.
///
/// # Why this is a file in the repository
///
/// An ordering constraint is a **fact about how these payloads work**, not a preference about
/// one machine: kstuff has to precede anything needing executable memory on every target there
/// has ever been. So it is tracked here, reviewed like anything else, and the same for
/// everybody - rather than typed into a window where it would live on one machine and be lost
/// with it.
///
/// A per-target note in `services.json` still wins, for the cases where somebody's setup
/// genuinely differs.
pub mod baseline {
    use serde::{Deserialize, Serialize};

    /// What the tracked list says about one payload.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
    pub struct Placed {
        /// The payload's name, matched with a version allowed on the end.
        pub name: String,
        /// Where it belongs, as a rank. Lower runs earlier.
        pub order: u32,
        /// Whether it belongs in an autoloader's list at all.
        ///
        /// **`false` for the loader, from the autoloader's own README**: *"Do NOT include the
        /// kernel exploit (e.g. `lapse.js`) or the `elf_loader` in `autoload.txt`; they are
        /// loaded automatically."* An entry for it there is a second copy of something already
        /// running - and this file said to put it early, on the reasoning that everything after
        /// it loads through it. That is true, and it is the autoloader's job rather than the
        /// list's.
        #[serde(default = "yes")]
        pub autoloader: bool,
        /// Where it belongs in **the manager's own list**, when that differs.
        ///
        /// # Why one entry needs two positions
        ///
        /// The loader. In an autoloader's list it goes early, because it is what makes 9021
        /// exist for everything after it. In the manager's own list it goes **last**, for the
        /// opposite reason: everything else has already loaded by then, and what it is there
        /// for is to still be running afterwards.
        ///
        /// One rank cannot say both, and picking either one makes the other list wrong.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub manager_order: Option<u32>,
        /// What breaks if it runs later - or that nothing does.
        pub why: String,
    }

    /// One way of bringing a target up, named.
    ///
    /// # Why more than one
    ///
    /// **There is more than one way to bring a PS5 up, and they are not variations.** A payload
    /// manager chain loads a dozen separate payloads in an order that matters. An etaHEN chain
    /// loads one payload that already contains most of them - so advice written for the first
    /// is not merely imprecise for the second, it is wrong: it would list beside etaHEN the
    /// very things etaHEN starts, and a second FTP server fighting the first for 2121 is not a
    /// better configuration than none.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
    pub struct Preset {
        /// What to call it.
        pub name: String,
        /// What it is, for somebody choosing between them.
        #[serde(default)]
        pub about: String,
        /// **What a person ends up with**, in their words, before they agree to deploy it.
        ///
        /// # Why this is text and not something computed
        ///
        /// The program prints it and understands none of it. A description assembled from the
        /// entries could only ever say *these payloads, in this order* - which is the plan,
        /// and the plan is already on the screen. What somebody needs before agreeing is the
        /// consequence: what the console does after a restart, and what they give up. That is
        /// a judgement about the chain, so it is written down beside the chain, and a chain
        /// added to the file without touching this program explains itself.
        #[serde(default)]
        pub result: String,
        /// What goes in it.
        pub entries: Vec<Placed>,
    }

    impl Placed {
        /// Where this belongs in a list of that kind.
        #[must_use]
        pub const fn rank(&self, kind: super::Kind) -> u32 {
            match (kind, self.manager_order) {
                (super::Kind::Manager, Some(instead)) => instead,
                _ => self.order,
            }
        }
    }

    impl Preset {
        /// Its entries, earliest first, for a list of this kind.
        ///
        /// **The rank is a rank, not an index**, so this sorts rather than trusting the order
        /// the file happens to be written in - a line moved by hand should change nothing. And
        /// it is asked per kind, because at least one entry belongs in a different place
        /// depending on which list it is going into - see [`Placed::manager_order`].
        #[must_use]
        pub fn in_order(&self, kind: super::Kind) -> Vec<Placed> {
            // **An entry can be excluded from one kind outright.** Only the loader is, and only
            // from an autoloader's list, because that autoloader already loads it - see
            // [`Placed::autoloader`].
            let mut all: Vec<Placed> = self
                .entries
                .iter()
                .filter(|one| one.autoloader || kind != super::Kind::Autoloader)
                .cloned()
                .collect();
            all.sort_by(|left, right| {
                left.rank(kind)
                    .cmp(&right.rank(kind))
                    .then_with(|| left.name.cmp(&right.name))
            });
            all
        }
    }

    /// The default for [`Placed::autoloader`]: an entry belongs in both lists unless it says
    /// otherwise, because that is true of every entry but one.
    const fn yes() -> bool {
        true
    }

    /// The document, which carries its own explanation for whoever opens it.
    #[derive(Debug, Clone, Deserialize)]
    struct Document {
        presets: Vec<Preset>,
    }

    /// Where somebody's own presets go.
    ///
    /// Beside the target registry, like everything else this program keeps - and read at
    /// startup, so a preset can be added or corrected without a rebuild.
    #[must_use]
    pub fn path() -> Option<std::path::PathBuf> {
        let mut path = crate::target::directory()?;
        path.push("chains.json");
        Some(path)
    }

    /// The presets compiled in.
    ///
    /// # Panics
    ///
    /// If the file in this repository is not valid. That is a build-time mistake in a file
    /// under version control, not a condition a target can produce, so it is not an error a
    /// caller could do anything about.
    #[must_use]
    pub fn shipped() -> Vec<Preset> {
        let text = include_str!("../data/chain.json");
        let document: Document =
            serde_json::from_str(text).expect("data/chain.json is part of this crate");
        document.presets
    }

    /// Every preset: the ones shipped here, with somebody's own file laid over the top.
    ///
    /// **Replaced by name, never merged entry by entry.** A preset is an ordering that has to
    /// hold as a whole, and half of somebody's chain interleaved with half of this one is a
    /// chain nobody designed. Naming a shipped preset replaces it outright; any other name is
    /// a preset of their own, and both sit in the same list afterwards.
    ///
    /// A file that cannot be read is no file. This is a set of recommendations, and refusing to
    /// start over one would be absurd - but it is **said**, because a preset somebody wrote and
    /// this quietly ignored is worse than one it refused out loud.
    #[must_use]
    pub fn all() -> (Vec<Preset>, Option<String>) {
        let mut presets = shipped();
        let Some(path) = path() else {
            return (presets, None);
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return (presets, None);
        };
        match serde_json::from_str::<Document>(&text) {
            Ok(document) => {
                for one in document.presets {
                    if let Some(existing) = presets.iter_mut().find(|kept| kept.name == one.name) {
                        *existing = one;
                    } else {
                        presets.push(one);
                    }
                }
                (presets, None)
            }
            Err(why) => (
                presets,
                Some(format!("{} was not read: {why}", path.display())),
            ),
        }
    }

    /// One preset by name.
    #[must_use]
    pub fn named(name: &str) -> Option<Preset> {
        all().0.into_iter().find(|one| one.name == name)
    }

    /// The one used when nobody has chosen, which is the first shipped.
    ///
    /// # Panics
    ///
    /// If this repository ships no presets at all, which would be a build-time mistake in a
    /// tracked file rather than anything a target or a person could produce.
    #[must_use]
    pub fn first() -> Preset {
        shipped()
            .into_iter()
            .next()
            .expect("data/chain.json ships at least one preset")
    }

    /// Writes down a chain a target is actually running, as a preset.
    ///
    /// # Why a recording instrument rather than a generator
    ///
    /// The shipped `payload-manager` preset was **taken from a working console**, because a
    /// chain is a thing that runs somewhere and the honest way to write one down is to copy one
    /// that does. That was done by hand, once, by reading a list off a target and typing it in.
    /// This is that job done by the program, so somebody with a console that works can keep
    /// what it does instead of retyping it.
    ///
    /// # What it will not invent
    ///
    /// Two things, and it says so rather than filling them in:
    ///
    /// - **`why`.** This file's own rule is that it says what breaks if an entry runs later, or
    ///   states plainly that nothing does. That is a judgement about the payload, not something
    ///   a position in a list can produce. Where a preset already explains an entry its words
    ///   are carried over; where none does, the entry says nobody has written one.
    /// - **The rank belonging to the other kind of list.** A manager's list was observed at
    ///   manager positions and says nothing about where those payloads go in an autoloader's. A
    ///   known preset's rank is kept for that; where none is known the observed rank stands for
    ///   both, which is the assumption every ordinary entry already makes.
    ///
    /// Both come back as notes beside the preset, because an export somebody believes is
    /// complete is worse than one that lists what it could not know.
    ///
    /// # It records rather than corrects
    ///
    /// If a target's autoloader list names the loader - which the autoloader's own README says
    /// never to do - that is what gets written down, because this answers *what does this
    /// console do*. The audit is what says whether it should, and it is still there to say it.
    #[must_use]
    pub fn from_list(
        name: &str,
        taken_from: &str,
        entries: &[String],
        kind: super::Kind,
    ) -> (Preset, Vec<String>) {
        let mut notes = Vec::new();
        let mut placed = Vec::new();
        for (at, entry) in entries.iter().enumerate() {
            // Ranks with gaps, as the shipped file has them, so something can be slotted
            // between two of these without renumbering.
            let seen = u32::try_from(at).unwrap_or_default().saturating_add(1) * 10;
            let known = about(entry);
            let why = known.as_ref().map_or_else(
                || {
                    notes.push(format!(
                        "{entry}: no preset explains it, so its `why` says nobody has written one",
                    ));
                    "not written down. This entry was copied from a target's list, and nothing \
                     here knows what breaks if it runs later."
                        .to_owned()
                },
                |one| one.why.clone(),
            );

            // **The observed rank goes in whichever field the kind of list actually governs.**
            // For every ordinary entry those are the same field; only an entry a preset marks
            // as belonging in two places has to have them told apart, and only then is there a
            // rank here that was not measured.
            let two_places = known
                .as_ref()
                .is_some_and(|one| one.manager_order.is_some() || !one.autoloader);
            let (order, manager_order, autoloader) = match (kind, known.as_ref()) {
                (super::Kind::Manager, Some(one)) if two_places => {
                    notes.push(format!(
                        "{entry}: this was a manager's list, so where it goes in an \
                         autoloader's list is kept from an existing preset rather than measured",
                    ));
                    (one.order, Some(seen), one.autoloader)
                }
                (_, Some(one)) => (seen, one.manager_order, one.autoloader),
                (_, None) => (seen, None, true),
            };
            placed.push(Placed {
                name: entry.clone(),
                order,
                autoloader,
                manager_order,
                why,
            });
        }

        let preset = Preset {
            name: name.to_owned(),
            about: format!("Taken from {taken_from}."),
            // **Left saying it is not written down.** `result` is what somebody is told they
            // will end up with before they agree to deploy it. A chain copied off a console has
            // not been designed, so the only true thing to say about it is where it came from -
            // and a sentence made up here would be read as a promise about a restart.
            result: format!(
                "Not written down. This chain was copied from {taken_from} rather than \
                 designed, so what you end up with is whatever that target does. Say what you \
                 know by editing this line in the file."
            ),
            entries: placed,
        };
        (preset, notes)
    }

    /// Keeps a preset in somebody's own file, replacing one of that name.
    ///
    /// # Why the rest of the file is read as text and put back untouched
    ///
    /// The shipped file carries an `about` block that nothing in this program models, and
    /// somebody's own file may carry anything else. Reading it into the document this module
    /// deserialises and writing that back
    /// would silently drop every field this program does not know about - the same
    /// failure as a list entry naming a file nobody can resolve, arriving in somebody's own
    /// configuration instead of on a console.
    ///
    /// So the array is edited and everything around it is left exactly as it was.
    ///
    /// # Errors
    ///
    /// When there is nowhere to keep it; when the existing file is not JSON, which is refused
    /// rather than replaced, because overwriting a file somebody typed by hand is not a repair;
    /// or when the write fails.
    pub fn keep(preset: &Preset) -> Result<std::path::PathBuf, String> {
        let Some(path) = path() else {
            return Err("there is nowhere to keep presets on this machine".to_owned());
        };
        let mut document = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<serde_json::Value>(&text).map_err(|why| {
                format!(
                    "{} is not valid JSON, so it was left alone: {why}",
                    path.display()
                )
            })?,
            // No file yet: a fresh one, carrying the note that says what it is for.
            Err(_) => serde_json::json!({
                "about": FRESH,
                "presets": [],
            }),
        };

        let object = document
            .as_object_mut()
            .ok_or_else(|| format!("{} is JSON, but not an object", path.display()))?;
        let presets = object
            .entry("presets")
            .or_insert_with(|| serde_json::Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("`presets` in {} is not a list", path.display()))?;
        let written = serde_json::to_value(preset).map_err(|why| why.to_string())?;
        let same_name = presets.iter().position(|one| {
            one.get("name").and_then(serde_json::Value::as_str) == Some(preset.name.as_str())
        });
        match same_name {
            Some(at) => presets[at] = written,
            None => presets.push(written),
        }

        let text = serde_json::to_string_pretty(&document).map_err(|why| why.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|why| format!("{} could not be made: {why}", parent.display()))?;
        }
        std::fs::write(&path, text + "\n")
            .map_err(|why| format!("{} was not written: {why}", path.display()))?;
        Ok(path)
    }

    /// What a file this program creates says about itself, for whoever opens it next.
    const FRESH: [&str; 5] = [
        "YOUR OWN STARTUP CHAINS. Read at startup, so a preset added here needs no rebuild.",
        "A preset named the same as one this program ships replaces it; any other name is a preset of your own.",
        "A PRESET NAME IS ONE WORD. It goes on a target's line in the registry as `chain=<name>`, and that file is whitespace-delimited.",
        "`order` is a rank, not an index. Lower runs earlier, and the gaps are so something can be slotted between two entries without renumbering.",
        "`why` should say what breaks if an entry runs later, or say plainly that nothing does. An entry written here by `export chain` says so when nobody has written one.",
    ];

    /// What some preset says about one entry, matched by name.
    ///
    /// **Any of them**, because this answers *why is this payload here* for a list somebody is
    /// looking at, and that list was not necessarily built from the preset they have selected -
    /// it may not have been built from any of them.
    #[must_use]
    pub fn about(entry: &str) -> Option<Placed> {
        all().0.into_iter().find_map(|preset| {
            preset.entries.into_iter().find(|placed| {
                crate::chain::Chain::parse(entry)
                    .position(&placed.name)
                    .is_some()
            })
        })
    }
}

#[cfg(test)]
mod baseline_tests {
    use super::baseline;

    /// **A chain read off a target keeps the words somebody already wrote for its entries.**
    ///
    /// The alternative is an exported preset where every `why` says nothing was known, which
    /// would be true of the export and false of the payloads - the reasons exist, they are in
    /// the file this is being written beside.
    #[test]
    fn an_exported_entry_carries_the_reason_a_preset_already_gives_it() {
        let read = ["kstuff-lite".to_owned(), "ftpsrv".to_owned()];
        let (preset, notes) = baseline::from_list("mine", "a target", &read, super::Kind::Manager);
        assert_eq!(preset.name, "mine");
        for entry in &preset.entries {
            let already = baseline::about(&entry.name).expect("both are in a shipped preset");
            assert_eq!(entry.why, already.why, "{} kept its reason", entry.name);
        }
        assert!(notes.is_empty(), "nothing here was unknown: {notes:?}");
    }

    /// **A payload no preset explains says so, rather than being given a reason.**
    #[test]
    fn an_entry_nobody_explains_says_nobody_has() {
        let read = ["somebodys-own-payload".to_owned()];
        let (preset, notes) = baseline::from_list("mine", "a target", &read, super::Kind::Manager);
        assert!(preset.entries[0].why.contains("not written down"));
        assert_eq!(notes.len(), 1, "and it is said out loud: {notes:?}");
    }

    /// **The rank that was not observed is kept, not overwritten with the one that was.**
    ///
    /// The loader sits last in the manager's own list and early in an autoloader's. Reading a
    /// manager's list measures the first and says nothing about the second, so an export that
    /// wrote the observed position into both would turn a correct preset into one that puts the
    /// loader last in an autoloader's list - where the autoloader's README says it should not
    /// appear at all.
    #[test]
    fn exporting_a_managers_list_does_not_claim_an_autoloader_position() {
        let shipped = baseline::about("elfldr").expect("the loader is in a shipped preset");
        let read = [
            "kstuff-lite".to_owned(),
            "ftpsrv".to_owned(),
            "elfldr".to_owned(),
        ];
        let (preset, notes) = baseline::from_list("mine", "a target", &read, super::Kind::Manager);
        let loader = preset
            .entries
            .iter()
            .find(|one| one.name == "elfldr")
            .expect("it was in the list");
        assert_eq!(loader.order, shipped.order, "the unobserved rank is kept");
        assert_eq!(loader.manager_order, Some(30), "the observed one is third");
        assert!(
            !loader.autoloader,
            "and it still stays out of an autoloader's list"
        );
        assert!(
            notes.iter().any(|note| note.contains("elfldr")),
            "said out loud: {notes:?}"
        );
    }

    /// **The list a target loads is the list that comes back**, in the same order.
    #[test]
    fn the_order_read_is_the_order_written() {
        let read = [
            "ftpsrv".to_owned(),
            "klogsrv".to_owned(),
            "shsrv".to_owned(),
        ];
        let (preset, _) = baseline::from_list("mine", "a target", &read, super::Kind::Manager);
        let back = preset
            .in_order(super::Kind::Manager)
            .into_iter()
            .map(|one| one.name)
            .collect::<Vec<_>>();
        assert_eq!(back, read);
    }

    /// **Every entry says what breaks, or says that nothing does.**
    ///
    /// The second is the normal case and has to be stated: a file where every line invents a
    /// constraint is a file nobody believes, and the lines that matter get lost among them.
    #[test]
    fn every_recommendation_carries_its_reason() {
        let presets = baseline::shipped();
        assert!(presets.len() > 1, "more than one way to bring a target up");
        for preset in &presets {
            assert!(!preset.entries.is_empty(), "{} is empty", preset.name);
            assert!(
                preset.about.len() > 30,
                "{} does not say what it is",
                preset.name
            );
            for placed in &preset.entries {
                assert!(
                    placed.why.len() > 30,
                    "{} in {} does not say why it is where it is",
                    placed.name,
                    preset.name
                );
            }
        }
    }

    /// **A preset that contains a payload does not also list it beside itself.**
    ///
    /// This is the whole reason presets exist rather than one list with exceptions: etaHEN
    /// starts the loader, FTP and the kernel log itself, and a chain that lists those beside it
    /// is a second copy of each fighting the first for a port.
    #[test]
    fn the_etahen_chain_does_not_list_what_etahen_already_starts() {
        let etahen = baseline::named("etaHEN").expect("it is a shipped preset");
        for absent in ["elfldr", "ftpsrv", "klogsrv", "kstuff"] {
            assert!(
                !etahen.entries.iter().any(|one| one.name == absent),
                "{absent} is in the etaHEN chain, which already starts it"
            );
        }
        assert_eq!(
            etahen
                .in_order(super::Kind::Autoloader)
                .first()
                .map(|one| one.name.as_str()),
            Some("etaHEN"),
            "nothing precedes it"
        );
    }

    /// The two shipped chains are genuinely different, not one renamed.
    #[test]
    fn the_shipped_chains_differ() {
        let manager = baseline::first();
        let etahen = baseline::named("etaHEN").expect("it is shipped");
        assert_ne!(manager.name, etahen.name);
        assert!(manager.entries.len() > etahen.entries.len());
    }

    /// **The one the whole file exists for.** kstuff patches the kernel so unsigned code can
    /// run, so it precedes everything that needs that - which is measured, not assumed.
    #[test]
    fn the_kernel_patch_comes_before_what_needs_it() {
        let kstuff = baseline::about("kstuff-lite_v1.09.elf").expect("it is in the list");
        let mounter = baseline::about("ShadowMountPlus_1.6beta16.elf").expect("and so is this");
        assert!(kstuff.order < mounter.order, "the patch precedes the user");
        assert!(kstuff.why.contains("executable"), "{}", kstuff.why);
    }

    /// The manager goes last, because it runs a list of its own once it is up.
    #[test]
    fn the_manager_is_last() {
        // **Of an autoloader's list**, which is the list the claim is about: the manager runs
        // a list of its own once it is up, so anything that list is going to do should be done
        // before it starts. Asked of the entries rather than of a list, this compared ranks
        // that belong to two different files - and the loader's, which is only ever in the
        // other one.
        for preset in baseline::shipped() {
            let listed = preset.in_order(super::Kind::Autoloader);
            let Some(at) = listed.iter().position(|one| one.name == "pldmgr") else {
                continue;
            };
            assert_eq!(
                at,
                listed.len() - 1,
                "{} recommends something after the manager: {:?}",
                preset.name,
                listed.iter().map(|one| &one.name).collect::<Vec<_>>()
            );
        }
    }

    /// **Matched with a version on the end**, the same as everywhere else these names are
    /// compared - a recommendation that only matched a bare name would match nothing real.
    #[test]
    fn a_versioned_filename_finds_its_recommendation() {
        assert!(baseline::about("ftpsrv_v0.21.elf").is_some());
        assert!(baseline::about("shsrv_v0.20.elf").is_some());
        assert!(baseline::about("nanodns.elf").is_some());
        assert!(baseline::about("something-nobody-tracked.elf").is_none());
    }
}
