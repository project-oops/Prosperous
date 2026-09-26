//! Starting an installed title on the target.
//!
//! `launch <APPID>` asks the target's system service to start an installed application, as
//! selecting it on the home screen does. Per `shsrv/bundles/launch/launch.c` the builtin calls
//! `sceSystemServiceLaunchApp(argv[1], &argv[1], &ctx)` for the foreground user; it does not
//! run an ELF (that is `elfldr` or `hbldr`). The shell splits on spaces without quoting and the
//! builtin passes every word after the identifier to the application as arguments, so an
//! identifier with a space is refused rather than trimmed. An accepted request does not mean
//! the title started; a refusal is visible, because the builtin `perror`s each failing call.

/// Whether this has the shape of an application identifier.
///
/// Nine characters, four letters then five digits. The prefix is not checked: `PPSA`, `CUSA`,
/// `NPXS` and homebrew prefixes such as `PLDM`, `LAPY` and `PUWX` are all seen on targets.
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
    /// The builtin `perror`s each failing call, so the call's name and an errno string arrive
    /// on the error channel.
    Refused(String),
    /// It said something else, which is carried rather than interpreted.
    ///
    /// No reply means the title started: a request that is not refused has only been accepted.
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
/// Read from `launch.c`, so a refusal never seen before is still recognised.
const CALLS: [&str; 3] = [
    "sceSystemServiceLaunchApp",
    "sceUserServiceGetForegroundUser",
    "sceUserServiceInitialize",
];

/// Reads what the target said back.
#[must_use]
pub fn read(said: &str) -> Said {
    // `launch.c` prints `usage: launch <APPID>` when given no argument.
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

    /// Identifiers seen on targets are accepted, and other shapes are not.
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

    /// An identifier followed by more words is refused, since the rest would become arguments.
    #[test]
    fn something_the_shell_would_split_is_refused() {
        assert!(!is_an_app_id("PPSA21564 extra"));
        assert!(!is_an_app_id(" PPSA21564 x"));
    }

    /// Surrounding whitespace, as from a listing, is trimmed rather than refused.
    #[test]
    fn space_around_an_identifier_does_not_make_it_wrong() {
        assert!(is_an_app_id("  PPSA21564\n"));
        assert_eq!(command("  PPSA21564\n"), "launch PPSA21564");
    }

    /// The usage line is recognised as the shell refusing.
    #[test]
    fn the_usage_line_means_it_did_not_take_it() {
        let said = read("[SceLncUtil] something\nusage: launch <APPID>");
        assert_eq!(said, Said::NotAnId);
        assert!(said.describe().contains("did not take"));
    }

    /// A `perror` line from any of the builtin's calls is a refusal.
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

    /// Any other reply is described as asked, never as started.
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
