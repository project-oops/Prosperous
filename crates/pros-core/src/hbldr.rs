//! Running an ELF the target already has.
//!
//! # The third way something runs, and the only one that moves nothing
//!
//! - The loader on its own port takes **bytes from here** and spawns them.
//! - `launch` takes an **identifier** and asks the system to boot an installed application.
//! - This takes a **path on the target's own disk**.
//!
//! Read in `shsrv/bundles/hbldr/hbldr.c`: the builtin resolves its argument with `which` and
//! spawns it through the same `elfldr_spawn` the loader port uses. `hbdbg` is the same
//! program waiting for a debugger, and is not offered here - a payload stopped before its
//! first instruction, with nothing on this side able to attach, is a target that looks hung.
//!
//! # Why it earns its place
//!
//! Because the alternative advice was wrong. A service that is not answering, whose payload
//! is **already sitting on the target**, was being met with an offer to download it - and
//! after downloading, to send a second copy of a file that was already there. This is the
//! action that matches the situation.

/// The command that runs a payload already on the target.
#[must_use]
pub fn command(path: &str) -> String {
    format!("hbldr {}", path.trim())
}

/// Whether the shell would take this as one argument.
///
/// **The same rule the installer and `launch` follow**, for the same measured reason: the
/// shell splits its line on spaces and offers no quoting, so a path with a space in it
/// arrives as two arguments and the first one is what runs - or fails to.
#[must_use]
pub fn is_one_argument(path: &str) -> bool {
    let path = path.trim();
    !path.is_empty() && !path.contains(char::is_whitespace)
}

/// What the target said about running it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// The shell could not find what it was asked to run.
    ///
    /// Measured from the source: `hbldr` resolves with `which` and prints
    /// `<name>: command not found` when that fails.
    NotFound(String),
    /// It printed its usage, so it did not accept the argument.
    NoArgument,
    /// Something else, carried rather than interpreted.
    ///
    /// **Not read as success.** The builtin waits for the payload and returns its exit
    /// status, but a payload that loads and then sits there serving a port has not exited and
    /// will not - so silence here means *running*, *still starting*, or *gone wrong quietly*,
    /// and this side cannot tell those apart. The check answers that question properly, by
    /// asking the port.
    Ran(String),
}

impl Said {
    /// How to put it to somebody.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::NotFound(what) => {
                format!("the target could not find {what} - it may have moved or been deleted")
            }
            Self::NoArgument => {
                "the shell printed its usage, so it did not take that path".to_owned()
            }
            Self::Ran(said) if said.is_empty() => {
                "asked the target to run it - check again to see whether its port answers"
                    .to_owned()
            }
            Self::Ran(said) => format!(
                "asked the target to run it - check again to see whether its port answers. \
                 It said:\n{said}"
            ),
        }
    }
}

/// Reads what the target said back.
#[must_use]
pub fn read(said: &str) -> Said {
    let lower = said.to_ascii_lowercase();
    if lower.contains("usage: hbldr") || lower.contains("usage: %s") {
        return Said::NoArgument;
    }
    // Measured: `printf("%s: command not found\n", argv[1])`.
    if let Some(line) = said
        .lines()
        .map(str::trim)
        .find(|line| line.ends_with(": command not found"))
    {
        let what = line.trim_end_matches(": command not found").to_owned();
        return Said::NotFound(what);
    }
    Said::Ran(said.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::{Said, command, is_one_argument, read};

    /// A path is passed through as it is, with surrounding space trimmed.
    #[test]
    fn the_command_is_the_path_it_was_given() {
        assert_eq!(
            command("  /data/pldmgr/payloads/klogsrv/klogsrv_v0.9.elf\n"),
            "hbldr /data/pldmgr/payloads/klogsrv/klogsrv_v0.9.elf"
        );
    }

    /// **A path with a space is refused before it is sent**, because the shell would split it.
    #[test]
    fn something_the_shell_would_split_is_not_one_argument() {
        assert!(is_one_argument(
            "/data/pldmgr/payloads/klogsrv/klogsrv_v0.9.elf"
        ));
        assert!(!is_one_argument("/data/my payloads/thing.elf"));
        assert!(!is_one_argument(""));
        assert!(!is_one_argument("   "));
    }

    /// The measured failure is recognised, and carries what could not be found.
    #[test]
    fn a_path_the_target_cannot_resolve_is_a_failure() {
        let said = read("/data/gone.elf: command not found");
        assert_eq!(said, Said::NotFound("/data/gone.elf".to_owned()));
        assert!(said.describe().contains("/data/gone.elf"));
    }

    /// **Anything else is *asked*, never *running*.**
    ///
    /// A payload that loads and stays up never exits, so there is no reply that means it
    /// worked. The port is what answers that, and the description says so.
    #[test]
    fn anything_else_is_asked_rather_than_running() {
        let said = read("");
        assert_eq!(said, Said::Ran(String::new()));
        assert!(said.describe().contains("check again"));
        assert!(!said.describe().contains("is running"));
    }
}
