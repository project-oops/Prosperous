//! What is described, what is trustworthy, and what is on the target.
//!
//! Lives here rather than in a window so the command line and the window give the same
//! answer. A payload nothing here can see is not absent: only services with a known port (or
//! a port the manifest declares) are measured, and everything else is [`Presence::Unknown`].

use std::collections::BTreeMap;

use pros_link::service::SERVICES;

use crate::chain::Chain;
use crate::check::Report;
use crate::checksum::Unreadable;
use crate::manifest::{Manifest, Payload};

/// Whether a payload can be checked before it is run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trust {
    /// It states a digest this can verify.
    Verifiable,
    /// It does not, and this is why.
    ///
    /// Carries the reason because no checksum at all and a digest in an unsupported algorithm
    /// need different work.
    Doubtful(Unreadable),
}

impl Trust {
    /// Whether it can be verified at all.
    #[must_use]
    pub const fn is_verifiable(&self) -> bool {
        matches!(self, Self::Verifiable)
    }
}

/// Whether the payload is running on the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// Its service answered.
    Loaded,
    /// Its service did not answer.
    NotLoaded,
    /// Nothing here can tell: no check has been run, or this payload has no known port.
    ///
    /// Distinct from [`Presence::NotLoaded`], which is a measurement.
    Unknown,
}

/// Whether the payload will be there after the next power cycle.
///
/// A different question from [`Presence`]: a service can answer now and be absent from the
/// boot list, so it is gone after the next power cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boot {
    /// It is in the boot list, at this position.
    At(usize),
    /// The boot list was read and does not name it.
    NotInList,
    /// The boot list was not read, so nothing is known about what it names.
    Unknown,
}

/// One payload, described and located.
#[derive(Debug, Clone)]
pub struct Row<'a> {
    /// What the manifest says about it.
    pub payload: &'a Payload,
    /// Whether it can be verified before being run.
    pub trust: Trust,
    /// Whether it is on the target.
    pub presence: Presence,
    /// Whether it will be after a reboot.
    pub boot: Boot,
}

/// Puts a manifest beside a check.
///
/// `report` and `chain` are optional: a manifest can be read with no target present, and then
/// every row says unknown.
#[must_use]
pub fn survey<'a>(
    manifest: &'a Manifest,
    report: Option<&Report>,
    chain: Option<&Chain>,
) -> Vec<Row<'a>> {
    manifest
        .payloads()
        .iter()
        .map(|payload| Row {
            payload,
            trust: match payload.checksum() {
                Ok(_) => Trust::Verifiable,
                Err(why) => Trust::Doubtful(why),
            },
            presence: presence_of(&payload.name, report),
            boot: chain.map_or(Boot::Unknown, |chain| {
                chain
                    .position(&payload.name)
                    .map_or(Boot::NotInList, Boot::At)
            }),
        })
        .collect()
}

/// Groups a survey the way the repository groups itself.
///
/// The repository's own categories travel with the data and match every other tool that
/// reads the same file. Entries with no category go in a group of their own, last.
#[must_use]
pub fn by_category<'a, 'p>(rows: &'a [Row<'p>]) -> Vec<(&'a str, Vec<&'a Row<'p>>)> {
    /// What an entry with no category is filed under.
    const UNSORTED: &str = "not categorised";

    let mut groups: BTreeMap<&str, Vec<&Row<'p>>> = BTreeMap::new();
    for row in rows {
        let group = row
            .payload
            .category
            .as_deref()
            .filter(|category| !category.trim().is_empty())
            .unwrap_or(UNSORTED);
        groups.entry(group).or_default().push(row);
    }

    let mut ordered: Vec<(&str, Vec<&Row<'p>>)> = groups.into_iter().collect();
    // Alphabetical, with the unclassified group last.
    ordered.sort_by_key(|(name, _)| (*name == UNSORTED, *name));
    ordered
}

/// Whether a named payload is one this project can see, and if so whether it answered.
fn presence_of(name: &str, report: Option<&Report>) -> Presence {
    let Some(report) = report else {
        return Presence::Unknown;
    };
    // A port the manifest declares for this entry is more specific than the known-service
    // table, so it is checked first.
    if let Some(found) = report.declared.get(name) {
        return if found.open {
            Presence::Loaded
        } else {
            Presence::NotLoaded
        };
    }
    // Matched by the name the service's own project uses, which a repository entry carries.
    // A different spelling reads as unknown rather than absent.
    if !SERVICES.iter().any(|service| service.name == name) {
        return Presence::Unknown;
    }
    report
        .findings
        .iter()
        .find(|finding| finding.service.name == name)
        .map_or(Presence::Unknown, |finding| {
            if finding.reachability.open {
                Presence::Loaded
            } else {
                Presence::NotLoaded
            }
        })
}

/// Every payload file the manager holds, found by looking inside its folders.
///
/// Measured on a target: the manager keeps `/data/pldmgr/payloads/<name>/<name>_<version>.elf`,
/// with a `.json` sidecar beside some, so the scan looks one folder down as well as at the top.
///
/// # Errors
///
/// Only when the top of the walk cannot be listed. A folder that will not open is skipped.
pub fn on_target_at(
    link: &pros_link::Link,
    root: &str,
    storage: Where,
) -> Result<Vec<There>, String> {
    let mut session = pros_link::files::Session::open(link).map_err(|why| why.to_string())?;
    let top = session.list(root).map_err(|why| why.to_string())?;
    let root = root.trim_end_matches('/');

    let mut found = Vec::new();
    let mut sidecars: Vec<String> = Vec::new();
    for entry in top {
        if !entry.is_usable() {
            continue;
        }
        if is_a_payload(&entry.name) {
            found.push(There {
                path: format!("{root}/{}", entry.name),
                name: entry.name.clone(),
                storage,
                about: None,
            });
            continue;
        }
        if entry.kind != pros_link::files::Kind::Directory {
            continue;
        }
        let inside = format!("{root}/{}", entry.name);
        let Ok(entries) = session.list(&inside) else {
            continue;
        };
        for one in entries {
            if !one.is_usable() {
                continue;
            }
            // Sidecars are taken from the listing, so only ones that exist are fetched.
            if one.name.to_ascii_lowercase().ends_with(".elf.json") {
                sidecars.push(format!("{inside}/{}", one.name));
                continue;
            }
            if is_a_payload(&one.name) {
                found.push(There {
                    path: format!("{inside}/{}", one.name),
                    name: one.name,
                    storage,
                    about: None,
                });
            }
        }
    }

    // Internal storage only: a payload on removable storage cannot be relied on by a startup
    // list, so its sidecar is not worth the fetch.
    if storage == Where::Internal {
        for path in sidecars {
            let Some(payload) = path.strip_suffix(".json") else {
                continue;
            };
            let Some(one) = found.iter_mut().find(|one| one.path == payload) else {
                continue;
            };
            if let Ok(bytes) = session.retrieve(&path)
                && let Ok(about) = serde_json::from_slice::<Beside>(&bytes)
            {
                one.about = Some(about);
            }
        }
    }
    session.close();

    found.sort_by_key(|one| one.name.to_lowercase());
    found.dedup_by(|a, b| a.path == b.path);
    Ok(found)
}

/// Where a payload file lives, and what that means for a startup list.
///
/// From the manager's source: `payload_mgr_resolve_path`, which resolves a startup-list name,
/// searches `SCAN_DIRS` (`/data/pldmgr` and `/mnt/usbN/pldmgr`), but its web listing with
/// `SCAN_USB_PAYLOADS=1` also walks the root of every stick. It lists payloads it can never
/// autoload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    /// On the target's own disk, under the manager's directory. Always resolvable.
    Internal,
    /// Under `pldmgr` on removable storage. Resolvable only while that is plugged in.
    Removable,
    /// Anywhere the manager cannot resolve: elsewhere on removable storage, or outside its
    /// directory. A startup-list entry naming one of these never loads.
    Unreachable,
}

impl Where {
    /// A short tag for a listing.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Internal => "INTERNAL",
            Self::Removable => "EXTERNAL",
            Self::Unreachable => "UNREACHABLE",
        }
    }

    /// Whether the manager could resolve a startup-list entry naming this.
    #[must_use]
    pub const fn can_autoload(self) -> bool {
        !matches!(self, Self::Unreachable)
    }

    /// What it means, for somebody deciding whether to use it.
    #[must_use]
    pub const fn means(self) -> &'static str {
        match self {
            Self::Internal => "on the target's own disk - safe to put in a startup list",
            Self::Removable => {
                "on removable storage - a startup list naming this only works while that is \
                 plugged in"
            }
            Self::Unreachable => {
                "on removable storage, outside the manager's own folder - the manager lists it \
                 but cannot resolve it, so a startup list naming it fails at every boot"
            }
        }
    }
}

/// A payload file on the target, and where it is.
///
/// The startup list names a bare filename; anything that runs the file needs the path, which
/// cannot be reconstructed from the name because the scan looks one folder down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct There {
    /// The file's own name, which is what the startup list refers to.
    pub name: String,
    /// The full path on the target, which is what anything running it needs.
    pub path: String,
    /// Which storage it is on, and so whether a startup list can rely on it.
    pub storage: Where,
    /// What the manager recorded beside it, if anything.
    ///
    /// A payload carries no version: an `elfldr_v0.24.elf` taken from a target has no version
    /// string, no `.note` section and no build id. The manager writes a `<filename>.json`
    /// sidecar when it installs from a repository, the only on-target record of version and
    /// checksum. `None` for a payload put there by hand or by an autoloader.
    pub about: Option<Beside>,
}

/// What a manager wrote beside a payload when it installed it.
///
/// A manifest entry: the manager copies the repository's description into the sidecar, so
/// the two compare like for like.
pub type Beside = Payload;

/// The bytes of the sidecar that describes a payload on the target.
///
/// The payload itself carries no version, so this file is what says which build it is.
///
/// # Errors
///
/// When the description cannot be serialised, which is a bug in this crate.
pub fn sidecar_for(payload: &Payload) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(payload).map_err(|why| why.to_string())
}

/// Whether a filename is one the loader would take.
///
/// A `.json` sidecar in the startup list would stop the chain.
fn is_a_payload(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".elf")
}
/// Where the manager keeps payloads on the target's own disk.
pub const INTERNAL: &str = "/data/pldmgr/payloads";

/// Another place on the target's own drive where payloads collect.
///
/// The manager cannot resolve it, so payloads here are scanned and marked unreachable.
pub const ELSEWHERE: &str = "/data/payloads";

/// Everything the manager can see, wherever it is, tagged with what that means.
///
/// The roots come from the manager's header: `SCAN_DIRS` is `/data/pldmgr` and
/// `/mnt/usbN/pldmgr` for eight sticks. The scan also covers what the manager lists but cannot
/// resolve, tagged [`Where::Unreachable`].
///
/// # Errors
///
/// Only when the internal directory cannot be listed. A missing stick is not a failure.
pub fn on_target_everywhere(link: &pros_link::Link) -> Result<Vec<There>, String> {
    let mut found = on_target_at(link, INTERNAL, Where::Internal)?;
    // On the target's drive, but outside `SCAN_DIRS`, so unreachable.
    if let Ok(more) = on_target_at(link, ELSEWHERE, Where::Unreachable) {
        found.extend(more);
    }
    for stick in 0..8 {
        // The manager's own folder on the stick: resolvable while it is plugged in.
        let mine = format!("/mnt/usb{stick}/pldmgr");
        if let Ok(more) = on_target_at(link, &mine, Where::Removable) {
            found.extend(more);
        }
        // The rest of the stick: listed by the manager, never resolvable by it.
        let root = format!("/mnt/usb{stick}");
        if let Ok(more) = on_target_at(link, &root, Where::Unreachable) {
            found.extend(more);
        }
    }
    // A file found twice keeps the place a startup list can rely on most.
    found.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| rank(a.storage).cmp(&rank(b.storage)))
    });
    found.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    Ok(found)
}

/// How much a startup list can rely on a place. Lower is better.
const fn rank(storage: Where) -> u8 {
    match storage {
        Where::Internal => 0,
        Where::Removable => 1,
        Where::Unreachable => 2,
    }
}
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pros_link::service::{Reachability, SERVICES};

    use super::{Boot, Presence, Trust, survey};
    use crate::chain::Chain;
    use crate::check::{Finding, Report};
    use crate::manifest::Manifest;

    const GOOD: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn manifest() -> Manifest {
        let text = format!(
            r#"[
                {{ "name": "shsrv",   "checksum": "{GOOD}" }},
                {{ "name": "klogsrv", "checksum": "{GOOD}" }},
                {{ "name": "cheats",  "checksum": "d41d8cd98f00b204e9800998ecf8427e" }}
            ]"#
        );
        Manifest::from_json(&text).expect("reads")
    }

    fn report(open: &[&str]) -> Report {
        let findings = SERVICES
            .iter()
            .map(|service| Finding {
                service: service.clone(),
                reachability: Reachability {
                    open: open.contains(&service.name.as_ref()),
                    took: Duration::from_millis(5),
                },
            })
            .collect();
        Report::new("prospero", "127.0.0.1", findings)
    }

    /// A service that answered is loaded; one that did not is not.
    #[test]
    fn a_service_with_a_known_port_is_measured() {
        let manifest = manifest();
        let report = report(&["shsrv"]);
        let rows = survey(&manifest, Some(&report), None);

        let shell = rows.iter().find(|row| row.payload.name == "shsrv").unwrap();
        let log = rows
            .iter()
            .find(|row| row.payload.name == "klogsrv")
            .unwrap();
        assert_eq!(shell.presence, Presence::Loaded);
        assert_eq!(log.presence, Presence::NotLoaded);
    }

    /// A payload with no known port is unknown, never absent.
    #[test]
    fn a_payload_with_no_known_port_is_unknown_and_never_absent() {
        let manifest = manifest();
        let report = report(&[]);
        let rows = survey(&manifest, Some(&report), None);

        let other = rows
            .iter()
            .find(|row| row.payload.name == "cheats")
            .unwrap();
        assert_eq!(
            other.presence,
            Presence::Unknown,
            "a payload nothing can see was reported as absent"
        );
    }

    /// With no check run, everything is unknown - not everything absent.
    #[test]
    fn no_check_means_unknown_rather_than_missing() {
        let manifest = manifest();
        let rows = survey(&manifest, None, None);
        assert!(
            rows.iter().all(|row| row.presence == Presence::Unknown),
            "a manifest read with no target present reported payloads as absent"
        );
        assert_eq!(rows.len(), 3, "the manifest should still be shown in full");
    }

    /// A service can be loaded now and absent from the boot list.
    #[test]
    fn a_service_can_be_loaded_now_and_not_in_the_boot_list() {
        let manifest = manifest();
        let report = report(&["shsrv"]);
        let chain = Chain::parse(
            "elfldr.elf
klogsrv.elf
",
        );
        let rows = survey(&manifest, Some(&report), Some(&chain));

        let shell = rows.iter().find(|row| row.payload.name == "shsrv").unwrap();
        assert_eq!(shell.presence, Presence::Loaded);
        assert_eq!(shell.boot, Boot::NotInList, "it will not come back");

        let log = rows
            .iter()
            .find(|row| row.payload.name == "klogsrv")
            .unwrap();
        assert_eq!(log.presence, Presence::NotLoaded);
        assert_eq!(
            log.boot,
            Boot::At(1),
            "it is in the list and is not running"
        );
    }

    /// A list nobody fetched says nothing about what is in it.
    #[test]
    fn no_boot_list_means_unknown_rather_than_absent_from_it() {
        let manifest = manifest();
        let rows = survey(&manifest, None, None);
        assert!(
            rows.iter().all(|row| row.boot == Boot::Unknown),
            "a boot list that was never read was reported as not naming things"
        );
    }

    /// A survey is grouped by the repository's own categories.
    #[test]
    fn a_survey_is_grouped_by_the_repositorys_own_categories() {
        let text = format!(
            r#"[
                {{ "name": "elfldr",  "category": "Loaders",  "checksum": "{GOOD}" }},
                {{ "name": "ftpsrv",  "category": "Networking & Servers", "checksum": "{GOOD}" }},
                {{ "name": "zftpd",   "category": "Networking & Servers", "checksum": "{GOOD}" }},
                {{ "name": "mystery", "checksum": "{GOOD}" }}
            ]"#
        );
        let manifest = Manifest::from_json(&text).expect("reads");
        let rows = survey(&manifest, None, None);
        let groups = super::by_category(&rows);

        let names: Vec<&str> = groups.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            ["Loaders", "Networking & Servers", "not categorised"]
        );
        assert_eq!(groups[1].1.len(), 2, "the two servers belong together");
    }

    /// An entry nobody classified is filed as unclassified, not somewhere plausible.
    #[test]
    fn an_uncategorised_entry_is_not_quietly_filed_under_something() {
        let text = format!(r#"[{{ "name": "mystery", "category": "  ", "checksum": "{GOOD}" }}]"#);
        let manifest = Manifest::from_json(&text).expect("reads");
        let rows = survey(&manifest, None, None);
        let groups = super::by_category(&rows);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "not categorised");
    }

    /// Trust carries why, because the two reasons need different work.
    #[test]
    fn trust_says_which_kind_of_doubt() {
        let manifest = manifest();
        let rows = survey(&manifest, None, None);
        let shell = rows.iter().find(|row| row.payload.name == "shsrv").unwrap();
        let other = rows
            .iter()
            .find(|row| row.payload.name == "cheats")
            .unwrap();

        assert_eq!(shell.trust, Trust::Verifiable);
        match &other.trust {
            Trust::Doubtful(why) => assert!(why.to_string().contains("md5"), "{why}"),
            Trust::Verifiable => panic!("an md5 digest was treated as verifiable"),
        }
    }

    /// A port declared in the manifest is measured like a known service's.
    #[test]
    fn a_declared_port_is_measured_like_any_other() {
        let manifest =
            Manifest::from_json(r#"[{ "name": "websrv", "port": 8080 }]"#).expect("reads");
        let mut report = report(&["elfldr"]);
        report.declared.insert(
            "websrv".to_owned(),
            Reachability {
                open: true,
                took: Duration::from_millis(2),
            },
        );

        let rows = survey(&manifest, Some(&report), None);
        assert_eq!(
            rows[0].presence,
            Presence::Loaded,
            "a declared port that answered still read as unknown"
        );
    }

    /// A declared port that did not answer is absent, not unknown, because it was measured.
    #[test]
    fn a_declared_port_that_is_shut_is_absent_rather_than_unknown() {
        let manifest =
            Manifest::from_json(r#"[{ "name": "websrv", "port": 8080 }]"#).expect("reads");
        let mut report = report(&["elfldr"]);
        report.declared.insert(
            "websrv".to_owned(),
            Reachability {
                open: false,
                took: Duration::from_millis(2),
            },
        );

        let rows = survey(&manifest, Some(&report), None);
        assert_eq!(rows[0].presence, Presence::NotLoaded);
    }
}

/// How an installed payload compares to what the list describes.
///
/// A version nobody recorded is [`Standing::Unknown`], not an old version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// The installed version is the one described.
    Current,
    /// Something newer is described, and this is what is installed.
    Behind {
        /// What is on the target.
        installed: String,
        /// What the list describes.
        described: String,
    },
    /// The installed version is not the described one, and is not obviously older.
    ///
    /// Version strings are free text that cannot be ordered in general.
    Different {
        /// What is on the target.
        installed: String,
        /// What the list describes.
        described: String,
    },
    /// Nothing on the target says what version it is.
    ///
    /// There is no sidecar (a payload put there by hand), or its version is empty.
    Unknown,
}

impl There {
    /// What version the target says this is, if anything does.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.about
            .as_ref()
            .and_then(|about| about.version.as_deref())
            .map(str::trim)
            .filter(|version| !version.is_empty())
    }

    /// How this compares to what a list describes.
    ///
    /// Matched on the checksum first, the only thing that identifies a build; a filename or
    /// version string is an editable claim.
    #[must_use]
    pub fn standing(&self, described: &Payload) -> Standing {
        let Some(installed) = self.version() else {
            return Standing::Unknown;
        };
        let Some(wanted) = described.version.as_deref().map(str::trim) else {
            return Standing::Unknown;
        };
        if wanted.is_empty() {
            return Standing::Unknown;
        }
        // Same digest means same build, whatever either is called.
        let same_bytes = match (
            self.about
                .as_ref()
                .and_then(|about| about.checksum.as_deref()),
            described.checksum.as_deref(),
        ) {
            (Some(here), Some(there)) if !here.is_empty() && !there.is_empty() => {
                Some(here.eq_ignore_ascii_case(there))
            }
            _ => None,
        };
        if same_bytes == Some(true) || installed.eq_ignore_ascii_case(wanted) {
            return Standing::Current;
        }
        if is_older(installed, wanted) {
            return Standing::Behind {
                installed: installed.to_owned(),
                described: wanted.to_owned(),
            };
        }
        Standing::Different {
            installed: installed.to_owned(),
            described: wanted.to_owned(),
        }
    }
}

/// Whether one version string is plainly older than another.
///
/// Only where both are dotted numbers, optionally with a leading `v`. Anything else (a date,
/// a beta, a word) answers `false`, so it is reported as different rather than ordered.
///
/// Public so the version column and the source column share one comparison rule.
#[must_use]
pub fn is_older(installed: &str, described: &str) -> bool {
    let parts = |text: &str| -> Option<Vec<u32>> {
        let text = text.trim().trim_start_matches(['v', 'V']);
        text.split('.')
            .map(|part| part.parse::<u32>().ok())
            .collect()
    };
    match (parts(installed), parts(described)) {
        (Some(here), Some(there)) => here < there,
        _ => false,
    }
}

#[cfg(test)]
mod standing_tests {
    use super::{Standing, There, Where};
    use crate::manifest::Payload;

    fn installed(version: &str, checksum: &str) -> There {
        There {
            name: "elfldr_v0.24.elf".to_owned(),
            path: "/data/pldmgr/payloads/elfldr/elfldr_v0.24.elf".to_owned(),
            storage: Where::Internal,
            about: Some(Payload {
                name: "elfldr".to_owned(),
                version: (!version.is_empty()).then(|| version.to_owned()),
                checksum: (!checksum.is_empty()).then(|| checksum.to_owned()),
                ..Payload::default()
            }),
        }
    }

    fn described(version: &str, checksum: &str) -> Payload {
        Payload {
            name: "elfldr".to_owned(),
            version: Some(version.to_owned()),
            checksum: (!checksum.is_empty()).then(|| checksum.to_owned()),
            ..Payload::default()
        }
    }

    /// Matching digests mean the same build, whatever the versions say.
    #[test]
    fn matching_bytes_are_the_same_build_whatever_the_version_says() {
        let one = installed("v0.24", "aa");
        assert_eq!(one.standing(&described("v0.25", "AA")), Standing::Current);
    }

    /// An older installed version is reported as behind.
    #[test]
    fn an_older_version_is_reported_as_behind() {
        let one = installed("v0.24", "aa");
        let Standing::Behind {
            installed,
            described: want,
        } = one.standing(&described("v0.25", "bb"))
        else {
            panic!("0.24 is behind 0.25");
        };
        assert_eq!(installed, "v0.24");
        assert_eq!(want, "v0.25");
    }

    /// A version nobody recorded is unknown, not behind.
    #[test]
    fn an_unrecorded_version_is_unknown_rather_than_behind() {
        let one = installed("", "");
        assert_eq!(one.standing(&described("v0.25", "bb")), Standing::Unknown);
        let no_sidecar = There {
            about: None,
            ..installed("v0.24", "aa")
        };
        assert_eq!(
            no_sidecar.standing(&described("v0.25", "bb")),
            Standing::Unknown
        );
    }

    /// Versions that cannot be ordered are different, never behind.
    #[test]
    fn versions_that_cannot_be_ordered_are_only_different() {
        let one = installed("1.6beta16", "aa");
        assert!(matches!(
            one.standing(&described("1.6beta17", "bb")),
            Standing::Different { .. }
        ));
    }

    /// A build newer than the list describes is different, not behind.
    #[test]
    fn a_newer_build_than_the_list_is_not_called_behind() {
        let one = installed("v0.25", "aa");
        assert!(matches!(
            one.standing(&described("v0.24", "bb")),
            Standing::Different { .. }
        ));
    }
}
