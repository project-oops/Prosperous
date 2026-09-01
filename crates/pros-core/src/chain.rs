//! What the target loads when it comes back.
//!
//! # Why this is worth reading at all
//!
//! A check says what is answering **now**. It does not say what will be answering after the
//! next power cycle, and those are different questions with the same-looking answer.
//!
//! The payload manager loads a list, in order, from a file somebody edited. That file is the
//! reason a service is missing far more often than anything going wrong is: it was never in
//! the list. A tool that reports *klogsrv is not loaded* without being able to add *and it
//! is not in the boot list either* has left the useful half of the finding out.
//!
//! # The path here is measured, unlike the repository's
//!
//! `/data/pldmgr/autoload.txt`, measured against a target on 2026-08-25 along
//! with the order it produced. That is why it is a constant here while the repository's path
//! is a parameter the caller supplies - one was seen, the other was reasoned about, and the
//! difference between those is the whole grading discipline of the sibling projects. (D007)

use pros_link::files;

/// Where the payload manager keeps its boot list.
///
/// **Confirmed against a target on 2026-08-26**, which is the difference between this and
/// every other path in this project.
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
                // **`!` lines are instructions to the manager, not payloads.** A real list
                // interleaves `!3000` between entries - a wait, by every appearance - and
                // reading those as payloads put six of them in a boot order of twelve. Found
                // by asking a target rather than by thinking about it.
                .filter(|line| !line.starts_with('!'))
                // The list names files; the manifest and the service table name payloads.
                // Comparing them means dropping the extension, which is the only translation
                // this does and is worth being explicit about.
                .map(|line| bare_name(line).to_owned())
                .collect(),
        }
    }

    /// Fetches the list off a target.
    ///
    /// # Errors
    ///
    /// Propagates the transfer. **A chain that could not be read is not an empty chain**, so
    /// this reports the failure rather than answering with nothing - the caller turns that
    /// into *unknown*, which is a different column from *not in the list*.
    pub fn read(link: &pros_link::Link) -> pros_link::Result<Self> {
        let bytes = files::retrieve(link, PATH)?;
        Ok(Self::parse(&String::from_utf8_lossy(&bytes)))
    }

    /// Where a payload appears in the list, if it does.
    /// # Why this is not an equality test
    ///
    /// A real list carries `elfldr_v0`, `kstuff-lite_v1` and `ps5upload-4`. Those are the
    /// same payloads as `elfldr`, `kstuff-lite` and `ps5upload` with a version stuck on the
    /// end, and an equality test reported every one of them as **absent from a list they
    /// were plainly in** - which is exactly the wrong answer, because it says a service will
    /// not come back after a reboot when it will.
    ///
    /// So a name matches when it is the whole entry, or when what follows it is a
    /// **separator and then a version**.
    ///
    /// That last part is not fussiness. A first attempt accepted any separator, and on a
    /// real target `kstuff` then matched `kstuff-lite_v1` - two different payloads, one
    /// reported as the other, in the column that says what comes back after a reboot. A
    /// version looks like a version: a digit, or a `v` and a digit.
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
/// # The last dot, not the first
///
/// This split on the **first** dot, and every versioned filename in the wild has dots in its
/// version. So `pldmgr_v0.5.1.elf` became `pldmgr_v0`, and - worse - `ftpsrv_v0.21.elf` and
/// `ftpsrv_v0.21.1.elf` both became `ftpsrv_v0` and were **the same payload** as far as
/// anything here could tell.
///
/// That was not cosmetic. A list entry naming the internal `ftpsrv_v0.21` matched a copy of a
/// different build on a USB stick, was judged unreachable because of where that copy was, and
/// was **removed from somebody's startup list** as dead weight. Two builds of one payload have
/// to be distinguishable or every judgement about either is a coin toss.
///
/// Lookups by service name still work: `elfldr` against `elfldr_v0.24` matches on the name plus
/// a separator plus something version-shaped, which is a different rule and is applied after
/// this one.
/// # A known extension, not "whatever follows a dot"
///
/// Splitting at *any* dot is what caused all of the above, and splitting at the **last** one
/// only moves the problem: this is applied to the thing being looked for as well as to the
/// list, so `ftpsrv_v0.21` searched for as a whole name would lose its `.21` and match
/// nothing. The extensions are known - the manager accepts `.elf` and `.bin`, measured in its
/// own source - so those are what comes off, and a name that has neither is already bare.
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

    /// The list is an order, and the order is the point: what loads before what.
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

    /// The list a real target had on 2026-08-26, verbatim.
    ///
    /// Kept as it was found. Every rule below is here because this text broke the version
    /// that was written without it.
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

    /// **A line beginning `!` is an instruction to the manager, not a payload.**
    ///
    /// Reading them as payloads made a boot order of six into one of twelve, with every
    /// real position doubled.
    #[test]
    fn a_directive_is_not_a_payload() {
        let chain = Chain::parse(REAL);
        assert_eq!(chain.order().len(), 6, "{:?}", chain.order());
        assert!(
            chain.position("!3000").is_none(),
            "an instruction was counted as a payload"
        );
    }

    /// **A version suffix does not make it a different payload.**
    ///
    /// The list says `elfldr_v0`. An equality test called that absent, which would have told
    /// somebody their loader will not come back after a reboot when it plainly will.
    #[test]
    fn a_version_suffix_still_matches_the_payload() {
        let chain = Chain::parse(REAL);
        assert_eq!(chain.position("elfldr"), Some(2));
        assert_eq!(chain.position("kstuff-lite"), Some(0));
        assert_eq!(chain.position("ps5upload"), Some(4));
        assert_eq!(chain.position("ftpsrv"), Some(5));
    }

    /// **Two payloads whose names share a prefix are two payloads.**
    ///
    /// A real target lists `kstuff-lite_v1` and its repository describes both `kstuff` and
    /// `kstuff-lite`. Accepting any separator made `kstuff` match, which reported one
    /// payload as another in the column that says what survives a reboot.
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

    /// This target runs a shell that is not in its boot list, which is the case the boot
    /// column exists for: **there until the target is turned off.**
    #[test]
    fn a_service_can_be_running_and_absent_from_the_real_list() {
        assert_eq!(Chain::parse(REAL).position("shsrv"), None);
        assert_eq!(Chain::parse(REAL).position("klogsrv"), None);
    }

    /// A payload not in the list is absent from it, which is a real finding: it explains why
    /// a service will still be missing after the next reboot.
    #[test]
    fn a_payload_not_in_the_list_is_absent_from_it() {
        assert_eq!(Chain::parse(AUTOLOAD).position("ftpsrv"), None);
    }

    /// The list names files and everything else names payloads, so the comparison drops the
    /// extension and any directory - and does it in one place.
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

    /// **Two builds of one payload are two different entries.**
    ///
    /// They were not: splitting a filename at its first dot made `ftpsrv_v0.21.elf` and
    /// `ftpsrv_v0.21.1.elf` identical, so a list entry naming the internal one matched a
    /// different build sitting on a USB stick - and was removed from a real startup list as
    /// dead weight because of where that other copy happened to be.
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

    /// A whole version survives, so anything reporting an entry names it in full.
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

    /// Looking a service up by its bare name still works, which is the rule that made the
    /// truncation survive unnoticed for so long.
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

    /// **And a different build does not answer to another build's full name.**
    #[test]
    fn one_build_does_not_match_another_by_full_name() {
        let chain = Chain::parse("ftpsrv_v0.21.1.elf\n");
        assert_eq!(chain.position("ftpsrv_v0.21"), None);
        assert_eq!(chain.position("ftpsrv_v0.21.1"), Some(0));
    }
}

/// A startup list a target may have, and what may be done with it.
///
/// # Why there is more than one, and why they are not interchangeable
///
/// The manager keeps a list at a **compile-time constant** path, so that one cannot move. The
/// autoloader that runs before it looks for its own list in several places - a stick first,
/// then the internal drive - and that list is the one that decides whether the manager runs at
/// all.
///
/// They are audited by different rules. The loader is **required** in an autoloader's list and
/// **impossible** in the manager's, so which list is being looked at is not a detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    /// What to call it.
    pub label: &'static str,
    /// Where it is on the target.
    pub path: &'static str,
    /// Whether this program will write to it.
    ///
    /// **Only the internal one.** A list on removable storage is somebody's way back in when
    /// the internal setup is broken; a tool that can damage the recovery path is worse than one
    /// that only reads it.
    pub editable: bool,
    /// Whether it is the manager's own list or an autoloader's.
    pub autoloader: bool,
}

/// Every startup list this program knows how to look at.
///
/// The manager's first, because it is the one that can be edited and the one most people mean.
pub const LISTS: &[Held] = &[
    Held {
        label: "manager (internal)",
        path: PATH,
        editable: true,
        autoloader: false,
    },
    Held {
        label: "autoloader (internal)",
        path: "/data/ps5_autoloader/autoload.txt",
        editable: false,
        autoloader: true,
    },
    Held {
        label: "autoloader (usb0)",
        path: "/mnt/usb0/ps5_autoloader/autoload.txt",
        editable: false,
        autoloader: true,
    },
    Held {
        label: "autoloader (usb1)",
        path: "/mnt/usb1/ps5_autoloader/autoload.txt",
        editable: false,
        autoloader: true,
    },
];

#[cfg(test)]
mod lists {
    use super::{LISTS, PATH};

    /// **Only the internal list is written to.** A list on a stick is the way back in when the
    /// internal setup is broken, and a tool that can damage the recovery path is worse than
    /// one that only reads it.
    #[test]
    fn nothing_on_removable_storage_is_editable() {
        for held in LISTS {
            if held.path.starts_with("/mnt/") {
                assert!(!held.editable, "{} would be written to", held.path);
            }
        }
    }

    /// The manager's own list is the editable one, and it is the path the manager compiles in.
    #[test]
    fn the_managers_list_is_the_one_that_can_be_edited() {
        let editable: Vec<&str> = LISTS
            .iter()
            .filter(|held| held.editable)
            .map(|held| held.path)
            .collect();
        assert_eq!(editable, [PATH]);
    }

    /// **Which kind each is, because the rules invert.** The loader is required in an
    /// autoloader's list and impossible in the manager's.
    #[test]
    fn each_list_says_which_kind_it_is() {
        let manager = LISTS.iter().find(|held| !held.autoloader).expect("one");
        assert_eq!(manager.path, PATH);
        assert!(
            LISTS.iter().filter(|held| held.autoloader).count() >= 2,
            "an autoloader looks in more than one place"
        );
    }
}
