//! Whether a startup list leaves a way back in: after the chain runs, can anything on the
//! target still accept a payload? If not, recovery means re-running the entry point.
//!
//! `pldmgr` loads each entry by handing it to `elfldr` on 127.0.0.1:9021 (`ps5_launch_elf` in
//! its source), so every entry after a broken loader fails. `elfldr` serves the boot chain and
//! then exits; listing it last in the manager's own list starts a fresh copy that keeps 9021
//! open for development. Listing it while 9021 already answers makes a second copy that finds
//! the port bound - the one condition [`crate::recovery::can_work_in`] refuses.
//!
//! Findings are hazards reported before a write, not lint advice.

use pros_link::service::Service;

use crate::catalogue::Catalogue;
use crate::chain::Chain;

/// Whether an entry can do anything at all in a list of this kind.
///
/// Anything works in an autoloader's list. In the manager's own list, the list runner never
/// does (a second copy fights the first for its port), and the loader does unless it is
/// already answering on 9021.
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
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Gravity {
    /// Worth knowing, costs visibility rather than access.
    Warning,
    /// This chain can leave the target unreachable.
    Critical,
}

/// Something about a startup list that is worth saying out loud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hazard {
    /// The list re-loads the loader that is loading the list.
    ///
    /// Carries the position, because everything after it is at risk.
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
    /// Nothing in this list starts the thing that runs the other list.
    ///
    /// An autoloader list that does not name the manager never starts it, so the manager's own
    /// list never runs and nothing reports it.
    ChainNeverRuns {
        /// What runs lists on this machine, from the catalogue.
        runner: String,
    },
    /// An entry names a file the manager cannot resolve, or can only resolve sometimes.
    ///
    /// In the manager's source, `payload_mgr_resolve_path` searches `/data/pldmgr` and
    /// `/mnt/usbN/pldmgr` only, while its listing also walks the root of every stick, so it
    /// lists payloads it cannot load.
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
            // Each leaves a target unlike its configuration, with no error saying so.
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
                 way back into this target is re-running the entry point",
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

/// The edit that would put a hazard right: adding or removing one entry.
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
    /// `None` for [`Hazard::NoWayBack`], where any of several would do and the choice is the
    /// person's, and for a removable entry that may be deliberate.
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
            // deliberate.
            Self::OnRemovable { .. } | Self::NoWayBack { .. } => None,
        }
    }
}

/// Which list this is, because the rules differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The manager's own list, which the manager runs after it is already up.
    ///
    /// The manager does not belong in it. Whether the loader may be depends on whether it is
    /// already answering - see [`can_work_in`].
    Manager,
    /// An autoloader's list, run by the entry point before anything else exists.
    ///
    /// It brings up everything, the manager included; without the manager, the manager's own
    /// list never runs.
    Autoloader,
}

impl Kind {
    /// What to call this kind of list, in a sentence about it.
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
/// `kind` is required, not guessed: the same text is safe in one kind of list and fatal in the
/// other. Every service name comes from `known`; no payload name is written into this code.
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

    // Taken from the catalogue, so a target running a different loader is audited against it.
    let loader = known.get(loader_name);
    // A hazard only while the loader answers (a second copy finds 9021 bound) and only with
    // entries after it to pay; last in the list is the deliberate way to keep 9021 open.
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

    // An autoloader list that does not start the list runner leaves the other list unrun.
    if kind == Kind::Autoloader {
        for runner in known.services().iter().filter(|one| one.runs_lists) {
            if named(runner).is_none() {
                found.push(Hazard::ChainNeverRuns {
                    runner: runner.name.to_string(),
                });
            }
        }
    }

    // What can still take a payload once this has run: for an autoloader's list, only what it
    // names; for the manager's, also the running loader unless the list reloads it.
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

    // Every entry checked against where its file is: the manager lists payloads it cannot
    // resolve.
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

    // Baseline requirements from chain.json.
    for req in baseline::required_for_prosperous() {
        if !req.autoloader && kind == Kind::Autoloader {
            continue;
        }
        if !can_work_in(&req.name, kind, known, loader_up) {
            continue;
        }
        if is_satisfied(chain, &req.name, &req.alternatives) {
            continue;
        }
        let gravity = if kind == Kind::Manager {
            Gravity::Warning
        } else {
            req.gravity
        };
        found.push(Hazard::Missing {
            service: req.name,
            unlocks: req.unlocks,
            gravity,
        });
    }

    preset_hazards(chain, known, kind, preset, loader_up, &mut found);

    found.sort_by_key(|hazard| std::cmp::Reverse(hazard.gravity()));
    found
}

/// Whether a service, or one of its alternatives, is already in the chain.
fn is_satisfied(chain: &Chain, name: &str, alternatives: &[String]) -> bool {
    chain.position(name).is_some() || alternatives.iter().any(|alt| chain.position(alt).is_some())
}

/// Audits the preset-specific entries against the chain, pushing any hazards into `found`.
fn preset_hazards(
    chain: &Chain,
    known: &Catalogue,
    kind: Kind,
    preset: &baseline::Preset,
    loader_up: Option<bool>,
    found: &mut Vec<Hazard>,
) {
    for placed in preset.in_order(kind) {
        if found
            .iter()
            .any(|h| matches!(h, Hazard::Missing { service, .. } if service == &placed.name))
        {
            continue;
        }
        // Nothing is reported missing that could not work if it were there.
        if !can_work_in(&placed.name, kind, known, loader_up) {
            continue;
        }
        if is_satisfied(chain, &placed.name, &placed.alternatives) {
            continue;
        }

        let (unlocks, required) = if let Some(service) = known.get(&placed.name) {
            (
                placed
                    .unlocks
                    .as_deref()
                    .unwrap_or(service.unlocks.as_ref())
                    .to_string(),
                placed.required.unwrap_or(service.required),
            )
        } else {
            let note = known.note(&placed.name);
            let unlocks = placed
                .unlocks
                .as_deref()
                .or(note)
                .unwrap_or(placed.why.as_str())
                .to_string();
            let required = placed.required.unwrap_or(false);
            (unlocks, required)
        };

        found.push(Hazard::Missing {
            service: placed.name.clone(),
            unlocks,
            // In an autoloader's list nothing else will provide it. In the manager's, whatever
            // launched the manager may already have.
            gravity: if required && kind == Kind::Autoloader {
                Gravity::Critical
            } else {
                Gravity::Warning
            },
        });
    }
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

    /// The loader (`autoloader: false`) is not reported missing from an autoloader's list.
    #[test]
    fn an_autoloader_list_is_not_told_to_add_the_loader() {
        let preset = crate::recovery::baseline::first();
        let loader = pros_link::service::LOADER.name.as_ref();
        assert!(
            preset
                .entries
                .iter()
                .any(|one| one.name == loader && !one.autoloader),
            "this test is about the entry that is excluded from an autoloader's list"
        );
        let listed = preset
            .in_order(Kind::Autoloader)
            .into_iter()
            .map(|one| one.name)
            .collect::<Vec<_>>()
            .join("\n");
        let found = audit(
            &Chain::parse(&listed),
            &Catalogue::builtin(),
            &[],
            Kind::Autoloader,
            &preset,
            Some(true),
        );
        assert!(
            !found.iter().any(|hazard| matches!(
                hazard,
                Hazard::Missing { service, .. } if service == loader
            )),
            "{found:?}"
        );
    }

    /// A manager list measured on a target, with the loader mid-list.
    const BROKEN: &str = "!3000\nkstuff-lite_v1.09.elf\n!3000\nnanodns.elf\n!3000\n\
                          elfldr_v0.24.elf\n!3000\nShadowMountPlus_1.6beta16.elf\n!3000\n\
                          ps5upload-4.1.2.elf\n!3000\nftpsrv_v0.21.elf\n";

    /// The list from a USB that boots correctly.
    const WORKING: &str = "etaHEN_2.5B.bin\n!2000\nftpsrv_v0.21.1.elf\nshsrv_v0.20.elf\n\
                           elfldr_v0.25.elf\nklogsrv_v0.9.elf\nps5debug-NG_1.3.0.elf\n\
                           pldmgr_v0.5.1.elf\n";

    /// The measured broken list is dangerous, naming the loader, its position and what follows.
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
        // The shell, the log and pltauth-patch are absent too, and reported.
        for wanted in ["shsrv", "klogsrv", "pltauth-patch"] {
            assert!(
                hazards.iter().any(|one| matches!(
                    one,
                    Hazard::Missing { service, .. } if service == wanted
                )),
                "{wanted} is missing and unreported: {hazards:?}"
            );
        }
    }

    /// pltauth-patch is reported and offered when missing from the startup list.
    ///
    /// Native category 0 homebrew needs /dev/pltauth patched to pass `PFAuthClient`
    /// verification (0x80de0051).
    #[test]
    fn pltauth_patch_is_demanded_when_missing_from_startup_list() {
        let without = Chain::parse(
            "!3000\nkstuff-lite_v1.09.elf\n!3000\nnanodns.elf\n!3000\nShadowMountPlus_1.6beta16.elf\n\
             !3000\nps5upload-4.1.2.elf\n!3000\nftpsrv_v0.21.elf\n!3000\nklogsrv_v0.9.elf\n\
             !3000\nshsrv_v0.20.elf\n!3000\nelfldr_v0.24.elf\n",
        );
        let hazards = audit(
            &without,
            &Catalogue::builtin(),
            &[],
            Kind::Manager,
            &super::baseline::first(),
            Some(true),
        );
        let missing = hazards
            .iter()
            .find(
                |one| matches!(one, Hazard::Missing { service, .. } if service == "pltauth-patch"),
            )
            .expect("pltauth-patch is missing and must be reported");
        assert_eq!(missing.gravity(), Gravity::Warning);
        assert!(missing.describe().contains("pltauth-patch"));
        assert!(missing.describe().contains("native Prospero homebrew"));
        assert_eq!(
            missing.fix(),
            Some(super::Fix::Add("pltauth-patch".to_owned()))
        );

        let with = Chain::parse(
            "!3000\nkstuff-lite_v1.09.elf\n!3000\npltauth-patch.elf\n!3000\nnanodns.elf\n\
             !3000\nShadowMountPlus_1.6beta16.elf\n!3000\nps5upload-4.1.2.elf\n!3000\nftpsrv_v0.21.elf\n\
             !3000\nklogsrv_v0.9.elf\n!3000\nshsrv_v0.20.elf\n!3000\nelfldr_v0.24.elf\n",
        );
        let hazards = audit(
            &with,
            &Catalogue::builtin(),
            &[],
            Kind::Manager,
            &super::baseline::first(),
            Some(true),
        );
        assert!(
            !hazards.iter().any(
                |one| matches!(one, Hazard::Missing { service, .. } if service == "pltauth-patch")
            ),
            "pltauth-patch is present and should not be reported missing: {hazards:?}"
        );
    }

    /// An autoloader list without the list runner is critical; adding it clears the hazard.
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

    /// A way back named only in the catalogue satisfies the audit.
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

    /// The gravest hazard is reported first.
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

    /// A working autoloader list raises nothing critical.
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

    /// The same list text gets opposite verdicts as an autoloader's and a manager's list.
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

    /// Every hazard that names one thing carries the edit that fixes it.
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

    /// No way back offers no single fix, since several answers would do.
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

    /// A list leaving no way back names the catalogue's ways back in its remedy.
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
        assert!(no_way.remedy().contains("re-running the entry point"));
        assert!(
            no_way.remedy().contains("elfldr"),
            "the remedy names what would fix it, from the catalogue"
        );
    }

    /// An empty manager list is not dangerous: everything already up stays up.
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

    /// A manager list measured on a target, verbatim.
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

    /// Applying every fix changes the list and clears every critical hazard.
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

    /// An entry the manager can never resolve is critical, and its fix removes it.
    ///
    /// The manager lists stick roots when `SCAN_USB_PAYLOADS` is on but resolves only from its
    /// own folders.
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

    /// A payload in a stick's own manager folder is a warning with no fix.
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

    /// A list whose files are all internal raises no storage hazard.
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
/// Tracked in `data/chain.json`, because an ordering constraint (kstuff before anything needing
/// executable memory) is a fact about the payloads, not one machine. A per-target note in
/// `services.json` still wins.
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
        /// `false` for the loader, which the autoloader has already started. In y2jb's source,
        /// `autoload.js` waits for the loader on 9021 before reading any list, so an entry for
        /// it is a second copy at a bound port.
        #[serde(default = "yes")]
        pub autoloader: bool,
        /// Whether it belongs in the manager's own list at all.
        ///
        /// The mirror of [`Self::autoloader`], `false` only for the manager itself: an entry
        /// telling it to load itself is a second copy fighting the first for its port.
        #[serde(default = "yes")]
        pub manager: bool,
        /// Where it belongs in the manager's own list, when that differs.
        ///
        /// The loader goes last there, so it is still running afterwards holding 9021 open;
        /// one rank cannot also place it for an autoloader's list.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub manager_order: Option<u32>,
        /// What breaks if it runs later - or that nothing does.
        pub why: String,
        /// What becomes possible once this is loaded or answering.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub unlocks: Option<String>,
        /// Whether there is no workflow at all without this.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub required: Option<bool>,
        /// Known alternative payloads that satisfy this entry if present in the chain.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub alternatives: Vec<String>,
    }

    /// A capability or payload declared as required for Prosperous on any target.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
    pub struct Requirement {
        /// The payload's name.
        pub name: String,
        /// Known alternative payloads that satisfy this requirement.
        #[serde(default)]
        pub alternatives: Vec<String>,
        /// Recommended order/rank.
        pub order: u32,
        /// Where it belongs in manager's own list, when that differs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub manager_order: Option<u32>,
        /// Whether it belongs in an autoloader list.
        #[serde(default = "yes")]
        pub autoloader: bool,
        /// Why it is needed.
        pub why: String,
        /// What capability it unlocks.
        pub unlocks: String,
        /// Severity if missing: "critical" or "warning".
        #[serde(default = "default_gravity")]
        pub gravity: super::Gravity,
        /// Whether it is strictly required.
        #[serde(default = "yes")]
        pub required: bool,
    }

    const fn default_gravity() -> super::Gravity {
        super::Gravity::Critical
    }

    /// One way of bringing a target up, named.
    ///
    /// Presets differ in kind: a payload manager chain loads many payloads in order, while an
    /// etaHEN chain loads one that starts most of them itself, so listing those beside it would
    /// start second copies.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
    pub struct Preset {
        /// What to call it.
        pub name: String,
        /// What it is, for somebody choosing between them.
        #[serde(default)]
        pub about: String,
        /// What a person ends up with, in words, before they agree to deploy it.
        ///
        /// Written text, not computed: it states the consequence after a restart, which the
        /// entries alone cannot say.
        #[serde(default)]
        pub result: String,
        /// What goes in it.
        pub entries: Vec<Placed>,
        /// Where this chain keeps its startup lists.
        ///
        /// Declared as data so a person can correct it without a rebuild. A chain that names no
        /// lists contributes none; the chooser offers the union of what chains declare.
        #[serde(default)]
        pub lists: Vec<Held>,
        /// Files this chain carries verbatim, to be put back exactly as they were read.
        ///
        /// Settings beside a list (the manager's own, above all) are carried as a path and
        /// bytes, never parsed, because their format belongs to someone else. [`Capture`]
        /// declares which paths are read. Empty for a shipped preset; filled for one exported
        /// off a target.
        #[serde(default)]
        pub files: Vec<Captured>,
    }

    /// A file a chain carries: where it was read, and the bytes that were there.
    ///
    /// Text, not raw bytes: captured files are `key=value` configuration a person reviews
    /// before deploying.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
    pub struct Captured {
        /// What it is, carried over from the [`Capture`] that named it, for somebody reviewing
        /// the export. Not matched on - the path is the identity.
        #[serde(default)]
        pub label: String,
        /// The full path it was read from, and the full path it will be written back to.
        pub path: String,
        /// The bytes that were there, as text, to be written back exactly.
        pub content: String,
    }

    /// A path worth copying off a target when a chain is written down, declared as data.
    ///
    /// Data rather than a constant, because the set of files worth carrying grows per setup and
    /// is corrected by editing the tracked file, like the lists beside it.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
    pub struct Capture {
        /// What to call it, for somebody reviewing what an export copied.
        pub label: String,
        /// Why it is worth carrying, in the same voice a preset entry's `why` uses.
        #[serde(default)]
        pub why: String,
        /// Every place it may be, highest priority first. `{device}` / `{usb}` are expanded over
        /// a target's removable mounts exactly as a list's `at` is, so a file on a stick is
        /// written once rather than ten times.
        pub at: Vec<String>,
    }

    /// One startup list a chain uses, as the chain declares it.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
    pub struct Held {
        /// What to call it in the chooser, written out in full, `(internal)` and all.
        pub label: String,
        /// Whether it is an autoloader's list rather than the manager's own.
        ///
        /// The two are audited by opposite rules: the loader is kept out of an autoloader's
        /// list, and the list runner is required in it.
        #[serde(default)]
        pub autoloader: bool,
        /// Whether this program will write to it.
        ///
        /// Defaults to read only: a list on removable storage is the way back in when the
        /// internal setup is broken.
        #[serde(default)]
        pub editable: bool,
        /// Every place this list may be, highest priority first.
        ///
        /// The autoloader's documentation gives more than one location, and its meaning on a
        /// removable device has two readings; both are listed. `{device}` is expanded over
        /// every removable mount point a target can have.
        pub at: Vec<String>,
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
        /// Sorted by rank, not file order, and per kind - see [`Placed::manager_order`].
        #[must_use]
        pub fn in_order(&self, kind: super::Kind) -> Vec<Placed> {
            // Entries flagged out of this kind are dropped: see [`Placed::autoloader`] and
            // [`Placed::manager`].
            let mut all: Vec<Placed> = self
                .entries
                .iter()
                .filter(|one| one.autoloader || kind != super::Kind::Autoloader)
                .filter(|one| one.manager || kind != super::Kind::Manager)
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

    /// The default for [`Placed::autoloader`] and [`Placed::manager`]: an entry belongs in both
    /// lists unless it says otherwise.
    const fn yes() -> bool {
        true
    }

    /// The document, which carries its own explanation for whoever opens it.
    #[derive(Debug, Clone, Deserialize)]
    pub struct Document {
        /// Format version of the chains configuration.
        #[serde(default)]
        pub version: Option<u32>,
        /// Baseline payloads required for Prosperous on any target.
        #[serde(default)]
        pub required_for_prosperous: Vec<Requirement>,
        /// Preset startup chains.
        pub presets: Vec<Preset>,
        /// Paths worth copying off a target when a chain is written down.
        ///
        /// Top-level, because an exported target may match no preset. See [`Capture`].
        #[serde(default)]
        pub capture: Vec<Capture>,
    }

    /// Where somebody's own presets go: `chains.json` beside the target registry, read at
    /// startup.
    #[must_use]
    pub fn path() -> Option<std::path::PathBuf> {
        let mut path = crate::target::directory()?;
        path.push("chains.json");
        Some(path)
    }

    /// The parsed document shipped with this binary.
    ///
    /// # Panics
    ///
    /// Panics if the compiled-in `data/chain.json` is not valid; the tests parse the same file.
    #[must_use]
    pub fn document() -> Document {
        let text = include_str!("../data/chain.json");
        serde_json::from_str(text).expect("data/chain.json is part of this crate")
    }

    /// The presets compiled in.
    ///
    /// # Panics
    ///
    /// If the tracked `data/chain.json` is not valid, which no target or caller can cause.
    #[must_use]
    pub fn shipped() -> Vec<Preset> {
        document().presets
    }

    /// Payloads declared as required for Prosperous across all targets.
    #[must_use]
    pub fn required_for_prosperous() -> Vec<Requirement> {
        document().required_for_prosperous
    }

    /// Whether a preset name belongs to a shipped preset that cannot be customized by user files.
    #[must_use]
    pub fn is_shipped_name(name: &str) -> bool {
        shipped()
            .iter()
            .any(|one| one.name.eq_ignore_ascii_case(name))
    }

    /// Every preset: the ones shipped here, with somebody's own custom presets added, and the
    /// reason the user file was not read, if it was not.
    ///
    /// A user preset with a shipped preset's name is ignored.
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
                    if is_shipped_name(&one.name) {
                        continue;
                    }
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

    /// Every path worth copying off a target: the ones shipped here, plus any a person declared
    /// in the `capture` block of their own `chains.json`. User entries add; they never replace.
    #[must_use]
    pub fn captures() -> Vec<Capture> {
        let mut found = document().capture;
        let Some(path) = path() else {
            return found;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return found;
        };
        if let Ok(document) = serde_json::from_str::<Document>(&text) {
            for one in document.capture {
                // By the paths it names, so the same file declared in both files is read once.
                if !found.iter().any(|kept| kept.at == one.at) {
                    found.push(one);
                }
            }
        }
        found
    }

    /// The one used when nobody has chosen, which is the first shipped.
    ///
    /// # Panics
    ///
    /// If the tracked `data/chain.json` ships no presets.
    #[must_use]
    pub fn first() -> Preset {
        shipped()
            .into_iter()
            .next()
            .expect("data/chain.json ships at least one preset")
    }

    /// Writes down a chain a target is running, as a preset, with notes on what it could not
    /// know.
    ///
    /// It records rather than corrects; the audit judges the result. It invents neither an
    /// entry's `why` (carried from a preset that explains it, otherwise marked unwritten) nor
    /// the rank for the other kind of list (kept from a known preset, otherwise the observed
    /// rank stands for both). Each gap is returned as a note.
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

            // The observed rank goes in the field this kind of list governs. Only an entry with
            // a `manager_order` has two ranks, and then the other one is kept, not measured.
            let two_places = known
                .as_ref()
                .is_some_and(|one| one.manager_order.is_some());
            let (order, manager_order, autoloader, manager) = match (kind, known.as_ref()) {
                (super::Kind::Manager, Some(one)) if two_places => {
                    notes.push(format!(
                        "{entry}: this was a manager's list, so where it goes in an \
                         autoloader's list is kept from an existing preset rather than measured",
                    ));
                    (one.order, Some(seen), one.autoloader, one.manager)
                }
                (_, Some(one)) => (seen, one.manager_order, one.autoloader, one.manager),
                (_, None) => (seen, None, true, true),
            };
            let (unlocks, required, alternatives) =
                known.as_ref().map_or((None, None, Vec::new()), |one| {
                    (one.unlocks.clone(), one.required, one.alternatives.clone())
                });
            placed.push(Placed {
                name: entry.clone(),
                order,
                autoloader,
                manager,
                manager_order,
                why,
                unlocks,
                required,
                alternatives,
            });
        }

        let preset = Preset {
            name: name.to_owned(),
            about: format!("Taken from {taken_from}."),
            // A copied chain was not designed, so `result` says only where it came from.
            result: format!(
                "Not written down. This chain was copied from {taken_from} rather than \
                 designed, so what you end up with is whatever that target does. Say what you \
                 know by editing this line in the file."
            ),
            entries: placed,
            // No lists: an exported chain must not redirect where a later deploy writes.
            lists: Vec::new(),
            // This builder reads no target; the caller that has one fills in the captured files
            // (see [`Capture`]).
            files: Vec::new(),
        };
        (preset, notes)
    }

    /// Keeps a preset in somebody's own file, replacing one of that name.
    ///
    /// Only the `presets` array is edited, as untyped JSON, so fields this program does not
    /// model survive the write.
    ///
    /// # Errors
    ///
    /// When the name is a shipped preset's; when there is nowhere to keep it; when the existing
    /// file is not a JSON object with a `presets` array, which is refused rather than replaced;
    /// or when the write fails.
    pub fn keep(preset: &Preset) -> crate::Result<std::path::PathBuf> {
        use crate::Error;
        if is_shipped_name(&preset.name) {
            return Err(Error::failed(format!(
                "'{}' is a built-in chain provided by Prosperous and cannot be overwritten. Choose a custom name for your chain.",
                preset.name
            )));
        }
        let Some(path) = path() else {
            return Err(Error::failed(
                "there is nowhere to keep presets on this machine",
            ));
        };
        let mut document = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<serde_json::Value>(&text).map_err(|why| {
                Error::failed(format!(
                    "{} is not valid JSON, so it was left alone: {why}",
                    path.display()
                ))
            })?,
            // No file yet: a fresh one, carrying the note that says what it is for.
            Err(_) => serde_json::json!({
                "about": FRESH,
                "presets": [],
            }),
        };

        let object = document.as_object_mut().ok_or_else(|| {
            Error::failed(format!("{} is JSON, but not an object", path.display()))
        })?;
        let presets = object
            .entry("presets")
            .or_insert_with(|| serde_json::Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| {
                Error::failed(format!(
                    "{} has `presets`, but it is not an array",
                    path.display()
                ))
            })?;

        let serialised = serde_json::to_value(preset).map_err(|why| {
            Error::failed(format!("could not write {} as JSON: {why}", preset.name))
        })?;
        if let Some(existing) = presets.iter_mut().find(|one| {
            one.get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name == preset.name)
        }) {
            *existing = serialised;
        } else {
            presets.push(serialised);
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|why| {
                Error::failed(format!("{} could not be made: {why}", parent.display()))
            })?;
        }
        let text = serde_json::to_string_pretty(&document)
            .map_err(|why| Error::failed(format!("could not format JSON: {why}")))?;
        std::fs::write(&path, text + "\n")
            .map_err(|why| Error::failed(format!("{} was not written: {why}", path.display())))?;
        Ok(path)
    }

    /// What a file this program creates says about itself, for whoever opens it next.
    const FRESH: [&str; 5] = [
        "YOUR OWN STARTUP CHAINS. Read at startup, so a preset added here needs no rebuild.",
        "Shipped presets cannot be customized or replaced; any name here is a custom preset of your own.",
        "A PRESET NAME IS ONE WORD. It goes on a target's line in the registry as `chain=<name>`, and that file is whitespace-delimited.",
        "`order` is a rank, not an index. Lower runs earlier, and the gaps are so something can be slotted between two entries without renumbering.",
        "`why` should say what breaks if an entry runs later, or say plainly that nothing does. An entry written here by `export chain` says so when nobody has written one.",
    ];

    /// What any preset, or else the baseline requirements, says about one entry, matched by
    /// name. Any preset, because the list being looked at may not come from the selected one.
    #[must_use]
    pub fn about(entry: &str) -> Option<Placed> {
        let in_presets = all().0.into_iter().find_map(|preset| {
            preset.entries.into_iter().find(|placed| {
                crate::chain::Chain::parse(entry)
                    .position(&placed.name)
                    .is_some()
            })
        });
        if in_presets.is_some() {
            return in_presets;
        }
        required_for_prosperous().into_iter().find_map(|req| {
            if crate::chain::Chain::parse(entry)
                .position(&req.name)
                .is_some()
            {
                Some(Placed {
                    name: req.name,
                    order: req.order,
                    autoloader: req.autoloader,
                    // A required payload is never the manager itself, so it always belongs in
                    // the manager's own list.
                    manager: true,
                    manager_order: req.manager_order,
                    why: req.why,
                    unlocks: Some(req.unlocks),
                    required: Some(req.required),
                    alternatives: req.alternatives,
                })
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod baseline_tests {
    use super::baseline;

    /// An exported entry keeps the `why` a shipped preset already gives it.
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

    /// A payload no preset explains is marked unwritten, with a note.
    #[test]
    fn an_entry_nobody_explains_says_nobody_has() {
        let read = ["somebodys-own-payload".to_owned()];
        let (preset, notes) = baseline::from_list("mine", "a target", &read, super::Kind::Manager);
        assert!(preset.entries[0].why.contains("not written down"));
        assert_eq!(notes.len(), 1, "and it is said out loud: {notes:?}");
    }

    /// Exporting a manager's list keeps the loader's unobserved autoloader rank.
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

    /// An exported list comes back in the order it was read.
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

    /// Every shipped entry states why it is where it is.
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

    /// The etaHEN chain does not list what etaHEN starts itself.
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

    /// The two shipped chains differ.
    #[test]
    fn the_shipped_chains_differ() {
        let manager = baseline::first();
        let etahen = baseline::named("etaHEN").expect("it is shipped");
        assert_ne!(manager.name, etahen.name);
        assert!(manager.entries.len() > etahen.entries.len());
    }

    /// The kernel patch precedes the payloads that need executable memory.
    #[test]
    fn the_kernel_patch_comes_before_what_needs_it() {
        let kstuff = baseline::about("kstuff-lite_v1.09.elf").expect("it is in the list");
        let mounter = baseline::about("ShadowMountPlus_1.6beta16.elf").expect("and so is this");
        assert!(kstuff.order < mounter.order, "the patch precedes the user");
        assert!(kstuff.why.contains("executable"), "{}", kstuff.why);
    }

    /// The manager is last in an autoloader's list, because it then runs its own list.
    #[test]
    fn the_manager_is_last() {
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

    /// A versioned filename finds its recommendation.
    #[test]
    fn a_versioned_filename_finds_its_recommendation() {
        assert!(baseline::about("ftpsrv_v0.21.elf").is_some());
        assert!(baseline::about("shsrv_v0.20.elf").is_some());
        assert!(baseline::about("nanodns.elf").is_some());
        assert!(baseline::about("something-nobody-tracked.elf").is_none());
    }

    /// Shipped presets are protected and cannot be overwritten by `keep()`.
    #[test]
    fn shipped_presets_cannot_be_overwritten_by_keep() {
        let preset = baseline::shipped()[0].clone();
        assert!(baseline::is_shipped_name(&preset.name));
        let err = baseline::keep(&preset).expect_err("shipped presets cannot be overwritten");
        assert!(err.to_string().contains("cannot be overwritten"), "{err}");
    }

    /// `data/chain.json` carries a format version and the required payloads.
    #[test]
    fn required_for_prosperous_is_declared_with_version() {
        let doc = baseline::document();
        assert!(
            doc.version.is_some(),
            "data/chain.json must carry a format version"
        );
        let reqs = baseline::required_for_prosperous();
        assert!(
            !reqs.is_empty(),
            "must declare required payloads for prosperous"
        );
        assert!(
            reqs.iter().any(|r| r.name == "pltauth-patch"),
            "must include pltauth-patch"
        );
        assert!(
            reqs.iter().any(|r| r.name == "kstuff-lite"),
            "must include kstuff-lite"
        );
    }

    /// An empty custom preset is still audited for the baseline required payloads.
    #[test]
    fn custom_preset_still_audits_required_for_prosperous() {
        let custom = baseline::Preset {
            name: "my-empty-preset".to_owned(),
            about: "User custom preset with no entries".to_owned(),
            result: "Custom".to_owned(),
            entries: Vec::new(),
            lists: Vec::new(),
            files: Vec::new(),
        };
        let chain = crate::chain::Chain::parse("!3000\nnanodns.elf\n");
        let hazards = super::audit(
            &chain,
            &crate::catalogue::Catalogue::builtin(),
            &[],
            super::Kind::Manager,
            &custom,
            Some(true),
        );
        assert!(
            hazards.iter().any(|h| matches!(h, super::Hazard::Missing { service, .. } if service == "pltauth-patch")),
            "custom preset without pltauth-patch must still demand pltauth-patch: {hazards:?}"
        );
        assert!(
            hazards.iter().any(
                |h| matches!(h, super::Hazard::Missing { service, .. } if service == "kstuff-lite")
            ),
            "custom preset without kstuff must still demand kstuff: {hazards:?}"
        );
    }
}
