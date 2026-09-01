//! Starting a title on the target.
//!
//! # What this is
//!
//! One shell command. `launch <APPID>`, which asks the target's own system service to start an
//! **installed application** - the same thing selecting it on the home screen does.
//!
//! # What it is not
//!
//! It is not a way to run an ELF. Read in `shsrv/bundles/launch/launch.c`, the builtin is
//! `sceSystemServiceLaunchApp(argv[1], &argv[1], &ctx)` with the foreground user filled in
//! from `sceUserServiceGetForegroundUser`. Nothing about the file on disk is this side's
//! business: the system finds the application by identifier and boots its own signed
//! executable.
//!
//! Running an ELF is a different door. `elfldr` on its own port takes the bytes and spawns
//! them, and the shell's `hbldr` builtin does the same through `elfldr_spawn`. Neither has
//! anything to do with this.
//!
//! # Why it is a module and not a formatted string at the call site
//!
//! Because the identifier has to be checked before it is sent, and what a bad one does is not
//! obvious. The shell splits its line on spaces and offers no quoting, and the builtin hands
//! **everything from the first word onwards** to the application as its own arguments. So a
//! stray word does not start the wrong title - it starts the right one and passes it something
//! nobody meant to pass. Refused rather than trimmed: a selection with a space in it means the
//! selection was wrong, not that it needs tidying.
//!
//! # What it does not promise
//!
//! That the title started. A request that is accepted says nothing about whether a game comes
//! up; that is on the target's own screen.
//!
//! **Refusal, though, is visible.** The builtin calls `perror` on each system call that fails,
//! so a rejection arrives as the name of the call and a reason on the error channel. That is a
//! measured negative, and this reads it rather than reporting every attempt as fine.

/// Every application identifier this recognises.
///
/// `PPSA` and `CUSA` are games and applications; `NPXS` is a system application; `PLDM`,
/// `LAPY` and `PUWX` were seen on a target as homebrew. **The prefix is not checked** - a list
/// of them would be a guess about the next one somebody installs - but the shape is: nine
/// characters, four letters then five digits, no separators.
#[must_use]
pub fn is_an_app_id(id: &str) -> bool {
    let id = id.trim();
    id.len() == 9
        && id.chars().take(4).all(|c| c.is_ascii_alphabetic())
        && id.chars().skip(4).all(|c| c.is_ascii_digit())
}

/// The command that starts a title.
#[must_use]
pub fn command(id: &str) -> String {
    format!("launch {}", id.trim())
}

/// What the target said about a launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// The shell printed its usage, which means it did not accept what it was given.
    NotAnId,
    /// A system call refused, and named itself doing so.
    ///
    /// Measured: the builtin `perror`s each failing call, so the name of the call and an errno
    /// string arrive on the error channel. This is the one negative the target actually draws.
    Refused(String),
    /// It said something else, which is carried rather than interpreted.
    ///
    /// **There is no known reply that means the title started.** A request that is not refused
    /// has been accepted, which is not the same thing, so a version of this that read one line
    /// as success would be inventing a distinction the target does not draw.
    Asked(String),
}

impl Said {
    /// How to put it to somebody.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::NotAnId => {
                "the shell printed its usage, so it did not take that as an application id"
                    .to_owned()
            }
            Self::Refused(why) => format!("the target refused to start it - {why}"),
            Self::Asked(said) if said.is_empty() => {
                "asked the target to start it - whether it did is on its own screen".to_owned()
            }
            Self::Asked(said) => format!(
                "asked the target to start it - whether it did is on its own screen. It said:\n\
                 {said}"
            ),
        }
    }
}

/// Every call the builtin makes, and so every name that can appear in front of a reason.
///
/// Read from the source rather than collected from failures, which is why a refusal this
/// program has never seen is still recognised as one.
const CALLS: [&str; 3] = [
    "sceSystemServiceLaunchApp",
    "sceUserServiceGetForegroundUser",
    "sceUserServiceInitialize",
];

/// Reads what the target said back.
#[must_use]
pub fn read(said: &str) -> Said {
    // Measured: the builtin prints `usage: launch <APPID>` when given no argument, and
    // `perror`s the name of whichever call refused.
    if said.to_ascii_lowercase().contains("usage: launch") {
        return Said::NotAnId;
    }
    if let Some(line) = said
        .lines()
        .map(str::trim)
        .find(|line| CALLS.iter().any(|call| line.starts_with(call)))
    {
        return Said::Refused(line.to_owned());
    }
    Said::Asked(said.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::{Said, command, is_an_app_id, read};

    /// The identifiers measured on a target, and the shapes that are not ones.
    #[test]
    fn an_application_identifier_is_four_letters_and_five_digits() {
        for good in [
            "PPSA21564",
            "CUSA00411",
            "NPXS40172",
            "PLDM00001",
            "LAPY20011",
        ] {
            assert!(is_an_app_id(good), "{good} is one");
        }
        for bad in [
            "PPSA2156",
            "PPSA215644",
            "PPS121564",
            "PPSA2156A",
            "",
            "PPSA 21564",
        ] {
            assert!(!is_an_app_id(bad), "{bad} is not one");
        }
    }

    /// **Anything with a space is not one**, because the builtin would pass the rest of the
    /// line to the application as its arguments.
    #[test]
    fn something_the_shell_would_split_is_refused() {
        assert!(!is_an_app_id("PPSA21564 extra"));
        assert!(!is_an_app_id(" PPSA21564 x"));
    }

    /// Surrounding space is trimmed rather than refused - it comes from a listing, not a
    /// person, and a trailing newline is not somebody's mistake.
    #[test]
    fn space_around_an_identifier_does_not_make_it_wrong() {
        assert!(is_an_app_id("  PPSA21564\n"));
        assert_eq!(command("  PPSA21564\n"), "launch PPSA21564");
    }

    /// The usage line is the shell refusing, and is recognised as that.
    #[test]
    fn the_usage_line_means_it_did_not_take_it() {
        let said = read("[SceLncUtil] something\nusage: launch <APPID>");
        assert_eq!(said, Said::NotAnId);
        assert!(said.describe().contains("did not take"));
    }

    /// A `perror` line from any of the builtin's calls is a refusal, not chatter.
    ///
    /// The names come from the source, so one this program has never seen still reads as a
    /// refusal rather than as a successful launch with something written underneath it.
    #[test]
    fn a_named_system_call_with_a_reason_is_a_refusal() {
        for (line, expected) in [
            (
                "sceSystemServiceLaunchApp: No such file or directory",
                "No such file or directory",
            ),
            (
                "sceUserServiceGetForegroundUser: Bad address",
                "Bad address",
            ),
            (
                "sceUserServiceInitialize: Invalid argument",
                "Invalid argument",
            ),
        ] {
            let said = read(&format!("[SceLncUtil] chatter\n{line}\n"));
            assert_eq!(said, Said::Refused(line.to_owned()), "{line}");
            assert!(said.describe().contains("refused"), "{}", said.describe());
            assert!(said.describe().contains(expected), "{}", said.describe());
        }
    }

    /// **Anything else is *asked*, never *started*.**
    ///
    /// The target draws no distinction this side can see, and a version that read some line as
    /// success would be inventing one.
    #[test]
    fn anything_else_is_asked_rather_than_started() {
        let said = read("");
        assert_eq!(said, Said::Asked(String::new()));
        assert!(
            said.describe().contains("on its own screen"),
            "{}",
            said.describe()
        );
        assert!(!said.describe().contains("started it"));
    }
}
