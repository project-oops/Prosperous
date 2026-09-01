//! What is described, what is trustworthy, and what is actually on the target.
//!
//! # Why this is a crate and not a panel
//!
//! Putting a manifest beside a check and saying *this one is loaded and that one is not* is
//! a judgement, and judgements do not live in windows. The command line asks the same
//! question and gets the same answer, which is the only way two front ends stay agreed.
//!
//! # The distinction the whole module exists for
//!
//! **A payload nothing here can see is not a payload that is absent.**
//!
//! Five services have known ports, so their presence is a measurement. Everything else in a
//! repository - a cheat menu, a file manager, anything somebody added - has no port this
//! project knows, and no amount of probing will find it. Reporting those as *not loaded*
//! would be inventing a measurement, and it would be believed, because it sits in the same
//! column as the ones that are real.
//!
//! So there are three answers and not two, and [`Presence::Unknown`] is the honest one.

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
    /// **Carried rather than reduced to a flag**: *no checksum at all* and *a digest in an
    /// algorithm this cannot check* need different work from different people.
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
    /// **Nothing here can tell.**
    ///
    /// Either no check has been run, or this payload is not one of the services with a port
    /// this project knows. Distinct from [`Presence::NotLoaded`] on purpose: reporting an
    /// unknown as absent is inventing a measurement, and it would sit in the same column as
    /// the real ones and be believed.
    Unknown,
}

/// Whether the payload will be there after the next power cycle.
///
/// **A different question from [`Presence`], with the same-looking answer.** A service can be
/// answering now and absent from the boot list, which means it is there until somebody turns
/// the target off - and that is usually the finding somebody actually needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boot {
    /// It is in the boot list, at this position.
    At(usize),
    /// The boot list was read and does not name it.
    NotInList,
    /// The boot list was not read.
    ///
    /// **Not the same as absent**, for the same reason [`Presence::Unknown`] is not: a list
    /// nobody fetched says nothing at all about what is in it.
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
/// `report` is optional because the two questions are independent: a manifest can be read
/// with no target present at all, and saying *unknown* for every row is a better answer
/// than refusing to show the manifest.
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
/// # Why the repository's own categories rather than any of mine
///
/// A target's repository sorts its entries into a handful of groups - loaders, servers,
/// system tools - written by whoever curates it. Twenty-five entries in one list looks like
/// duplication when it is really `ftpsrv`, `ftpsrv-drakmor` and `zftpd` sitting next to each
/// other, and grouping them by what they are for makes that legible.
///
/// **Inventing a taxonomy here would be worse than using theirs**: theirs travels with the
/// data, updates when the data updates, and is the one a person sees in every other tool
/// that reads the same file.
///
/// Entries with no category go in a group of their own, last, named for what they are: not
/// categorised. They are not quietly filed under something plausible.
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
    // Alphabetical, except that the ones nobody classified go last - they are the group a
    // person is least likely to be looking for and the one most likely to grow.
    ordered.sort_by_key(|(name, _)| (*name == UNSORTED, *name));
    ordered
}

/// Whether a named payload is one this project can see, and if so whether it answered.
fn presence_of(name: &str, report: Option<&Report>) -> Presence {
    let Some(report) = report else {
        return Presence::Unknown;
    };
    // A port the manifest declared for itself. Checked first because it is the more specific
    // statement: somebody wrote this down about *this* entry, where the table below is this
    // project's own list of five.
    if let Some(found) = report.declared.get(name) {
        return if found.open {
            Presence::Loaded
        } else {
            Presence::NotLoaded
        };
    }
    // Matched by the name the service's own project uses, which is the name a repository
    // entry carries. A repository that spells one differently reads as unknown rather than
    // as absent, which is the right way for that to fail.
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
/// # Why one level down and not the top
///
/// Measured: the manager keeps `/data/pldmgr/payloads/<name>/<name>_<version>.elf`, with a
/// `.json` beside some of them. A listing of the top level is therefore almost all
/// directories - a scan that took only the files there found **one payload out of ten**, and
/// found it because somebody had dropped a loose copy in.
///
/// Both halves come back, because two callers need different ones - see [`There`].
///
/// # Errors
///
/// Only when the top of the walk cannot be listed. **A folder that will not open is skipped
/// rather than failing the scan**: one unreadable directory should not hide the nine that
/// were fine.
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
            // **Noted from the listing rather than guessed at.** Asking for `<file>.json` for
            // every payload would be a fetch per file, most of them for something that is not
            // there - two payloads on the target this was written against have no sidecar.
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

    // Fetched only where the listing said one exists, and only for what is on the target's own
    // disk: a sidecar on a stick describes a payload no startup list can resolve anyway.
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
/// # Read from the manager's own source, because the two lists differ
///
/// `payload_mgr_resolve_path` - what the manager uses to turn a startup-list name into a file -
/// searches `SCAN_DIRS`, which is `/data/pldmgr` and `/mnt/usbN/pldmgr`. Its **listing** for the
/// web interface scans more than that: with `SCAN_USB_PAYLOADS=1` it also walks the root of
/// every stick.
///
/// So the manager will show payloads it can never autoload. A list naming one of those is a
/// list with an entry that fails at every boot, and the only sign is a line in a log nobody
/// reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    /// On the target's own disk, under the manager's directory. Always resolvable.
    Internal,
    /// Under `pldmgr` on removable storage. **Resolvable only while that is plugged in.**
    Removable,
    /// Elsewhere on removable storage.
    ///
    /// **Listed by the manager and never resolvable by it.** An entry naming one of these
    /// cannot load, with or without the stick.
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
/// # Why both, when the name used to be enough
///
/// The startup list names a **bare filename** and lets the manager resolve it, so that is what
/// the autoload screen needs. Anything that *runs* a file needs the **path** - and the scan
/// looks one folder down, so the path is not something a caller can reconstruct from the name.
///
/// Returning only the name meant the one thing that knew where a payload was threw it away,
/// and every later question about it had to be answered with a guess or not at all.
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
    /// # Why the file next to the file
    ///
    /// **A payload carries no version.** Measured: no `VERSION` macro in any of these
    /// projects, and a real `elfldr_v0.24.elf` pulled off a target contains no version string,
    /// no `.note` section and no build id. The only strings in it are symbol and source names.
    ///
    /// So a version is metadata that travels alongside, and the manager writes it into a
    /// `<filename>.json` sidecar when it installs from a repository. That sidecar is the only
    /// place a build's **checksum** is recorded on the target, which makes it the only thing
    /// that can answer *is this the version the list describes* without fetching the file back
    /// and hashing it.
    ///
    /// `None` for a payload put there by hand or by an autoloader - which is a real state and
    /// not a defect. Two of the payloads on the target this was written against have no
    /// sidecar at all, and for those the filename is the only claim about what they are.
    pub about: Option<Beside>,
}

/// What a manager wrote beside a payload when it installed it.
///
/// Deliberately the same shape as a manifest entry, because it **is** one: the manager copies
/// the repository's description into the sidecar. Reusing the type means a sidecar and a
/// manifest entry are compared as like for like rather than through a translation.
pub type Beside = Payload;

/// The bytes of the sidecar that describes a payload on the target.
///
/// **Written beside a payload, because the payload itself cannot say.** No ELF in any of these
/// projects carries a version string, so this file is the only thing on a console that ever
/// answers *which build is this* - and a payload put there without one is one nothing can
/// report as out of date, ever.
///
/// # Errors
///
/// When the description cannot be serialised, which would be a bug in this crate rather than
/// anything about the payload.
pub fn sidecar_for(payload: &Payload) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(payload).map_err(|why| why.to_string())
}

/// Whether a filename is one the loader would take.
///
/// The `.json` sidecars beside some payloads are description, not code, and offering one for
/// the startup list would put a line in it that stops the chain.
fn is_a_payload(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".elf")
}
/// Where the manager keeps payloads on the target's own disk.
pub const INTERNAL: &str = "/data/pldmgr/payloads";

/// Another place on the console's own drive where payloads collect.
///
/// **Not somewhere a startup list can name.** It is where this program's send button used to
/// default to, which is how payloads came to be on a target's drive and invisible to the thing
/// that loads them. Scanned so they are seen; marked unreachable so nothing recommends one.
pub const ELSEWHERE: &str = "/data/payloads";

/// Everything the manager can see, wherever it is, tagged with what that means.
///
/// # The roots, read from the manager's own header
///
/// `SCAN_DIRS` is `/data/pldmgr` and `/mnt/usbN/pldmgr` for eight sticks, and those are the
/// **only** places `payload_mgr_resolve_path` looks. Its listing for the web interface walks
/// the root of every stick as well when `SCAN_USB_PAYLOADS` is on - so it shows payloads it
/// cannot resolve, and a startup list naming one of those fails at every boot.
///
/// This scan covers all three so the difference can be shown rather than discovered.
///
/// # Errors
///
/// Only when the internal directory cannot be listed. **A stick that is not there is not a
/// failure** - it is the normal case, and eight of them are normal eight times over.
pub fn on_target_everywhere(link: &pros_link::Link) -> Result<Vec<There>, String> {
    let mut found = on_target_at(link, INTERNAL, Where::Internal)?;
    // **Where this program's own send button has been putting them.** It is on the console's
    // drive and the manager still cannot resolve it - `payload_mgr_resolve_path` looks in
    // `/data/pldmgr` and `/mnt/usbN/pldmgr` and nowhere else - so it is listed as unreachable,
    // which is exactly what it is. Not scanning it at all was worse: a payload somebody had
    // already sent read as one the target had never seen.
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
    // A file found twice keeps the better answer: internal beats removable beats unreachable,
    // because that is the one a startup list can rely on.
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
        Report::new("ps5", "127.0.0.1", findings)
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

    /// **The rule this module exists for.**
    ///
    /// A payload with no port this project knows cannot be found by probing, and saying it
    /// is absent would be inventing a measurement - in the same column as the real ones,
    /// where it would be believed.
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

    /// **Answering now and absent from the boot list is a real state**, and it is usually
    /// the finding somebody needed: it is there until the target is turned off.
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

    /// **Grouped the way the repository groups itself**, not the way this project would.
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

    /// **A port in the list makes a payload answerable that this project could not see.**
    ///
    /// The whole reason the field exists. Before it, everything outside the five known
    /// services read as unknown forever, and the only way to widen that was a rebuild.
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

    /// And a declared port that did not answer is absent, not unknown - because it *was*
    /// measured. That distinction is the only reason to declare one.
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
/// **Three answers, not two.** A version nobody recorded is not an old version, and drawing it
/// as one would invent a measurement - the same rule the presence column follows.
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
    /// **Carried rather than called *behind*.** Version strings are somebody's text, not
    /// numbers this project can order in general; saying *different* is what is known.
    Different {
        /// What is on the target.
        installed: String,
        /// What the list describes.
        described: String,
    },
    /// Nothing on the target says what version it is.
    ///
    /// Either there is no sidecar - a payload put there by hand - or there is one and its
    /// version is empty, which a real target does have.
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
    /// Matched on the **checksum first**, because that is the only thing that actually
    /// identifies a build: a filename is a claim anybody can edit, and a version string is
    /// copied from the same place the filename was.
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
        // The digest settles it when both sides state one: two builds with the same bytes are
        // the same build whatever either of them is called.
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
/// **Only where both are plainly dotted numbers**, optionally with a leading `v`. Anything
/// else - a date, a beta, a word - answers `false` and is reported as *different* rather than
/// ordered, because inventing an order over somebody's text is how a tool tells you to
/// downgrade.
/// **Public so the one comparison rule has one home.** The version column and the source
/// column both order version strings, and two copies of this would eventually disagree about
/// the same pair while both looking right.
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

    /// **The digest settles it**, whatever either side is called.
    #[test]
    fn matching_bytes_are_the_same_build_whatever_the_version_says() {
        let one = installed("v0.24", "aa");
        assert_eq!(one.standing(&described("v0.25", "AA")), Standing::Current);
    }

    /// The case that matters on a real target: an older build installed.
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

    /// **A version nobody recorded is unknown, not old.** Reporting it as behind would be
    /// inventing a measurement, and it would sit in the same column as the real ones.
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

    /// **Versions this cannot order are *different*, never *behind*.** Inventing an order over
    /// somebody's text is how a tool ends up telling you to downgrade.
    #[test]
    fn versions_that_cannot_be_ordered_are_only_different() {
        let one = installed("1.6beta16", "aa");
        assert!(matches!(
            one.standing(&described("1.6beta17", "bb")),
            Standing::Different { .. }
        ));
    }

    /// A newer build installed than the list describes is different, not behind - the list is
    /// what is out of date, and this does not pretend to know which way somebody wants it.
    #[test]
    fn a_newer_build_than_the_list_is_not_called_behind() {
        let one = installed("v0.25", "aa");
        assert!(matches!(
            one.standing(&described("v0.24", "bb")),
            Standing::Different { .. }
        ));
    }
}
