//! Running an ELF that is already on the target's own disk.
//!
//! The loader port takes bytes from here and `launch` takes an installed application's
//! identifier; this takes a path on the target. Per `shsrv/bundles/hbldr/hbldr.c`, the builtin
//! resolves its argument with `which` and spawns it through the same `elfldr_spawn` the loader
//! port uses. `hbdbg` is not offered: it stops the program before its first instruction to
//! wait for a debugger, and nothing on this side can attach, so the target looks hung.

/// The command that runs a payload already on the target.
#[must_use]
pub fn command(path: &str) -> String {
    format!("hbldr {}", path.trim())
}

/// Whether the shell would take this as one argument.
///
/// The shell splits its line on spaces and offers no quoting, so a path with a space arrives
/// as two arguments and the first one is what runs.
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
    /// `hbldr` resolves with `which` and prints `<name>: command not found` when that fails.
    NotFound(String),
    /// It printed its usage, so it did not accept the argument.
    NoArgument,
    /// Something else, carried rather than interpreted.
    ///
    /// Not read as success: a payload that loads and then serves a port never exits, so
    /// silence means running, still starting or failed quietly. The check answers that by
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
    // `hbldr.c` prints `printf("%s: command not found\n", argv[1])`.
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

    /// A path with a space is refused before it is sent, because the shell would split it.
    #[test]
    fn something_the_shell_would_split_is_not_one_argument() {
        assert!(is_one_argument(
            "/data/pldmgr/payloads/klogsrv/klogsrv_v0.9.elf"
        ));
        assert!(!is_one_argument("/data/my payloads/thing.elf"));
        assert!(!is_one_argument(""));
        assert!(!is_one_argument("   "));
    }

    /// The not-found reply is recognised and carries what could not be found.
    #[test]
    fn a_path_the_target_cannot_resolve_is_a_failure() {
        let said = read("/data/gone.elf: command not found");
        assert_eq!(said, Said::NotFound("/data/gone.elf".to_owned()));
        assert!(said.describe().contains("/data/gone.elf"));
    }

    /// Any other reply is described as asked, never as running.
    #[test]
    fn anything_else_is_asked_rather_than_running() {
        let said = read("");
        assert_eq!(said, Said::Ran(String::new()));
        assert!(said.describe().contains("check again"));
        assert!(!said.describe().contains("is running"));
    }
}
