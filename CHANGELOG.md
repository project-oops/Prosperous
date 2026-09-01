# Changelog

Prosperous ships as a **rolling build** plus tagged drafts - `main` refreshes one
`latest-main` prerelease, and a `v*` tag opens a draft versioned release. There is no
semantic version yet, so for anything off `main` the **short commit SHA is the version**.

Each entry is headed by the SHA (+ date) that shipped it, newest first. Within an entry,
changes are grouped **Added / Changed / Fixed**.

Nothing has shipped yet. This is the initial commit, so no entry below carries a SHA and
the CI that would produce one has never run.

## [unreleased] - as of 2026-09-01

### Added

- **Two programs over one library.** `pros` (command line) and `pros-gui` (an eframe
  window) sit on `pros-core` and `pros-link`. Both do nearly the same things, deliberately:
  a capability that exists in only one of them is a capability that gets forgotten.
- **Invocation of services the console already runs.** `run`, `install` and `launch` against
  an on-console ELF loader. Prosperous invents no protocol of its own - it speaks what is
  already listening, and where it does not know a wire format it says so rather than guessing.
- **Portable mode.** An empty `.portable` directory beside the binaries moves all state there
  instead of a user profile, so a copy on a stick stays a copy on a stick.
- **The measured finding about routes**, recorded rather than papered over: there is no
  single path that is both our own code and native to the current generation. `run` executes
  our code through the previous generation's compatibility path; `launch` is native but starts
  vendor code. The bridge between them is understood and deliberately not built.
- **`docs/VIDEO.md`**, which scopes a first-party capture and input path and states its own
  go/no-go condition instead of assuming the answer.
- **Release workflow.** Windows, Linux and macOS archives, built through `./bin/prosperous
  build` so the workflow and the local command cannot drift. This existed because
  `docs/guide/getting-started.md` already told a reader to download from a releases page that
  nothing populated - the first instruction in the guide was a dead end.

### Changed

- The `build` verb passes extra arguments through to cargo, so CI can select a target without
  a second code path.
