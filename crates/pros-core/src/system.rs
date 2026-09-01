//! What the target is: firmware, target, storage, and what is running.
//!
//! # Why this is worth a view of its own
//!
//! **Firmware version decides almost everything else on this platform.** Which jailbreak
//! works, which payloads run, whether a game needs backporting before it will start. It is
//! the first thing anybody asks and the first thing anybody has to go and look up somewhere
//! else.
//!
//! # Measured, through the shell, and parsed carefully
//!
//! The shell answers `sysctl`, `df` and `ps`. All three were run against a target and their
//! output shapes are what these parsers were written from - not from a manual page for a
//! different system that happens to have commands with the same names.
//!
//! `sysctl` prints a hex dump rather than a value, so a fact has to be reassembled from the
//! bytes. **The bytes rather than the dump's own ASCII column**, because that column replaces
//! anything unprintable with a dot and there is no way afterwards to tell a real dot from a
//! substituted one.
//!
//! # Nothing here is inferred from anything else here
//!
//! A target that answers `sysctl hw.model` and not `hw.ncpu` reports the model and says
//! nothing about processors. Every fact is separately present or separately absent, because
//! a panel that filled a gap with a plausible value would be indistinguishable from one
//! reporting a measurement.

use std::collections::BTreeMap;

/// One thing the target said about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// What it is called, in words.
    pub name: &'static str,
    /// What the target said.
    pub value: String,
}

/// A filesystem, as `df` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filesystem {
    /// The device or pool.
    pub device: String,
    /// How big.
    pub size: String,
    /// How much is gone.
    pub used: String,
    /// How much is left.
    pub free: String,
    /// How full, as the target puts it.
    pub full: String,
    /// Where it is mounted.
    pub at: String,
}

impl Filesystem {
    /// Whether this is one of a running application's sandbox mounts.
    ///
    /// # Why this matters more than it sounds like it should
    ///
    /// A target measured here listed **1183 filesystems**. Twenty-two of them are the
    /// machine's storage; the other 1161 are bind mounts inside `/mnt/sandbox/<app>`, dozens
    /// per running application, remounting the same handful of pools under different names.
    ///
    /// That is a real property of the platform and not noise to be discarded - so they are
    /// counted and shown, behind a fold, rather than dropped. **What would be wrong is
    /// listing all 1183 flat**, because the ones that answer *how much room is left* would
    /// be somewhere in the middle of it.
    #[must_use]
    pub fn is_a_sandbox_mount(&self) -> bool {
        self.at.starts_with("/mnt/sandbox/")
    }
}

/// A running process, as `ps` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    /// Its identifier.
    pub pid: String,
    /// What state it is in.
    pub state: String,
    /// The title it belongs to, when it belongs to one.
    ///
    /// **Empty for anything that is not a game or application** - a payload, a shell, the
    /// system's own processes. Kept as an empty string rather than a placeholder so a caller
    /// filtering for titles gets titles.
    pub title: String,
    /// What it is called.
    pub command: String,
}

impl Process {
    /// Whether this is a game or application rather than a payload or a system process.
    #[must_use]
    pub fn is_a_title(&self) -> bool {
        // Title identifiers on this platform start PPSA or CUSA. Anything else in the column
        // is a placeholder the listing uses for processes that have none.
        self.title.starts_with("PPSA") || self.title.starts_with("CUSA")
    }
}

/// The `sysctl` keys worth asking about, and what to call them.
///
/// **Each was tried on a target.** `machdep.idle` and `hw.physmem` were not: they answered
/// *no such file or directory*, so they are not here. A key that only exists on some other
/// system would show as permanently unavailable and look like a fault.
pub const FACTS: &[(&str, &str)] = &[
    ("kern.version", "firmware"),
    ("hw.model", "model"),
    ("hw.ncpu", "processors"),
    ("kern.osrelease", "kernel"),
];

/// Reassembles the value out of what `sysctl` prints.
///
/// The shell prints a hex dump: an offset, the bytes, then a rendering. **The bytes are what
/// is read**, because the rendering shows a dot for anything unprintable and nothing
/// afterwards can tell those apart from real ones.
///
/// Returns the text with trailing padding and zero bytes removed - both of which the measured
/// values carried.
#[must_use]
pub fn value_in(dump: &str) -> Option<String> {
    let mut bytes = Vec::new();
    for line in dump.lines() {
        // Everything up to the rendering, minus the offset that starts the line.
        let hex = line.split('|').next()?;
        let mut columns = hex.split_whitespace();
        // The offset itself is eight hex digits and is not data.
        let first = columns.next()?;
        if first.len() != 8 || !first.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        for column in columns {
            if column.len() == 2
                && let Ok(byte) = u8::from_str_radix(column, 16)
            {
                bytes.push(byte);
            }
        }
    }
    if bytes.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes)
        .trim_end_matches('\0')
        .trim()
        .to_owned();
    (!text.is_empty()).then_some(text)
}

/// A `sysctl` value that is a number rather than text.
///
/// Four bytes, least significant first, which is what the target returned for `hw.ncpu`.
#[must_use]
pub fn number_in(dump: &str) -> Option<u32> {
    let mut bytes = Vec::new();
    for line in dump.lines() {
        let hex = line.split('|').next()?;
        let mut columns = hex.split_whitespace();
        let first = columns.next()?;
        if first.len() != 8 || !first.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        for column in columns {
            if column.len() == 2
                && let Ok(byte) = u8::from_str_radix(column, 16)
            {
                bytes.push(byte);
            }
        }
    }
    let four: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    Some(u32::from_le_bytes(four))
}

/// Reads a `df` listing.
///
/// Skips the header and anything that does not have the six columns it prints, rather than
/// failing the whole listing for one odd line.
#[must_use]
pub fn storage(output: &str) -> Vec<Filesystem> {
    let mut found = Vec::new();
    for line in output.lines() {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 6 || columns[0] == "Filesystem" {
            continue;
        }
        // Taken from the end, because a device name can carry spaces and a mount point
        // cannot be mistaken for anything else.
        let at = columns[columns.len() - 1];
        if !at.starts_with('/') {
            continue;
        }
        found.push(Filesystem {
            device: columns[0].to_owned(),
            size: columns[columns.len() - 5].to_owned(),
            used: columns[columns.len() - 4].to_owned(),
            free: columns[columns.len() - 3].to_owned(),
            full: columns[columns.len() - 2].to_owned(),
            at: at.to_owned(),
        });
    }
    found
}

/// Reads a `ps` listing.
///
/// The columns measured are: pid, ppid, pgid, sid, uid, state, appid, titleid, memory, then
/// the command - and the title column is blank for anything that is not a title, which is why
/// it cannot be found by counting from the left.
#[must_use]
pub fn processes(output: &str) -> Vec<Process> {
    let mut found = Vec::new();
    for line in output.lines() {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 8 || columns[0] == "PID" || !columns[0].chars().all(char::is_numeric) {
            continue;
        }
        let title = columns
            .iter()
            .find(|column| column.starts_with("PPSA") || column.starts_with("CUSA"))
            .copied()
            .unwrap_or_default();
        found.push(Process {
            pid: columns[0].to_owned(),
            state: columns[5].to_owned(),
            title: title.to_owned(),
            command: (*columns.last().unwrap_or(&"")).to_owned(),
        });
    }
    found
}

/// Everything the target said, ready to show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// One per [`FACTS`] entry that answered.
    pub facts: Vec<Fact>,
    /// Every filesystem `df` listed.
    pub storage: Vec<Filesystem>,
    /// Everything `ps` listed.
    pub processes: Vec<Process>,
}

impl Report {
    /// Builds one from the outputs of the three commands.
    ///
    /// `answers` maps a `sysctl` key to what the target printed for it. **A key that is
    /// missing from the map is missing from the report** rather than present and empty - the
    /// difference between *this target did not say* and *this target said nothing*.
    #[must_use]
    pub fn from(answers: &BTreeMap<String, String>, df: &str, ps: &str) -> Self {
        let mut facts = Vec::new();
        for (key, name) in FACTS {
            let Some(dump) = answers.get(*key) else {
                continue;
            };
            // Processors came back as a four-byte number; everything else as text.
            let value = if *key == "hw.ncpu" {
                number_in(dump).map(|count| count.to_string())
            } else {
                value_in(dump)
            };
            if let Some(value) = value {
                facts.push(Fact { name, value });
            }
        }
        Self {
            facts,
            storage: storage(df),
            processes: processes(ps),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Report, number_in, processes, storage, value_in};

    /// Exactly what a target printed for `sysctl kern.version`.
    fn firmware_dump() -> &'static str {
        "00000000  72 32 32 36 39 37 34 2f 72 65 6c 65 61 73 65 73 | r226974/releases\n\
         00000010  2f 31 32 2e 34 30 20 4e 6f 76 20 32 37 20 32 30 | /12.40 Nov 27 20\n"
    }

    /// **The firmware comes out of the bytes**, which is the fact everything else on this
    /// platform depends on.
    #[test]
    fn the_firmware_is_reassembled_from_what_the_target_printed() {
        let value = value_in(firmware_dump()).expect("it reads");
        assert!(value.starts_with("r226974/releases/12.40"), "{value}");
    }

    /// A four-byte value is a number, least significant byte first - as measured.
    #[test]
    fn a_count_is_read_as_a_number() {
        let dump = "00000000  10 00 00 00                                     | ....\n";
        assert_eq!(number_in(dump), Some(16));
    }

    /// **Nothing at all gives nothing**, rather than an empty string that would sit in the
    /// panel looking like a target that answered.
    #[test]
    fn a_key_that_printed_nothing_is_absent_rather_than_blank() {
        assert_eq!(value_in("sysctl: No such file or directory"), None);
        assert_eq!(value_in(""), None);
        assert_eq!(number_in(""), None);
    }

    /// The storage listing a target printed, read back.
    #[test]
    fn the_filesystems_a_target_listed_are_read() {
        let df = "Filesystem                Size     Used    Avail  Capacity  Mounted on\n\
                  md0                       7.0M     6.2M   756.0K       89%  /\n\
                  /dev/ssd0.system        639.8M   472.6M   167.2M       73%  /system\n\
                  ssd0.user               624.6G    10.8G   605.4G        1%  /user\n";
        let found = storage(df);
        assert_eq!(found.len(), 3);
        let user = found.iter().find(|one| one.at == "/user").expect("there");
        assert_eq!(user.size, "624.6G");
        assert_eq!(user.free, "605.4G");
        assert_eq!(user.full, "1%");
    }

    /// **A title is found by its shape, not by its column.**
    ///
    /// The column is blank for every process that is not a game, so counting across would put
    /// the memory figure in the title field for most rows - and `4.7` would look like an
    /// identifier to anything that did not know better.
    #[test]
    fn a_running_title_is_recognised_and_a_payload_is_not() {
        let ps = "     PID      PPID     PGID      SID      UID      State  AppId    TitleId     Memory (MiB)  Command\n\
                       362       361      361      361        0        RUN   0000                4.7 /   21.4  ps\n\
                       182        54       54       54        1      SLEEP   4018  PPSA02664   833.0 /  867.9  eboot.bin\n\
                       171       168      168      168        0      SLEEP   0000                4.2 /   18.8  ftpsrv.elf\n";
        let found = processes(ps);
        assert_eq!(found.len(), 3);

        let titles: Vec<&super::Process> = found.iter().filter(|one| one.is_a_title()).collect();
        assert_eq!(titles.len(), 1, "one of these is a game");
        assert_eq!(titles[0].title, "PPSA02664");
        assert_eq!(titles[0].command, "eboot.bin");

        let payload = found
            .iter()
            .find(|one| one.command == "ftpsrv.elf")
            .expect("there");
        assert!(!payload.is_a_title());
        assert!(
            payload.title.is_empty(),
            "a payload has no title, not a blank one"
        );
    }

    /// A target that answers some keys and not others reports what it answered.
    #[test]
    fn a_target_that_answered_half_the_questions_reports_half_the_answers() {
        let mut answers = BTreeMap::new();
        answers.insert("kern.version".to_owned(), firmware_dump().to_owned());
        answers.insert(
            "hw.ncpu".to_owned(),
            "00000000  10 00 00 00  | ....\n".to_owned(),
        );

        let report = Report::from(&answers, "", "");
        assert_eq!(report.facts.len(), 2, "{:?}", report.facts);
        assert_eq!(report.facts[0].name, "firmware");
        assert_eq!(report.facts[1].name, "processors");
        assert_eq!(report.facts[1].value, "16");
        assert!(report.storage.is_empty());
    }

    /// **The machine's storage is told apart from a running application's mounts.**
    ///
    /// A target listed 1183 filesystems, twenty-two of which are the machine. Without this
    /// the figure somebody came to read is one row in a thousand.
    #[test]
    fn a_sandbox_mount_is_not_the_targets_storage() {
        let df = "Filesystem   Size   Used  Avail  Capacity  Mounted on
                  ssd0.user  624.6G  10.8G 605.4G        1%  /user
                  /user/catalog_downloader/appmeta 624.6G 10.8G 605.4G 1% /mnt/sandbox/NPXS40093_000/user/catalog_downloader/appmeta
                  /mnt/rnps2   2.0M 400.0K   1.6M       19%  /mnt/sandbox/NPXS40093_000/mnt/rnps2
";
        let found = storage(df);
        assert_eq!(found.len(), 3);
        assert_eq!(
            found.iter().filter(|one| !one.is_a_sandbox_mount()).count(),
            1,
            "only /user is the machine's own"
        );
        assert!(!found[0].is_a_sandbox_mount());
        assert!(found[1].is_a_sandbox_mount());
        assert!(found[2].is_a_sandbox_mount());
    }
}
