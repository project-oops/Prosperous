//! What the target loads when it comes back.
//!
//! A check says what is answering now; the payload manager's boot list says what will answer
//! after the next power cycle. A service missing after a reboot is most often one that was
//! never in the list, so a report that a service is not loaded also says whether it is listed.
//!
//! The list's path was measured on a target, so it is a constant here.

use pros_link::files;

/// Where the payload manager keeps its boot list, as measured on a target.
pub const PATH: &str = "/data/pldmgr/autoload.txt";

/// The payloads a target loads at boot, in the order it loads them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chain {
    order: Vec<String>,
}

impl Chain {
    /// Reads the list.
    ///
    /// One name per line. Blank lines and `#` comments are ignored, and a line's leading and
    /// trailing space is not part of a name.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        Self {
            order: text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                // `!` lines are instructions to the manager (a real list interleaves `!3000`
                // between entries), not payloads.
                .filter(|line| !line.starts_with('!'))
                // The list names files; the manifest and the service table name payloads.
                .map(|line| bare_name(line).to_owned())
                .collect(),
        }
    }

    /// Fetches the list off a target.
    ///
    /// # Errors
    ///
    /// Propagates the transfer. A chain that could not be read is not an empty chain; the
    /// caller reports it as unknown, which differs from not in the list.
    pub fn read(link: &pros_link::Link) -> pros_link::Result<Self> {
        let bytes = files::retrieve(link, PATH)?;
        Ok(Self::parse(&String::from_utf8_lossy(&bytes)))
    }

    /// Where a payload appears in the list, if it does.
    ///
    /// A real list carries versioned names such as `elfldr_v0` and `kstuff-lite_v1`, so a name
    /// matches the whole entry, or the entry's start followed by `_` or `-` and something
    /// version-shaped (a digit, or `v` and a digit). Requiring the version keeps `kstuff` from
    /// matching `kstuff-lite_v1`, which is a different payload.
    #[must_use]
    pub fn position(&self, name: &str) -> Option<usize> {
        let wanted = bare_name(name);
        self.order.iter().position(|loaded| {
            if loaded.eq_ignore_ascii_case(wanted) {
                return true;
            }
            let Some(head) = loaded.get(..wanted.len()) else {
                return false;
            };
            if !head.eq_ignore_ascii_case(wanted) {
                return false;
            }
            let rest = loaded.get(wanted.len()..).unwrap_or_default();
            let Some(tail) = rest.strip_prefix(['_', '-']) else {
                return false;
            };
            is_version(tail)
        })
    }

    /// Everything in the list, in order.
    #[must_use]
    pub fn order(&self) -> &[String] {
        &self.order
    }

    /// Whether the list is empty, which is a real answer about a target that loads nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// Whether what follows a name is a version rather than more name.
///
/// `v1`, `0`, `1.6beta16` are versions. `lite_v1` is the rest of somebody else's name.
fn is_version(tail: &str) -> bool {
    let digits = tail.strip_prefix(['v', 'V']).unwrap_or(tail);
    digits.starts_with(|c: char| c.is_ascii_digit())
}

/// A file name without its directory or extension.
///
/// Only the known extensions `.elf` and `.bin` come off (the ones the manager accepts, from
/// its source), never whatever follows a dot: versions contain dots, and `ftpsrv_v0.21.elf`
/// and `ftpsrv_v0.21.1.elf` must stay two different builds. The same rule applies to the
/// name being looked for, so `ftpsrv_v0.21` keeps its `.21`.
fn bare_name(line: &str) -> &str {
    let file = line.rsplit(['/', '\\']).next().unwrap_or(line);
    for extension in [".elf", ".bin"] {
        if let Some(stem) = file.len().checked_sub(extension.len())
            && file
                .get(stem..)
                .is_some_and(|tail| tail.eq_ignore_ascii_case(extension))
            && stem > 0
        {
            return &file[..stem];
        }
    }
    file
}

#[cfg(test)]
mod tests {
    use super::Chain;

    const AUTOLOAD: &str = "# loaded in this order\n\
                            kstuff-lite.elf\n\
                            nanodns.elf\n\
                            \n\
                            elfldr.elf\n\
                            klogsrv.elf\n\
                            /data/payloads/shsrv.elf\n";

    /// The list keeps its order: what loads before what.
    #[test]
    fn the_list_keeps_its_order() {
        let chain = Chain::parse(AUTOLOAD);
        assert_eq!(chain.position("kstuff-lite"), Some(0));
        assert_eq!(chain.position("elfldr"), Some(2));
        assert!(
            chain.position("elfldr") < chain.position("klogsrv"),
            "the loader must come before what it loads"
        );
    }

    /// A boot list read verbatim from a target.
    const REAL: &str = "!3000
                        kstuff-lite_v1
                        !3000
                        nanodns
                        !3000
                        elfldr_v0
                        !3000
                        ShadowMountPlus_1
                        !3000
                        ps5upload-4
                        !3000
                        ftpsrv_v0
";

    /// A line beginning `!` is an instruction to the manager, not a payload.
    #[test]
    fn a_directive_is_not_a_payload() {
        let chain = Chain::parse(REAL);
        assert_eq!(chain.order().len(), 6, "{:?}", chain.order());
        assert!(
            chain.position("!3000").is_none(),
            "an instruction was counted as a payload"
        );
    }

    /// A version suffix does not make it a different payload.
    #[test]
    fn a_version_suffix_still_matches_the_payload() {
        let chain = Chain::parse(REAL);
        assert_eq!(chain.position("elfldr"), Some(2));
        assert_eq!(chain.position("kstuff-lite"), Some(0));
        assert_eq!(chain.position("ps5upload"), Some(4));
        assert_eq!(chain.position("ftpsrv"), Some(5));
    }

    /// Two payloads whose names share a prefix are two payloads.
    #[test]
    fn a_name_that_is_the_start_of_another_name_does_not_match_it() {
        let chain = Chain::parse(REAL);
        assert_eq!(chain.position("kstuff-lite"), Some(0));
        assert_eq!(
            chain.position("kstuff"),
            None,
            "kstuff matched kstuff-lite, which is a different payload"
        );
    }

    /// A suffix that is not marked as one is a different name.
    #[test]
    fn a_longer_name_is_not_the_same_payload() {
        let chain = Chain::parse(
            "elfldrx
ftpsrvng
",
        );
        assert_eq!(chain.position("elfldr"), None);
        assert_eq!(chain.position("ftpsrv"), None);
    }

    /// A running service can be absent from the boot list, so it is gone after a power cycle.
    #[test]
    fn a_service_can_be_running_and_absent_from_the_real_list() {
        assert_eq!(Chain::parse(REAL).position("shsrv"), None);
        assert_eq!(Chain::parse(REAL).position("klogsrv"), None);
    }

    /// A payload not in the list is reported absent from it.
    #[test]
    fn a_payload_not_in_the_list_is_absent_from_it() {
        assert_eq!(Chain::parse(AUTOLOAD).position("ftpsrv"), None);
    }

    /// A directory and an extension are not part of the name.
    #[test]
    fn a_path_and_an_extension_are_not_part_of_the_name() {
        let chain = Chain::parse(AUTOLOAD);
        assert_eq!(chain.position("shsrv"), Some(4), "a path was not stripped");
        assert_eq!(chain.position("shsrv.elf"), Some(4), "asked with extension");
    }

    /// Comments and blank lines are formatting, not payloads.
    #[test]
    fn comments_and_blanks_are_not_payloads() {
        assert_eq!(Chain::parse(AUTOLOAD).order().len(), 5);
        assert!(Chain::parse("# nothing here\n\n").is_empty());
    }
}

#[cfg(test)]
mod versioned_names {
    use super::Chain;

    /// Two builds of one payload are two different entries.
    #[test]
    fn two_builds_of_one_payload_are_not_the_same_entry() {
        let chain = Chain::parse("ftpsrv_v0.21.elf\n");
        assert_eq!(chain.order(), ["ftpsrv_v0.21"]);
        assert_eq!(
            Chain::parse("ftpsrv_v0.21.1.elf").order(),
            ["ftpsrv_v0.21.1"]
        );
        assert_ne!(
            Chain::parse("ftpsrv_v0.21.elf").order(),
            Chain::parse("ftpsrv_v0.21.1.elf").order()
        );
    }

    /// A version with dots in it is kept whole.
    #[test]
    fn a_version_with_dots_in_it_is_kept_whole() {
        assert_eq!(
            Chain::parse("pldmgr_v0.5.1.elf").order(),
            ["pldmgr_v0.5.1"],
            "the panel said `pldmgr_v0` because this was truncated"
        );
        assert_eq!(
            Chain::parse("kstuff-lite_v1.09.elf").order(),
            ["kstuff-lite_v1.09"]
        );
        assert_eq!(Chain::parse("etaHEN_2.5B.bin").order(), ["etaHEN_2.5B"]);
    }

    /// A service is found by its bare name under a versioned filename.
    #[test]
    fn a_service_is_still_found_under_its_versioned_filename() {
        let chain = Chain::parse(
            "kstuff-lite_v1.09.elf\nnanodns.elf\nelfldr_v0.24.elf\nftpsrv_v0.21.elf\n",
        );
        assert_eq!(chain.position("elfldr"), Some(2));
        assert_eq!(chain.position("ftpsrv"), Some(3));
        assert_eq!(chain.position("nanodns"), Some(1));
        assert_eq!(chain.position("shsrv"), None);
    }

    /// One build does not answer to another build's full name.
    #[test]
    fn one_build_does_not_match_another_by_full_name() {
        let chain = Chain::parse("ftpsrv_v0.21.1.elf\n");
        assert_eq!(chain.position("ftpsrv_v0.21"), None);
        assert_eq!(chain.position("ftpsrv_v0.21.1"), Some(0));
    }
}

/// The placeholder a chain writes when a list can be on any removable device.
pub const DEVICE: &str = "{device}";

/// The placeholder for a USB stick, and only a stick.
///
/// The autoloader searches `USB_BASES[]` (`/mnt/usb0` to `/mnt/usb7`) and not `/mnt/ext0` or
/// `/mnt/ext1`, so `{device}` there would offer paths the autoloader never reads.
pub const USB: &str = "{usb}";

/// A startup list a target may have, and what may be done with it.
///
/// The manager's list sits at a compile-time constant path. The autoloader that runs before
/// it looks for its own list in several places, a stick first, then the internal drive, and
/// that list decides whether the manager runs at all. The audit rules invert between them:
/// the loader is kept out of an autoloader's list and belongs last in the manager's; the
/// manager is required in an autoloader's list and impossible in its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// What to call it.
    pub label: String,
    /// Where it is on the target.
    pub path: String,
    /// Whether this program will write to it.
    ///
    /// Only the internal one: a list on removable storage is the recovery path when the
    /// internal setup is broken, and is only read.
    pub editable: bool,
    /// Whether it is the manager's own list or an autoloader's.
    pub autoloader: bool,
}

/// Every startup list the loaded chains know about.
///
/// The union of the lists the loaded chains declare, so a path is here because a chain file
/// says so and can be changed without rebuilding. If none is declared, the manager's measured
/// [`PATH`] remains.
///
/// This parses the chains, including the user's own file; call it once and keep the answer.
#[must_use]
pub fn lists() -> Vec<Held> {
    let mut found: Vec<Held> = Vec::new();
    for preset in crate::recovery::baseline::all().0 {
        for one in preset.lists {
            for at in &one.at {
                for (path, label) in spread(at, &one.label) {
                    // By path: two chains naming the same file mean one list.
                    if found.iter().any(|kept| kept.path == path) {
                        continue;
                    }
                    found.push(Held {
                        label,
                        path,
                        editable: one.editable,
                        autoloader: one.autoloader,
                    });
                }
            }
        }
    }
    // Never empty: an emptied or unparseable chains file still leaves the measured manager list.
    if found.is_empty() {
        found.push(Held {
            label: "manager (internal)".to_owned(),
            path: PATH.to_owned(),
            editable: true,
            autoloader: false,
        });
    }
    found
}

/// Every actual path worth copying off a target when a chain is written down, each with its
/// label.
///
/// The declared [`crate::recovery::baseline::Capture`] paths, with `{device}` / `{usb}` expanded
/// over a target's removable mounts as a list's places are, and deduplicated by path. A pair
/// rather than a [`Held`], because a captured file is not a startup list.
///
/// This parses the chains, including the user's own file; call it once and keep the answer.
#[must_use]
pub fn capture_spots() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for one in crate::recovery::baseline::captures() {
        for at in &one.at {
            for (path, label) in spread(at, &one.label) {
                if found.iter().any(|(kept, _)| *kept == path) {
                    continue;
                }
                found.push((path, label));
            }
        }
    }
    found
}

/// One declared place, as the paths it actually means.
///
/// A path naming [`DEVICE`] is every removable device a target can have, [`USB`] every stick;
/// anything else is itself. The label gains the device's name.
fn spread(at: &str, label: &str) -> Vec<(String, String)> {
    let (mark, sticks_only) = if at.contains(USB) {
        (USB, true)
    } else if at.contains(DEVICE) {
        (DEVICE, false)
    } else {
        return vec![(at.to_owned(), label.to_owned())];
    };
    crate::places::Device::all()
        .into_iter()
        .filter(|device| !sticks_only || matches!(device, crate::places::Device::Usb(_)))
        .filter_map(|device| {
            let root = device.root()?;
            Some((
                at.replace(mark, &root),
                format!("{label} ({})", device.label()),
            ))
        })
        .collect()
}

#[cfg(test)]
mod lists {
    use super::{DEVICE, PATH, USB, lists};

    /// Nothing on removable storage is editable.
    #[test]
    fn nothing_on_removable_storage_is_editable() {
        for held in lists() {
            if held.path.starts_with("/mnt/") {
                assert!(!held.editable, "{} would be written to", held.path);
            }
        }
    }

    /// The manager's own list is the only editable one.
    #[test]
    fn the_managers_list_is_the_one_that_can_be_edited() {
        let editable: Vec<String> = lists()
            .into_iter()
            .filter(|held| held.editable)
            .map(|held| held.path)
            .collect();
        assert_eq!(editable, [PATH.to_owned()]);
    }

    /// Each list says whether it is the manager's or an autoloader's.
    #[test]
    fn each_list_says_which_kind_it_is() {
        let all = lists();
        let manager = all.iter().find(|held| !held.autoloader).expect("one");
        assert_eq!(manager.path, PATH);
        assert!(
            all.iter().filter(|held| held.autoloader).count() >= 2,
            "an autoloader looks in more than one place"
        );
    }

    /// A device placeholder expands to every device and never reaches the chooser as itself.
    #[test]
    fn a_removable_list_is_offered_once_per_device() {
        let all = lists();
        assert!(
            !all.iter()
                .any(|held| held.path.contains(DEVICE) || held.path.contains(USB)),
            "a placeholder reached the chooser: {all:?}"
        );
        for device in ["/mnt/usb0", "/mnt/usb7"] {
            assert!(
                all.iter().any(|held| held.path.starts_with(device)),
                "{device} has no list offered"
            );
        }
    }

    /// A stick placeholder never expands to an external drive.
    #[test]
    fn a_stick_placeholder_stays_on_sticks() {
        let stuck: Vec<String> = lists()
            .into_iter()
            .filter(|held| held.autoloader)
            .map(|held| held.path)
            .collect();
        assert!(
            !stuck.iter().any(|path| path.starts_with("/mnt/ext")),
            "an external drive was offered an autoloader list: {stuck:?}"
        );
        assert!(stuck.iter().any(|path| path.starts_with("/mnt/usb7")));
    }

    /// A path that several chains name is offered once.
    #[test]
    fn a_path_two_chains_share_is_offered_once() {
        let all = lists();
        let mut paths: Vec<&str> = all.iter().map(|held| held.path.as_str()).collect();
        paths.sort_unstable();
        let mut once = paths.clone();
        once.dedup();
        assert_eq!(paths, once, "a path is offered twice");
    }

    /// The shipped capture paths, including the manager's settings, arrive expanded and once.
    #[test]
    fn the_declared_capture_paths_are_offered() {
        let spots = super::capture_spots();
        assert!(
            spots
                .iter()
                .any(|(path, _)| path == "/data/pldmgr/pldmgr_config.txt"),
            "the shipped capture declaration names the manager's settings: {spots:?}"
        );
        assert!(
            !spots
                .iter()
                .any(|(path, _)| path.contains(DEVICE) || path.contains(USB)),
            "a placeholder reached a caller unexpanded: {spots:?}"
        );
        let mut paths: Vec<&str> = spots.iter().map(|(path, _)| path.as_str()).collect();
        paths.sort_unstable();
        let mut once = paths.clone();
        once.dedup();
        assert_eq!(paths, once, "a capture path is offered twice");
    }
}
