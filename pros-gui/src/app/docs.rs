//! The pages the documentation reader ships.

/// The pages this build ships, and their order in the reader.
///
/// `include_str!` puts them in the binary, so they match the build. Only the user manual is
/// listed; development records stay in the repository.
pub(super) const DOCS: &[oops_docs::Doc] = &[
    oops_docs::Doc::new(
        "user-guide",
        "User Guide",
        "Paths, portable mode, network daemons, and local-first storage",
        include_str!("../../../docs/features/user-guide.md"),
    ),
    oops_docs::Doc::new(
        "targets",
        "Targets",
        "Registering a console, and asking what it can currently do",
        include_str!("../../../docs/features/targets.md"),
    ),
    oops_docs::Doc::new(
        "logs",
        "Kernel Logs",
        "Streaming live system and title telemetry unbuffered",
        include_str!("../../../docs/features/logs.md"),
    ),
    oops_docs::Doc::new(
        "files",
        "Remote Storage",
        "Browsing files, transferring saves, and staging titles",
        include_str!("../../../docs/features/files.md"),
    ),
    oops_docs::Doc::new(
        "titles",
        "Titles & Execution",
        "Supervising running processes and launching BIG_APPs",
        include_str!("../../../docs/features/titles.md"),
    ),
    oops_docs::Doc::new(
        "shell",
        "Command Shell",
        "Executing remote commands directly on the target",
        include_str!("../../../docs/features/shell.md"),
    ),
    oops_docs::Doc::new(
        "payloads",
        "Payloads",
        "Why none are bundled, and what is checked before one runs",
        include_str!("../../../docs/features/payloads.md"),
    ),
    oops_docs::Doc::new(
        "library",
        "The target's storage",
        "Titles, saves and packages; the log; moving files",
        include_str!("../../../docs/features/library.md"),
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
