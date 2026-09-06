# CLAUDE.md

How Prosperous is built and the constraints to honour when changing it.

**Read [the OOPS conventions](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md) first.** Provenance, naming, decision logs, worklogs and
gates are shared across [Orbistoun](https://github.com/project-oops/Orbistoun), [obSCEne](https://github.com/project-oops/obSCEne), [Prosperous](https://github.com/project-oops/Prosperous) and
[SELFish](https://github.com/project-oops/SELFish), and are stated once there. This file holds only what Prosperous adds.

## Mission, in one breath

Remote management for a jailbroken Prospero-generation target, and the transport library
underneath it. Register a target, ask what it can currently do, put a payload on it, read its
log, run a command, move files. The binary is `pros`; the window is `pros-gui`.

## Why it is a library first

Two other projects need to talk to a target and neither is a target tool: an emulator that can
only settle some questions by asking real hardware, and a conformance probe whose whole
delivery problem is getting itself onto the machine. Both had started building the same
transport. So the transport is one crate, shared, and the tools on top of it hold no logic of
their own. `pros-link` is taken by obSCEne and orbistoun; `pros-core` by orbistoun. That is
what makes a change to either a change the consumers feel, and it is the reason for principle 4.

## Principles

### 1. It invents nothing

Prosperous does not put software on a console. It speaks to services somebody else's exploit
chain already started - `elfldr`, `pldmgr`, `klogsrv`, `shsrv`, `ftpsrv` - and invents no
protocol of its own. Where a command is one line typed at the target's shell, this builds that
line and reads the reply; it does not model a capability the target does not expose.

### 2. Measured over reasoned, and the distinction is the whole project

A path, a port, a constant is a **measured** thing - observed on a target - not a parameter
reasoned about and given a default. The autoload path is the example the project turns on: it
is a fixed constant because it was read off a machine, and writing it as a configurable would
invite a plausible wrong value that looks right until a title placed there stays inert. When
you add one, say where it was measured, the same way SELFish records where a format field came
from.

### 3. The shims hold no logic

`pros-cli` and `pros-gui` do nearly the same things **on purpose**: a capability in only one of
them is a capability that gets forgotten. Neither holds behaviour the other lacks - the logic
lives in `pros-core` and `pros-link`, and the one thing the window reaches that the command
line does not is `pros_core::install`, which is called out precisely because it is the
exception. If a shim starts holding logic, the other is already drifting.

### 4. `pros-link` stays minimal, because obSCEne holds it

`pros-link` carries one dependency - `tracing`, the logging facade - and adds each one the way
obSCEne adds its own: with an argument in the manifest, individually justified. What it must
not grow is a runtime, a TLS stack or a serialisation framework, because obSCEne takes this
crate and holds a small, deliberate dependency list of its own. Anything that genuinely needs a
library lives one layer up, in `pros-core`. (D025)

### 5. Honest failure, and a finding carries its remedy

Shared as [OOPS conventions §3](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md#3-honest-failure-over-plausible-output); what it binds here:

- **A health check answers *now*; the boot list answers *after a reboot*.** They are different
  questions with similar-looking answers - a service running today can be absent after the next
  power cycle because it was never in the list. A tool that reports "klogsrv is not loaded"
  without adding "and it is not in the list either" has left out the useful half.
- **A finding names what would put it right.** `doctor` exists because a diagnosis with no
  remedy is half a diagnosis.
- **`fetched 0 times` means unreachable, not empty.** The install serves a package and has the
  target fetch it; zero fetches means the target never came, so nothing about the package is
  implicated - the failure is the network, and it must say so rather than imply the package was
  judged and failed.

### 6. A format is shared; a measurement stays with whoever measured it

Prosperous reads the platform's formats from SELFish and invents none of its own. What it adds
is measurement about a *running* target - which service answers, what the boot list holds, what
`ps` shows - and that stays here. Process control (finding a process by name, the signal a
stopped title needs before it is killed) is one such measurement, and it lives in
`pros-core::system` for exactly this reason rather than in the probe that first needed it. (D027)

## Building, and the one networking gotcha

It is a plain Cargo workspace and **builds anywhere** - no target, no WSL, no cross-compile.
Tests need no target either: `pros-link` ships a fake one, deliberately not behind
`#[cfg(test)]`, because every consumer has the same problem and three private copies of it is
what this crate exists to prevent. `./bin/prosperous check` is what CI runs.

**`hw install` is the exception, and it is a networking constraint, not a code one.** Serving a
package means the target connects *in*, to us. `pros_core::handover` binds the interface that
routes to the target - correct - but under WSL2's default NAT that interface is an address the
target cannot reach, and the install then fails in the most misleading way available: the shell
prints its usual line, the target never sends a request, and the handover reports
`fetched 0 time(s)`. Run the install from a shell that binds a LAN address the target can
reach. Everything that connects *out* (check, logs, sh, send, report) works from anywhere,
because outbound NAT forwards fine.

## Working sessions

- Every non-obvious choice gets a numbered entry in `docs/DECISIONS.md` **as it is made**, with
  the reasoning - that is what stops it being re-litigated.
- Append to `docs/WORKLOG.md` at the end of a completed unit of work. Record surprises
  especially.
- Anything consulted goes in `ACKNOWLEDGEMENTS.md` in the same change.
- Run `./bin/prosperous check` before logging anything as done.

## Where things live

- `crates/pros-link` - the transport: the target services over `std::net` and nothing else.
  Taken by obSCEne and orbistoun, which is why principle 4 guards its dependencies.
- `crates/pros-core` - registry, the payload manifest and checksum verification, the check and
  autoload workflows, `system` (what a running target is, and process control), `install`,
  `handover`, `transfer`. The logic layer. Pure builders and parsers where it can be, with the
  effect (running a shell line over `pros-link`) left to the shim - `launch` and `system` are
  the pattern to copy.
- `pros-cli` - the `pros` command. `pros-gui` - the window. Neither holds logic (principle 3).
- `docs/DESIGN.md` - the transport and the services in full. `docs/CAPABILITIES.md` - the
  three-layer model of what is durable versus configurable. `docs/USAGE.md` - the verbs, for
  someone using it rather than changing it. `docs/VIDEO.md` - Porthole, the planned own-capture
  path, part three the go/no-go.
- `docs/DECISIONS.md`, `docs/WORKLOG.md` - durable memory.
