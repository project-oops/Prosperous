# Design

Prosperous is a target-management tool and the library underneath it. It registers a target,
finds out what the target can do, puts payloads on it, reads its log, moves files, controls
its processes and watches its output. The binary is `pros`; the window is `pros-gui`.

The library is the whole implementation and both programs are shims over it. Two kinds of
consumer need that: the sibling projects, which call it from a diagnostic loop, and a person
driving a target directly.

## The target and its services

Prosperous speaks to services the entry point starts on the target. It adds no protocol of
its own for them. The compiled-in list is `pros_link::service::SERVICES`:

| Service | Port | Protocol | Required |
|---|---|---|---|
| `elfldr` | 9021 | send an ELF, it runs | yes |
| `ftpsrv` | 2121 | anonymous FTP | yes |
| `klogsrv` | 3232 | `/dev/klog` as an endless stream | no |
| `shsrv` | 2323 | a shell over raw TCP, not telnet | no |
| `pldmgr` | 8084 | the payload manager and its dashboard | no |

The payload manager starts the rest from an autoload list on the target, and it launches
everything through `elfldr`. When `elfldr` is gone nothing can be reloaded, including
`elfldr`, and the dashboard still answers because it is a separate listener. A check that
finds the loader missing therefore says to rerun the entry point, not to load a payload
(`Remedy::RerunTheEntryPoint`).

Three facts about the transport shape the code:

- A vendor-format module and a plain ELF share their first four bytes. `pros-link` reads
  `e_type` at offset `0x10` before sending: `0x0003` is a payload, `0xFE10` and `0xFE18` are
  vendor module types and are refused by name, with the loader that wants them.
- The loader duplicates the connection socket onto the payload's stdout and stderr. The send
  call offers that as an optional read-back. Nothing above it depends on it, because a payload
  started any other way has no such socket.
- Every service is unauthenticated on the local network. Prosperous adds no login of its own.

## Crates

```
crates/pros-link       the target services: transport only, over std::net
crates/pros-core       registry, checks, the manifest, workflows
crates/pros-moonlight  the Moonlight bridge in front of Porthole
pros-cli               the `pros` shim
pros-gui               the window shim
```

The line between them is drawn at what needs a dependency.

- **`pros-link`** speaks the service protocols, the frame grabber and Porthole's wire
  formats. Its only dependency is `tracing` (D025), which a consumer can compile out with
  `tracing/max_level_off`. No hashing, no JSON, no async. obSCEne's tool takes it by path and
  justifies each dependency it adds, so this crate stays small enough to pass that review.
- **`pros-core`** verifies checksums, reads and writes the manifest, holds the registry and
  sequences workflows: checks, deployment, transfers, saves, titles, supervision and
  `watch`, which pipes Porthole's video to a player.
- **`pros-moonlight`** holds the TLS stack, RTSP, RTP, forward error correction and pairing
  the GameStream protocol needs. None of that belongs in a transport crate, and it is not a
  general workflow, so it is a crate of its own that takes `pros-link` directly.

The repositories are checked out together under the OOPS root and joined by relative path
dependencies. A standalone clone of a consumer does not build; that is the cost of not
running a release process for `pros-link`.

## Registration

A registration holds only what a power cycle cannot change: a name, an address, port
overrides by service name, and the startup chain the target is meant to run. The registry is
one line per target, `<name> <address> [service=port ...] [chain=<name>]`, in the
collection's shared data directory resolved through `oops_paths`, so sibling projects reach
the same targets. There is no ambient current target: every operation names one.

What a target can do is never stored. Loaded services do not survive a power cycle, so a
cached capability is a claim that expires without notice. It is probed on every use.

## Checks

A check reports what each service unlocks, not up or down. A missing required service
blocks every workflow; a missing optional one dims the view, because less is visible when
something fails. The verdict is `Ready`, `Dimmed` or `Blocked` with a remedy.

The **doctor** turns findings into a repair plan: a payload the target lacks is fetched from
the manifest, verified, sent and listed in the chain. A plan is confirmed by a person before
it runs, and where more than one payload would answer, the choice is offered rather than
made.

Timing is part of the answer. A port that refuses at once is a machine saying no; one that
takes the full timeout is usually the network. A probe carries its duration and the
reporting layer decides what to remark on.

The **chain** is a separate question from the check: what the target loads when it comes
back, read from the payload manager's autoload list. A service that answers now and is
absent from the chain is gone after the next power cycle.

## Payloads

Prosperous ships no payload binaries. It ships a manifest of where to get them, editable
outside the source tree so a moved mirror is a text edit rather than a release.

- The upstream payloads are GPL-3.0. Pointing at them carries no obligation; redistributing
  them would.
- The manifest schema is the payload manager's own `repository_cache.json` format, so a
  target's repository reads as a source (D016).
- A checksum is verified before anything is sent, always. The file is about to run on the
  target, and the ordinary path has no way to skip the check.

## Capabilities

What Prosperous knows about a target falls into three layers, and only the middle one is
configuration.

1. **Protocols** are code. FTP is FTP whoever serves it; a socket that takes an ELF and runs
   it is a loader. Configuration never names a protocol this program has no code for.
2. **Providers** are data. Which payload provides a capability, on which port, at which
   paths and in which file format. This is the layer that changes when a payload is replaced
   by another.
3. **Facts about the machine** are code: `/user/home`, `/user/app`, `/user/appmeta`, the SFO
   layout, the save container shape. A configurable machine fact invites a confidently wrong
   answer with no second opinion to catch it.

The test for a layer: if a different entry point would change it, it is a provider; if only
a different machine would, it is a machine fact.

A **capability** names what Prosperous needs, such as moving files, and lists the providers
known to supply it. A capability is satisfied when any provider answers, and the report names
which one, because "files, via `zftpd`" and "files" are different facts. Its `speaks` field
names a protocol and is validated against what is compiled in when the file is read. A
provider's dependencies are stated against capabilities, not payload names, so a root failure
such as a missing loader is reported once and what follows from it is reported as following.

Two rules keep this honest:

- Capabilities refer to payloads by the manifest's names. The manifest is the only list of
  payloads.
- The built-in list is compiled in and runs when no file exists.

Port overrides on a registration are the provider layer in its simplest form. Every
connection goes through the registration, so an override reaches transfers as well as the
probe. Manifest entries may declare `unlocks` and `required`, so a declared payload takes part
in the verdict the same way a compiled-in service does.

## Video

Watching and diffing are separate problems with no shared code. Watching goes through
Porthole, a payload that encodes the target's output in hardware and serves it on a socket;
`pros-core::watch` reads it, counts it and pipes it to a player, and `pros-moonlight` offers
the same stream to any Moonlight client. Diffing goes through a resident frame grabber that
returns one lossless frame on request, read by `pros-link::frames` (D008). Both are specified
in [VIDEO.md](VIDEO.md).

## Testing without a target

The target is a physical machine on a network that is usually switched off. `pros-link::fake`
is a loopback stand-in for the services, shipped rather than hidden in a test module so every
consumer uses the same one. It fakes the awkward parts, because that is where the bugs are:

- `klogsrv` streams and never ends, so a reader stops on its own window.
- `shsrv` has no framing, so a reader stops on silence.
- The loader may or may not answer, so a reader is correct when it does not.
- A port that refuses at once and one that refuses slowly produce different reports.

`pros fake-target` does the same for Porthole: it serves an Annex B file on 9805 and prints
the pad records it receives on 9806, so the Moonlight bridge runs end to end on one machine.

A fake cannot say whether the target agrees. That takes a registered target and a manual
run, and results say which of the two produced them.
