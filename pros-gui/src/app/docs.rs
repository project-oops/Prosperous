//! The pages the documentation reader ships.

/// The pages this build ships, and their order in the reader.
///
/// `include_str!` puts them in the binary, so they match the build. Only the user guide is
/// listed; development records stay in the repository.
pub(super) const DOCS: &[oops_docs::Doc] = &[
    oops_docs::Doc::new(
        "getting-started",
        "Getting started",
        "Where things are kept, the services, and reading a check",
        include_str!("../../../docs/guide/getting-started.md"),
    ),
    oops_docs::Doc::new(
        "targets",
        "Targets",
        "Registering a target, and asking what it can do",
        include_str!("../../../docs/guide/targets.md"),
    ),
    oops_docs::Doc::new(
        "logs",
        "Logs",
        "Following the target's system log",
        include_str!("../../../docs/guide/logs.md"),
    ),
    oops_docs::Doc::new(
        "files",
        "Files",
        "Moving files and folders, and restoring a build",
        include_str!("../../../docs/guide/files.md"),
    ),
    oops_docs::Doc::new(
        "titles",
        "Titles",
        "Launching and closing titles, processes, and the probe loop",
        include_str!("../../../docs/guide/titles.md"),
    ),
    oops_docs::Doc::new(
        "shell",
        "Shell",
        "Running one command on the target",
        include_str!("../../../docs/guide/shell.md"),
    ),
    oops_docs::Doc::new(
        "payloads",
        "Payloads",
        "Fetching, verifying and sending payloads, and the startup list",
        include_str!("../../../docs/guide/payloads.md"),
    ),
    oops_docs::Doc::new(
        "library",
        "Library",
        "Titles, saves, packages and cheats on the target's storage",
        include_str!("../../../docs/guide/library.md"),
    ),
];

#[cfg(test)]
mod docs_tests {
    /// No page is empty or headingless, and no two entries share a slug.
    #[test]
    fn the_registry_is_sound() {
        assert_eq!(oops_docs::check(super::DOCS), Vec::<String>::new());
    }
}
