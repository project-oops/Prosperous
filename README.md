<p align="center">
  <img src="assets/logo.png" alt="Prosperous" width="200">
</p>

# Prosperous

Remote management for a Prospero-generation target, and the transport library underneath it.
Prosperous registers a target, asks what it can currently do, moves files, sends and supervises
payloads, launches and closes titles, runs shell commands and reads the system log. The target
is a machine running the homebrew services, or orbistoun.

It speaks to services already running on the target (`elfldr`, `ftpsrv`, `klogsrv`, `shsrv`,
`pldmgr`) and defines no protocol of its own. Payload binaries are never shipped, only
described in a manifest and verified by digest before they are kept or sent.

Site: [project-oops.github.io/Prosperous](https://project-oops.github.io/Prosperous/)

## Crates

Prosperous is a library first; the two programs hold no logic of their own.

| Crate | Purpose |
|---|---|
| `pros-link` | the transport: each target service over `std::net`, with `tracing` as its only dependency. Used by obSCEne |
| `pros-core` | target registry, payload manifest and checksums, check, transfers, process control, package install |
| `pros-moonlight` | a Moonlight host bridge in front of Porthole ([VIDEO.md](docs/VIDEO.md)) |
| `pros-cli` | `pros`, the command line |
| `pros-gui` | `pros-gui`, the window |

The architecture is in [DESIGN.md](docs/DESIGN.md) and the vocabulary in
[GLOSSARY.md](docs/GLOSSARY.md).

## Building

A Rust toolchain is the only requirement: no C compiler, vendor SDK, firmware or signing keys.

The workspace takes `oops-build`, `oops-log`, `oops-paths` and `oops-docs` from oops-libs, and
`selfish-title` and `selfish-abi` from SELFish, by relative path. Both must be checked out as
siblings of this repository, which the [OOPS](https://github.com/project-oops/OOPS) collection
does:

```bash
./bin/oops bootstrap prosperous    # from the collection root: fetches the siblings
```

`bin/prosperous` is the one dev command, and CI runs the same one:

| Verb | Does |
|---|---|
| `check` | the full gate (the default) |
| `build` | release build of the workspace; extra arguments pass to cargo |
| `test` | `cargo test --workspace` |
| `lint` | clippy at `-D warnings` |
| `fmt` | format in place |
| `doc` | build the API docs |
| `clean` | remove build output |
| `provenance` | fail if any payload binary or executable is tracked |
| `target` | the read-only tests against a real target |

`check` runs, in order:

1. `provenance`, which also fails outside a git repository, where it cannot look
2. `cargo fmt --all -- --check`
3. `cargo clippy --all-targets -- -D warnings`
4. `cargo test`
5. `cargo doc --no-deps --workspace` with `RUSTDOCFLAGS="-D warnings"`

It needs no target and no network: `pros-link` ships a fake target that the tests stand on the
real service ports. The tests that need hardware are `#[ignore]` by default and run through
`target`:

```bash
PROS_TARGET=192.168.1.211 ./bin/prosperous target
```

CI (`.github/workflows/check.yml`) checks out the collection, bootstraps the siblings and runs
`oops check prosperous`, a wrapper over `./bin/prosperous check`.

## Using it

The releases page has builds for Windows, Linux and macOS, each holding `pros` and `pros-gui`.
From a build, they are in `target/release/`.

```bash
pros register 192.168.1.211 --name living-room
pros check                                   # what the target can do right now
pros logs --seconds 30                       # listen to the system log
pros restore ./build/title/GLCB00001 /data/homebrew/GLCB00001
pros launch GLCB00001
pros probe GLCB00001 ./build/title/GLCB00001 # deploy, launch and follow the log in one step
pros-gui                                     # the window
```

[Getting started](docs/guide/getting-started.md) begins the user guide, which the window also
shows under **help > documentation...**.

## Licence

MIT or Apache-2.0, at your option. Third-party notices are in
[THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md) and sources consulted in
[ACKNOWLEDGEMENTS.md](ACKNOWLEDGEMENTS.md).
