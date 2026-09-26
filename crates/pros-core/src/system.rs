//! What the target is: firmware, target, storage, and what is running.
//!
//! The firmware version decides which entry point, payloads and titles run on a target.
//! These are pure parsers of the shell's `sysctl`, `df` and `ps` output, written from what a
//! target printed rather than from another system's manual pages; running the commands is
//! left to the shim. (D027)
//!
//! Every fact is separately present or absent: nothing is inferred from another fact, and a
//! gap is never filled with a plausible value.

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
    /// On a measured target nearly all `df` rows were bind mounts under `/mnt/sandbox/<app>`,
    /// remounting the same few pools. They are shown behind a fold rather than dropped, so the
    /// machine's own storage is not lost among them.
    #[must_use]
    pub fn is_a_sandbox_mount(&self) -> bool {
        self.at.starts_with("/mnt/sandbox/")
    }
}

/// The memory a process is using and the most it has used, in MiB, as `ps` prints them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Memory {
    /// In use now, MiB, as the target printed it.
    pub current: String,
    /// The most it has used, MiB.
    pub peak: String,
}

impl Memory {
    /// The current figure as a number, for sorting a listing by it.
    ///
    /// `None` when what the target printed was not a plain number, so such a row sorts as
    /// absent rather than as zero.
    #[must_use]
    pub fn current_mib(&self) -> Option<f64> {
        self.current.parse().ok()
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
    /// Empty for anything that is not a game or application: a payload, a shell, the system's
    /// own processes.
    pub title: String,
    /// What it is called.
    pub command: String,
    /// How much memory it is using, when the listing carried the figure.
    ///
    /// `None` rather than zero for a row without the measured `current / peak` MiB shape.
    pub memory: Option<Memory>,
}

impl Process {
    /// Whether this is a game or application rather than a payload or a system process.
    #[must_use]
    pub fn is_a_title(&self) -> bool {
        // `processes` fills the title only for a game or application.
        !self.title.is_empty()
    }
}

/// The `sysctl` keys worth asking about, and what to call them.
///
/// Each answers on a target. `machdep.idle` and `hw.physmem` answer "no such file or
/// directory" there, so they are left out rather than shown as permanently unavailable.
pub const FACTS: &[(&str, &str)] = &[
    ("kern.version", "firmware"),
    ("hw.model", "model"),
    ("hw.ncpu", "processors"),
    ("kern.osrelease", "kernel"),
];

/// Reassembles the value out of what `sysctl` prints.
///
/// The shell prints a hex dump: an offset, the bytes, then a rendering. The bytes are read,
/// because the rendering shows unprintable bytes as dots indistinguishable from real ones.
///
/// Returns the text with trailing padding and zero bytes removed; the measured values carry
/// both.
#[must_use]
pub fn value_in(dump: &str) -> Option<String> {
    let mut bytes = Vec::new();
    for line in dump.lines() {
        let hex = line.split('|').next()?;
        let mut columns = hex.split_whitespace();
        // The offset is eight hex digits and is not data.
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
/// Four bytes, least significant first, as the target returns for `hw.ncpu`.
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

/// Whether `column` has the shape of a title identifier: four capital letters, then five digits.
///
/// Measured off a target's `ps`: `PPSA` and `CUSA` ids are retail games, `NPXS40087` is the
/// system's own shell, `GLCB00001` and `PUWX90000` are homebrew. Matching the shape rather than
/// the retail prefixes finds homebrew titles too. A process with no title shows a memory
/// figure such as `4.7` in that column, which does not fit.
#[must_use]
pub fn is_a_title_id(column: &str) -> bool {
    column.len() == 9
        && column.bytes().take(4).all(|b| b.is_ascii_uppercase())
        && column.bytes().skip(4).all(|b| b.is_ascii_digit())
}

/// Whether a title identifier is the system's own rather than a game's or an application's.
///
/// Every system process on a target carried the `NPXS` prefix - `SceShellUI` is `NPXS40087`,
/// `SceSysCore` is `NPXS45091` - and none of them is something `close` should find.
#[must_use]
pub fn is_the_systems_own(id: &str) -> bool {
    id.starts_with("NPXS")
}

/// Reads a `ps` listing.
///
/// The measured columns are: pid, ppid, pgid, sid, uid, state, appid, titleid, memory, then
/// the command. The title column is blank for anything that is not a title, so counting from
/// the left puts the memory figure there for most rows; [`is_a_title_id`] tells them apart.
#[must_use]
pub fn processes(output: &str) -> Vec<Process> {
    let mut found = Vec::new();
    for line in output.lines() {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.len() < 8 || columns[0] == "PID" || !columns[0].chars().all(char::is_numeric) {
            continue;
        }
        let title = columns
            .get(7)
            .copied()
            .filter(|column| is_a_title_id(column) && !is_the_systems_own(column))
            .unwrap_or_default();
        found.push(Process {
            pid: columns[0].to_owned(),
            state: columns[5].to_owned(),
            title: title.to_owned(),
            command: (*columns.last().unwrap_or(&"")).to_owned(),
            memory: memory_in(&columns),
        });
    }
    found
}

/// The memory figure at the end of a `ps` row, when it has the measured `current / peak` shape.
///
/// Read from the end, because the optional title column shifts every fixed index: the command
/// is the last token and `current / peak` the three before it. Both figures must parse as
/// numbers, so a stray slash is not mistaken for a memory reading.
#[must_use]
fn memory_in(columns: &[&str]) -> Option<Memory> {
    let n = columns.len();
    // ... <current> "/" <peak> <command>
    if n >= 4 && columns[n - 3] == "/" {
        let current = columns[n - 4];
        let peak = columns[n - 2];
        if current.parse::<f64>().is_ok() && peak.parse::<f64>().is_ok() {
            return Some(Memory {
                current: current.to_owned(),
                peak: peak.to_owned(),
            });
        }
    }
    None
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
    /// `answers` maps a `sysctl` key to what the target printed for it. A key missing from the
    /// map is missing from the report rather than present and empty.
    #[must_use]
    pub fn from(answers: &BTreeMap<String, String>, df: &str, ps: &str) -> Self {
        let mut facts = Vec::new();
        for (key, name) in FACTS {
            let Some(dump) = answers.get(*key) else {
                continue;
            };
            // Processors come back as a four-byte number; everything else as text.
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

// Process control: find a process in the listing and build the command that signals it, to
// restart the user interface or close a title. Running the command stays in the shim.

/// The command name the current-generation user interface runs under.
///
/// Killing it clears a UI softlock without a reboot: the system's own `SceSysCore` respawns
/// it (measured on a target).
pub const SHELL_UI: &str = "SceShellUI";

/// A signal to send with the target's `kill`, by the number its builtin takes.
///
/// `Terminate` suits the user interface, which is meant to come back; `Kill` is for a title
/// that must go now; `Continue` wakes a stopped process so its teardown can finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// SIGTERM. Ask a process to end - what a bare `kill` sends.
    Terminate,
    /// SIGCONT. Wake a stopped process so a pending exit can complete.
    Continue,
    /// SIGKILL. End a process that will not go on its own.
    Kill,
}

impl Signal {
    /// The number the target's `kill -s` expects.
    #[must_use]
    pub fn number(self) -> u8 {
        match self {
            Self::Terminate => 15,
            Self::Continue => 19,
            Self::Kill => 9,
        }
    }
}

/// The shell command that sends `signal` to `pid`.
///
/// Always an explicit `-s <n>`, so the signal is stated rather than defaulted.
#[must_use]
pub fn kill(pid: &str, signal: Signal) -> String {
    format!("kill -s {} {}", signal.number(), pid.trim())
}

/// The user-interface process in a listing, if it is running.
#[must_use]
pub fn shell_ui(processes: &[Process]) -> Option<&Process> {
    processes.iter().find(|p| p.command == SHELL_UI)
}

/// The process with this pid in a listing, if it is running.
///
/// Matched exactly after trimming, as [`kill`] trims what it signals. `None` lets the caller
/// report a pid nothing is using rather than signal it.
#[must_use]
pub fn by_pid<'a>(processes: &'a [Process], pid: &str) -> Option<&'a Process> {
    let pid = pid.trim();
    processes.iter().find(|p| p.pid == pid)
}

/// Every process a listing attributes to a title.
///
/// Matched by the title column, and also by the command carrying the id, because a homebrew
/// title launched as a bare payload has no title column of its own.
#[must_use]
pub fn of_title<'a>(processes: &'a [Process], id: &str) -> Vec<&'a Process> {
    let id = id.trim();
    processes
        .iter()
        .filter(|p| p.title == id || (!id.is_empty() && p.command.contains(id)))
        .collect()
}

/// The kill commands that end one process, in the order they must be sent.
///
/// A process in `STOP` state is sent `Continue` first: a stopped title does not run its exit
/// teardown, and killing it while stopped leaves locked vnodes behind (measured on a target).
/// Anything else is a single `Kill`.
#[must_use]
pub fn end(process: &Process) -> Vec<String> {
    let mut commands = Vec::new();
    if process.state == "STOP" {
        commands.push(kill(&process.pid, Signal::Continue));
    }
    commands.push(kill(&process.pid, Signal::Kill));
    commands
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        Memory, Process, Report, SHELL_UI, Signal, by_pid, end, is_a_title_id, kill, number_in,
        of_title, processes, shell_ui, storage, value_in,
    };

    /// Exactly what a target printed for `sysctl kern.version`.
    fn firmware_dump() -> &'static str {
        "00000000  72 32 32 36 39 37 34 2f 72 65 6c 65 61 73 65 73 | r226974/releases\n\
         00000010  2f 31 32 2e 34 30 20 4e 6f 76 20 32 37 20 32 30 | /12.40 Nov 27 20\n"
    }

    /// The firmware version is reassembled from the dump's bytes.
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

    /// A key that printed nothing is absent, not an empty string.
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

    /// A title is recognised by its id's shape, so a memory figure in that column is not one.
    #[test]
    fn a_running_title_is_recognised_and_a_payload_is_not() {
        let ps = "     PID      PPID     PGID      SID      UID      State  AppId    TitleId     Memory (MiB)  Command\n\
                       362       361      361      361        0        RUN   0000                4.7 /   21.4  ps\n\
                       182        54       54       54        1      SLEEP   4018  PPSA02664   833.0 /  867.9  eboot.bin\n\
                       171       168      168      168        0      SLEEP   0000                4.2 /   18.8  ftpsrv.elf\n";
        let found = processes(ps);
        assert_eq!(found.len(), 3);

        let titles: Vec<&Process> = found.iter().filter(|one| one.is_a_title()).collect();
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

    /// A homebrew title is a title, and the system's own `NPXS` processes are not.
    #[test]
    fn a_homebrew_title_is_a_title_and_the_system_is_not() {
        let ps = "     PID      PPID     PGID      SID      UID      State  AppId    TitleId     Memory (MiB)  Command\n\
                       274        55       55       55        1        RUN   c018  GLCB00001    82.8 /   90.8  eboot.bin\n\
                       266        55       55       55        0      SLEEP   4007  NPXS40087   740.5 / 3782.4  SceShellUI\n\
                       289       288      288      288        0        RUN   0000                4.7 /   21.4  ps\n";
        let found = processes(ps);
        assert_eq!(found.len(), 3);

        let mine = of_title(&found, "GLCB00001");
        assert_eq!(mine.len(), 1, "the homebrew title is closable");
        assert_eq!(mine[0].pid, "274");
        assert!(mine[0].is_a_title());

        let ui = shell_ui(&found).expect("the shell is listed");
        assert!(
            !ui.is_a_title(),
            "the system's own processes are not titles"
        );
        assert!(ui.title.is_empty());
        assert!(
            of_title(&found, "NPXS40087").is_empty(),
            "and cannot be closed as one"
        );

        assert!(is_a_title_id("PPSA02664"));
        assert!(is_a_title_id("CUSA00001"));
        assert!(!is_a_title_id("4.7"));
        assert!(!is_a_title_id("eboot.bin"));
        assert!(!is_a_title_id("PPSA0266"));
    }

    /// The memory figure is read from the end, for titles and payloads alike.
    #[test]
    fn the_memory_figure_is_read_for_titles_and_payloads_alike() {
        let ps = "     PID      PPID     PGID      SID      UID      State  AppId    TitleId     Memory (MiB)  Command\n\
                       182        54       54       54        1      SLEEP   4018  PPSA02664   833.0 /  867.9  eboot.bin\n\
                       171       168      168      168        0      SLEEP   0000                4.2 /   18.8  ftpsrv.elf\n";
        let found = processes(ps);
        let title = found
            .iter()
            .find(|p| p.title == "PPSA02664")
            .expect("the game");
        assert_eq!(
            title.memory,
            Some(Memory {
                current: "833.0".to_owned(),
                peak: "867.9".to_owned()
            })
        );
        assert_eq!(
            title.memory.as_ref().and_then(Memory::current_mib),
            Some(833.0),
            "the current figure is a number for sorting"
        );

        let payload = found
            .iter()
            .find(|p| p.command == "ftpsrv.elf")
            .expect("the payload");
        assert_eq!(
            payload.memory,
            Some(Memory {
                current: "4.2".to_owned(),
                peak: "18.8".to_owned()
            }),
            "a payload's memory is read the same way, with no title id in front of it"
        );
    }

    /// A row without the `current / peak` shape reports no memory rather than zero.
    #[test]
    fn a_row_without_the_memory_shape_reports_none() {
        let ps = "PID PPID PGID SID UID STATE APPID TITLEID MEM COMMAND\n\
                  100 1 100 100 0 S - - 1M SceShellUI\n";
        let found = processes(ps);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].memory, None,
            "a single-token memory column is not the measured shape"
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

    /// The machine's storage is told apart from a running application's sandbox mounts.
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

    /// A short `ps` fixture: the user interface, a running title, and a stopped one.
    fn listing() -> Vec<Process> {
        processes(
            "PID PPID PGID SID UID STATE APPID TITLEID MEM COMMAND\n\
             100 1 100 100 0 S - - 1M SceShellUI\n\
             200 1 200 200 1 S PPSA00001 PPSA00001 8M eboot.bin\n\
             300 1 300 300 1 STOP PPSA99980 PPSA99980 4M eboot.bin\n",
        )
    }

    /// The signal numbers are the ones the target's `kill` takes.
    #[test]
    fn signals_are_the_measured_numbers() {
        assert_eq!(Signal::Terminate.number(), 15);
        assert_eq!(Signal::Continue.number(), 19);
        assert_eq!(Signal::Kill.number(), 9);
        assert_eq!(kill("  200\n", Signal::Kill), "kill -s 9 200");
    }

    /// The user interface is found by its command name, not its position.
    #[test]
    fn the_shell_ui_is_found_by_name() {
        let found = listing();
        assert_eq!(shell_ui(&found).expect("it is running").pid, "100");
        assert!(shell_ui(&processes("PID STATE COMMAND\n1 S launchd\n")).is_none());
    }

    /// A title's processes are found by the title column.
    #[test]
    fn a_titles_processes_are_found() {
        let found = listing();
        let mine = of_title(&found, "PPSA00001");
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].pid, "200");
    }

    /// A process is found by its trimmed pid, and an unused pid is `None`.
    #[test]
    fn a_process_is_found_by_its_pid() {
        let found = listing();
        assert_eq!(by_pid(&found, "200").expect("it is running").pid, "200");
        assert_eq!(
            by_pid(&found, "  100\n").expect("trimmed").command,
            SHELL_UI
        );
        assert!(by_pid(&found, "999999").is_none());
    }

    /// A running process is killed outright; a stopped one is woken first.
    #[test]
    fn a_stopped_process_is_woken_before_it_is_killed() {
        let found = listing();
        let running = of_title(&found, "PPSA00001");
        assert_eq!(end(running[0]), ["kill -s 9 200"]);

        let stopped = of_title(&found, "PPSA99980");
        assert_eq!(end(stopped[0]), ["kill -s 19 300", "kill -s 9 300"]);
    }
}
