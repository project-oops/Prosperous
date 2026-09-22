# Changelog

Prosperous ships as a **rolling build** plus tagged drafts - `main` refreshes one
`latest-main` prerelease, and a `v*` tag opens a draft versioned release. There is no
semantic version yet, so for anything off `main` the **short commit SHA is the version**.

Each entry is headed by the SHA (+ date) that shipped it, newest first. Within an entry,
changes are grouped **Added / Changed / Fixed**.

Nothing has shipped yet. This is the initial commit, so no entry below carries a SHA and
the CI that would produce one has never run.

## [unreleased] - as of 2026-09-22

### Added

- **Process control, in both programs.** `pros ps` lists what is running (pid, state, memory,
  title, command), `pros kill` ends one process by pid, `pros close` ends a title, and `pros top`
  is the live view - the same list redrawn on an interval until Ctrl-C or `--seconds`. The window's
  system panel does all of these, shows each process's memory (peak on hover), and has a sort
  chooser and an opt-in auto-refresh. Memory is the figure the target's own `ps` prints; there is
  no CPU column, because it reports none. (D027, D035)
- **`pros probe`** - deploy a homebrew build, launch it, and follow its log until it parks, exits
  or a `--seconds` cap elapses, in one command: the hand-run `restore` then `launch` then `logs`
  in a second window, made one verb, following before it launches so nothing is missed. (D033)
- **Save the log to a file you choose.** The log screen's *save* button writes what is shown
  (filter and all) to a `.log` where you pick - beside the per-target file every line is already
  appended to as it arrives.
- **A regex filter on the log.** The filter box has a *regex* checkbox; ticked, it matches lines as
  a regular expression instead of plain (case-insensitive) text. A pattern that does not compile
  shows every line and says so rather than blanking the log. (D036, `regex-lite`)

- **The Moonlight bridge.** A third library crate, `pros-moonlight`, re-presents Porthole's two
  ports (9805/9806) as the NVIDIA GameStream protocol, so any Moonlight client can discover, pair
  with and stream a registered target. Reached by two verbs: `pros moonlight` runs the bridge, and
  `pros fake-target` stands in for the console payload so the whole client leg can be tested with
  no hardware. Discovery, PIN pairing, RTSP, RTP video and the ENet control/input channel are
  built and unit-tested; it decodes no video (reading is not decoding) and copies no GPL code. See
  [`docs/VIDEO.md`](docs/VIDEO.md) part four and `ACKNOWLEDGEMENTS.md` (Moonshine, BSD-2-Clause).
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

- **The log view is virtualized and holds far more.** It was capped at 2000 lines because the old
  view laid out every line every frame; it now lays out only the rows on screen, so the on-screen
  buffer is 20,000 lines (scrollback only - the full history is still in the kept file). The filter
  no longer allocates per line. (D036)
- **`restore` no longer re-sends a file it already put there unchanged.** It records what it
  verified landing on each target and skips a file whose local bytes have not changed and which the
  target still reports present, so re-deploying a large title becomes mostly cheap checks rather
  than the whole tree sent again. `restore --all` (and `probe --all`) force every file across. Why
  the record is kept here rather than asked of the target - the console unwraps a signed container
  on read - is D034.
- The `build` verb passes extra arguments through to cargo, so CI can select a target without
  a second code path.

### Fixed

- **`restore` no longer rewrites a title identifier.** Routing an id without a Sony prefix to the
  homebrew folder had also rewritten `MESA00001` to a Sony-looking `PPSA00001` - overwriting a
  different installed title. It now routes by location and never changes the id. (D030)
- **`restore` reports the truth about what landed.** A store the server acknowledged was counted as
  written even when a mounted title left the old file in place. It now reads the size back, and for
  a fake-signed SELF the console unwraps on access it checks the file is *present* rather than
  comparing a size the unwrap has changed - so a correct SELF deploy is no longer called incomplete
  and a store that vanished is still caught. (D031, D032)

- **`close` finds a homebrew title.** The `ps` reader recognised a title column only by the two
  retail prefixes, `PPSA` and `CUSA`, so a homebrew title such as `GLCB00001` sat in `RUN` with
  its `eboot.bin` held open while `close` reported no running process. The column is now
  recognised by its shape - four capital letters, then five digits - with the system's own
  `NPXS` identifiers still excluded, so `SceShellUI` remains a job for `restart-ui`.

- **A quiet stream no longer ends the watch on Windows.** The pump treated only `WouldBlock`
  as the pause between frames, which is the name Unix gives a timed-out read; Windows names
  it `TimedOut`, so the first half-second with nothing arriving was reported as the stream
  having ended, with a message blaming the target. Both names are now the pause, and a unit
  test and a standin test pin each.
