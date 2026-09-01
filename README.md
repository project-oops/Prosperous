<p align="center">
  <img src="assets/logo.png" alt="Prosperous" width="200">
</p>

# Prosperous

**Remote management for anything that runs Orbis software**, and the library underneath it.

Site: **[project-oops.github.io/Prosperous](https://project-oops.github.io/Prosperous/)**

Register a target, find out what it can currently do, put a payload on it, read its log, run
a command, move files. The binary is `pros`.

```
pros register 192.168.1.206 --name living-room
pros check
pros send probe.elf
pros pull /data/report.txt
```

## Why it is a library first

Two other projects need to talk to a target and neither is a target tool: an emulator that
can only settle some questions by asking real target, and a conformance probe whose entire
delivery problem is getting itself onto the machine. Both had started building the same
transport.

So the transport is one crate, shared, and the tools on top of it hold no logic of their own.

| crate | what it is | who takes it |
|---|---|---|
| `pros-link` | the five target services, over `std::net` and **nothing else** | Prosperous, obSCEne, orbistoun |
| `pros-core` | registry, payload manifest, checksum verification, the check workflow | Prosperous, orbistoun |
| `pros-cli` | the `pros` command | - |
| `pros-gui` | the `pros-gui` window. Holds no logic; the one thing it reaches that `pros` does not is `pros_core::install` | - |

`pros-link` carries one dependency - `tracing`, the logging facade - and adds each one the way
obSCEne adds its own: with an argument in the manifest. What it must not carry is a runtime, a
TLS stack or a serialisation framework, because obSCEne takes this crate and holds a small,
individually-justified list. Everything that genuinely needs a library lives one layer up.
(D025)

The projects are checked out as siblings under [OOPS](https://github.com/project-oops/OOPS)
and joined by relative paths. That is a
deliberate trade - a checkout convention instead of a release process - and it means
`obscene-tool` no longer builds from a standalone clone of `obscene`.

## Using it

Reading a check, fetching and verifying payloads, and working with titles, saves and packages
is in **[docs/USAGE.md](docs/USAGE.md)**.

## Building

**The recommended way in is [OOPS](https://github.com/project-oops/OOPS)**, which holds all four
side by side and carries one entry point over them:

```bash
./bin/oops check prosperous   # also: build, test, fmt, clean
```

That relays to this repository's own entry point rather than reimplementing anything, so the
two cannot disagree - and it is what CI runs, for the same reason.
[docs/BUILDING.md](https://github.com/project-oops/OOPS/blob/main/docs/BUILDING.md) has every verb.

**From inside this repository the entry point is `bin/prosperous`**, carrying the same verbs:

```bash
./bin/prosperous check        # everything that has to pass. What CI runs.
```

`check` is one command: no tracked payload binaries, formatting, clippy at `-D warnings`,
tests, and a doc build. It is what CI runs, so a green pipeline and a working copy cannot
disagree.

**[docs/BUILDING.md](docs/BUILDING.md)** is the full account - every verb, what `check` runs
and in what order, why `target` is deliberately not part of it, and what CI runs.

**A clone of only this repository is no longer enough.** Prosperous takes `oops-build`,
`oops-log` and `oops-paths` from `oops-libs` by relative path, as a sibling, so the layout is
a build requirement. `oops bootstrap prosperous` fetches it.

Tests need no target. `pros-link` ships a fake one - not behind `#[cfg(test)]`, because every
consumer has the same problem and three private copies of it is what this crate exists to
prevent.

## Status

Built and green: `pros-link`, `pros-core`, `pros-cli`, and `pros-gui` - a native window on
the same `eframe` version and backend as the sibling project's shell, so that a shared GUI
crate later is a move rather than a port.

Half built, and it is worth being exact about which half. **Porthole** - the stand-in: our own
video and input over our own payload, instead of the vendor's remote-play protocol - exists on this side:
the stream section connects, counts what arrives, pipes it to a player, and drives four pads
from the keyboard. **Nothing serves it yet.** Pressing *watch* today produces *connection
refused*, naming the port, because the payload at the other end is gated on one question for
real hardware. See [docs/VIDEO.md](docs/VIDEO.md) part three.

Designed, not built: the lossless frame grab for diffing (VIDEO.md part two), and fetching a
payload from a public mirror, which is what would let `pros check` repair rather than only
report.

Proposed, not decided: a shared home for what has been measured about the controller, so that
the projects stop learning it separately - [docs/PAD.md](docs/PAD.md).

See [docs/DESIGN.md](docs/DESIGN.md) for the whole shape and
[docs/DECISIONS.md](docs/DECISIONS.md) for why each part is the way it is.

## Credit

Every target-side payload this talks to is somebody else's work, used unmodified and
credited. See [ACKNOWLEDGEMENTS.md](ACKNOWLEDGEMENTS.md).

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option -
the Rust ecosystem convention.

## Part of OOPS

Prosperous is one of four projects aimed at the same platform's operating system. They are developed
together in **[OOPS](https://github.com/project-oops/OOPS)** and released separately.

| | |
|---|---|
| **[Orbistoun](https://github.com/project-oops/Orbistoun)** | the emulator - attempts to reimplement what a title runs on |
| **[obSCEne](https://github.com/project-oops/obSCEne)** | the probe - a guest that interrogates whatever runs it and reports what it found |
| **[SELFish](https://github.com/project-oops/SELFish)** | the formats - read, write and build tools for the platform's own file formats |

**Developing any of them?** Clone [OOPS](https://github.com/project-oops/OOPS) - it holds all four side by side, arranged so
they build against each other. Cloning this repository alone gets you this project; it is
the right thing for using it and the wrong thing for changing it.

Shared rules - provenance, naming, decision logs, worklogs, gates - live in
[the OOPS conventions](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md) and are not restated here.
