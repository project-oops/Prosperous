# CLAUDE.md

Read [AGENTS.md](../AGENTS.md), [CONVENTIONS](../docs/CONVENTIONS.md) and
[STYLE](../docs/STYLE.md) first. This file only adds what Prosperous needs.

## Scope

Remote management for a Prospero-generation target, and the transport library underneath it:
register a target, check what it can do, put a payload on it, read its log, run a command,
move files. The binary is `pros`; the window is `pros-gui`.

Prosperous is a library first. obSCEne uses `pros-link` and `pros-core`, so a change to either
is a change it feels; grep its `tool/` before changing a public signature. The tools on top hold no
logic of their own.

## Principles

- **Invents nothing.** Prosperous speaks to services already running on the target (`elfldr`,
  `pldmgr`, `klogsrv`, `shsrv`, `ftpsrv`) and defines no protocol of its own. A command that
  is one shell line on the target is built as that line and its reply parsed; no capability
  the target does not expose is modelled.
- **Measured over reasoned.** A path, port or constant is observed on a target, not a
  parameter given a default. The autoload path is a fixed constant, not a setting, because a
  plausible wrong value leaves a title inert. A new measured value says where it was measured.
- **Shims hold no logic.** `pros-cli` and `pros-gui` expose the same capabilities; the logic
  lives in `pros-core` and `pros-link`. The one exception is `pros_core::install`, reached
  only from the window.
- **`pros-link` stays minimal.** Its one dependency is `tracing`. Each new dependency is
  justified individually in the manifest; no runtime, TLS stack or serialisation framework,
  because obSCEne holds a deliberate dependency list. Anything that needs a library goes in
  `pros-core`. (D025)
- **A finding carries its remedy.** A diagnosis names what would put it right; `doctor`
  exists for this.
- **Now and after reboot are different questions.** A health check answers now; the boot list
  answers after a reboot. A report that a service is not loaded also says whether it is in
  the boot list.
- **`fetched 0 times` means unreachable, not empty.** The target never connected, so the
  failure is the network and the message says so, not that the package failed.
- **A format is shared; a measurement stays with its measurer.** Platform formats come from
  SELFish; Prosperous defines none. Measurements of a running target (which service answers,
  the boot list, `ps`, process control) live here, in `pros-core::system`. (D027)
- **No offensive-security vocabulary.** "Jailbroken" and similar words are not used in code
  or docs; use the clean-room terms in [AGENTS.md](../AGENTS.md#4-models).

## Building and networking

- A plain Cargo workspace that builds anywhere: no target, no WSL, no cross-compile.
- Tests need no target. `pros-link` ships a fake target outside `#[cfg(test)]` so every
  consumer uses the same one.
- The gate is `./bin/prosperous check`. Run it before reporting work as done.
- Installing a package needs a LAN address the target can reach. The target connects in to
  `pros_core::handover`, which binds the interface that routes to the target; under WSL2's
  default NAT that address is unreachable, the target never fetches, and the handover reports
  `fetched 0 time(s)`. Outbound verbs (check, logs, sh, send, pull) work from anywhere.

## Where things live

| Path | Holds |
|---|---|
| `crates/pros-link` | the transport: target services over `std::net` and nothing else |
| `crates/pros-core` | registry, payload manifest and checksums, check and autoload workflows, `system`, `install`, `handover`, `transfer`; pure builders and parsers, with the effect left to the shim (`launch` and `system` are the pattern) |
| `crates/pros-moonlight` | the Moonlight/GameStream bridge in front of Porthole, reached by `pros moonlight` and `pros fake-target`; a separate `pros-link` consumer because it carries TLS, RTSP, RTP and pairing |
| `pros-cli` | the `pros` command |
| `pros-gui` | the window |
| `docs/DESIGN.md` | the transport, the services, and the three-layer capability model |
| `docs/guide/` | the user guide, one page per area; the window embeds it |
| `README.md` | building, and the `bin/prosperous` verbs |
| `docs/VIDEO.md` | diffing, Porthole and the Moonlight bridge |
| `docs/GLOSSARY.md` | the words Prosperous uses |
| `docs/decisions/` | the decisions in force |
