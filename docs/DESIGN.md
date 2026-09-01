# Prosperous

**A target-management tool, and the library underneath it.**

One instrument for talking to a prepared target target: register it, find out what it
can currently do, put a payload on it, read its log, move files, watch its output. Two
things consume that library - an emulator that needs target to check itself against,
and a standalone product for people who want to drive the target directly.

**Status lives in [ROADMAP.md](ROADMAP.md), not here**, and `Cargo.toml` is the list of what
exists. A design document that also claims what is built goes stale in one direction only,
and this one did: it described `pros-gui` and payload fetching as unbuilt after both had
shipped, and named a `pros-video` crate that was never created.

What this document is for is the *shape* - what each part is responsible for and why the
boundaries fall where they do. That does not change when a crate lands.

## The name

`orbistoun` embeds *orbis*, the previous generation's OS. `obSCEne` embeds *sce*, the
vendor's own prefix. `PROSPERous` embeds *prospero*, the current generation's codename -
and it is the only common English word that contains the whole of it.

That is the point rather than a coincidence. [OOPS conventions §2](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md#2-naming-no-vendor-brands-in-prose-or-in-our-own-api) asks for
a **low profile**: not concealment, since what any of this targets is obvious from the
first paragraph, but no reason to repeat brand names either. A real English word is the
cheapest way to hold that line, and it reads as a product rather than as a leak.

The binary is `pros`. The repository carries the long name.

## Two consumers, one library

| consumer | uses it for |
|---|---|
| **orbistoun** | probing and testing against real target while diagnosing: remote launch, pull files, read the kernel log |
| **Prosperous** | the standalone product: remote streaming, control, file transfer, file browsing |

This is new functionality. It is unrelated to orbistoun's existing remote-control
component and does not replace it.

The two consumers pull in opposite directions - one wants a library it can call from a
diagnostic loop, the other wants an application. Both get what they want only if the
library is the whole implementation and the applications are shims. That is
[Orbistoun's shim rule](https://github.com/project-oops/Orbistoun), taken here as a starting
condition rather than arrived at later.

## Where this sits in the hardware loop

The projects form one cycle, and each has exactly one job in it.

| | asks | answers | carries |
|---|---|---|---|
| **orbistoun** | ranks its unsettled assumptions and submits them | | |
| **obSCEne** | | runs on the metal and reports what actually happened | |
| **Prosperous** | | | gets the probe onto the target and keeps it there |

orbistoun writes every behaviour down with how it is known - published, measured,
guest-observed, assumed - and `orbistoun-cli questions --json` ranks the unsettled ones by
how often real guests call them. obSCEne carries those questions to hardware over its command
protocol, and an answer turns `assumed` into `measured`. See `THE_LOOP.md` there and
`docs/HARDWARE-PROBE.md` in obSCEne.

**Prosperous is the third side, and it is deliberately the dullest.** A probe that answers
arbitrary questions faults constantly - that is the normal case, not a fault - and the
protocol says restarting is out of scope, naming the restarter as *"a person on the hardware"*.
`pros supervise` is that person: it watches the serving port and re-sends the same bytes when
it stops answering. `pros logs` reads the report as it comes out over the kernel log, and
`pros send` put it there to begin with.

### What Prosperous does not do, and why it is written down

**It does not drive the protocol.** obSCEne has a driver, orbistoun has the questions, and a
second client here would be a third place that could disagree about what `died` means.

One was written and removed. The argument for it was that a published specification invites
more than one implementation - which explains why it is *permitted* and not why it is
*wanted*, and nothing here wanted it. Recorded because the same argument will read as
convincing the next time.

## Repository and crate layout

Three sibling repositories under one root, joined by **relative path dependencies**. No
feature flags, no duplicated code.

```
prosperous/
  crates/pros-link      the five target services - transport only
  crates/pros-core      device registry, dependency probing, workflows, the manifest
  (a video crate is discussed below and has not been created)
  pros-cli              shim
  pros-gui              shim - the standalone product

orbistoun/   ->  pros-link, pros-core
obscene/     ->  pros-link            (replaces tool/src/target.rs)
```

### Why the split falls where it does

`pros-link` stays **small and argued**, which is a hard requirement rather than a preference.
obSCEne's tool adds each of its dependencies with a paragraph justifying it, with
`forbid(unsafe_code)` on top. A transport crate that dragged in a runtime, a TLS stack or a
serialisation framework could not be taken by that project without breaking a policy it holds
deliberately.

It was **std-only** until D025, on the same reasoning taken one step further: zero is easier to
defend than one. What that cost was a transport with no way to say what it was doing, which is
the layer where that is worth most - so it now carries `tracing` and nothing else. Four packages
in obSCEne's tree, no proc-macro, and a consumer that wants none of it can compile every call
out with `tracing/max_level_off`.

So the line is drawn at **what needs a dependency**:

- **`pros-link`** speaks the five protocols over `std::net`. No hashing, no JSON, no
  async. This is the part obSCEne takes.
- **`pros-core`** verifies checksums, reads and writes the manifest, holds the registry
  and sequences workflows. It needs a hash and a JSON reader, and obSCEne never sees them.
**There is no third crate.** This named a `pros-video` that would integrate a remote-play
client and, later, the frame-grab client. The remote-play half is gone - see
`DECISIONS.md` - and the frame-grab half turned out to belong in `pros-link`, because it is a
socket protocol and that is what `pros-link` is for. The counting-and-piping half of Porthole
is in `pros-core::watch`, for the same reason: it sequences a workflow.

That leaves a dependency spine in orbistoun's sense: `link` -> `core`, each layer adding what
the one below deliberately does without - and video distributed across the two by what each
piece actually is, rather than gathered into a crate named after a subject.

### The cost, accepted rather than discovered

A path dependency means **`obscene-tool` no longer builds from a standalone clone of
`obscene`**. The projects are checked out as a set, under
[OOPS](https://github.com/project-oops/OOPS).

Written down because it will be noticed by somebody who did not choose it. The
alternative - publishing `pros-link` so obSCEne can depend on a version - trades a
checkout convention for a release process, and a release process is a worse thing to owe
than a clone somebody forgot.

## The target target

Measured on 2026-08-25 against a target. Payloads are loaded by `pldmgr` from
`/data/pldmgr/autoload.txt`, in this order:

```
kstuff-lite -> nanodns -> elfldr -> klogsrv -> shsrv -> ShadowMountPlus -> ps5upload -> ftpsrv
```

| service | port | what it is |
|---|---|---|
| `elfldr` | 9021 | send it an ELF, it runs it |
| `ftpsrv` | 2121 | anonymous FTP; 13-23 MB/s measured |
| `klogsrv` | 3232 | streams `/dev/klog` |
| `shsrv` | 2323 | a shell. **Raw TCP, not telnet** |
| `pldmgr` | 8084 | web dashboard, and the thing that loaded the rest |

### Three facts that cost real time to learn

**1. `elfldr` is a single point of failure, and it is the one that fails invisibly.**
`pldmgr` launches everything *through* `elfldr` - including, if asked, `elfldr` itself. So
when `elfldr` dies, `pldmgr` cannot bring anything back, and the dashboard that would tell
you keeps answering because it is a separate listener. Only re-running the jailbreak
recovers it.

Design consequence: **`elfldr` is checked first and reported first**, and a check that
finds it down says *re-run the jailbreak* rather than *reload a payload*. Those are
different amounts of work, and the tool knows which one applies.

**2. A vendor-format module and a plain ELF share their first four bytes.** Both begin
`7f 45 4c 46`. The loader's sanity check passes either, then maps a module whose entry
point expects tens of thousands of resolved imports, and dies without saying anything.

Design consequence: **guard on `e_type` at offset `0x10` before sending**, inside
`pros-link`, once. `0x0003` is a payload; `0xFE10` and `0xFE18` are vendor module types
and are refused by name. The refusal says which shape was found and which loader wants it,
because *that file is for the emulator, not the target* is the message a person needs.

**3. The loader duplicates the connection socket onto the payload's stdout and stderr.** A
payload sent that way reports back over the socket it arrived on.

Design consequence: **convenience, never mechanism.** A payload installed as a package or
started from the home screen has no such socket, and anything built on the assumption
breaks the moment a payload is launched another way. `pros-link` offers it as an optional
read-back on the send call, and nothing above may require it.

## What a registration is

An address and a name. Nothing else.

Capabilities do not survive a power cycle - a jailbreak does not, and the chain that comes
back depends on a text file edited weeks ago. **Anything cached about what a target can do
is a claim that expires without notice**, which is the same mistake as a stale exclusion
list. So capability is probed on every use and never stored.

The registry file is line-oriented and parsed by hand: it is a small table, and splitting
is simpler to test than a format crate is to justify.

It does **not** live under `%APPDATA%` on Windows. A tool running inside a packaged
container has its writes there redirected into a per-package cache, invisible to the same
user running the same tool from an ordinary shell. A configuration file the user cannot
find is worse than no configuration file.

## What a check reports

Not up or down. **What each service unlocks**, and whether its absence blocks anything:

- `elfldr` and a report channel are **required** - without them there is no workflow.
- `klogsrv`, `shsrv` and `pldmgr` are **optional** - they change how much is *visible*
  when something goes wrong, which is a different kind of important.

Required and optional fail differently and are reported differently.

**Timing is part of the answer.** A port that refuses instantly and one that takes 1500 ms
to refuse mean different things: the first is a machine saying no, the second is usually a
network deciding. The probe carries its own duration and the reporting layer decides what
is worth remarking on.

## Payloads: fetched, never vendored

Prosperous ships **no payload binaries**. It ships a manifest of where to get them,
editable outside the source tree so a moved mirror needs no recompile - the same rule as
Orbistoun's rules-in-data principle (its own principles file), rules in data rather than code.

Three reasons, in order of weight:

1. The `ps5-payload-dev` payloads are GPL-3.0. Redistributing binaries obliges you to
   offer corresponding source; pointing at upstream obliges nothing.
2. URLs rot, and a rotted URL should be a text edit rather than a release.
3. obSCEne's CI already refuses any tracked `.elf` or `.bin`, and Prosperous inherits the
   habit rather than arguing with it.

### The schema is copied, not invented

`pldmgr`'s own `repository_cache.json` already carries 25 entries with exactly the right
fields:

```
name  filename  url  source  source_direct  version
last_update  checksum  category  description  extract_file  asset_pattern
```

Copying it costs nothing and buys interoperability: **Prosperous can read `pldmgr`'s
repository as a source**, so a target that is already configured is already described.

`checksum` is the field that matters. You are downloading from a mirror and then executing
the result with kernel-adjacent privileges. **Verification happens before sending, always,
and the ordinary path offers no way to skip it.**

### `check` repairs, it does not only report

`pros check` should be able to fix what it finds missing - fetch an absent payload from the
manifest, verify it, send it - rather than printing a list for somebody else to act on. A
tool that can see a problem and not fix it has left the interesting half undone.

## Video: two problems, not one

### Watching

**Porthole.** The target encodes its own output in hardware and serves it on a socket this
project defined; this reads it, counts what goes past, and pipes it to a player. See
`VIDEO.md` part three.

Not the vendor's protocol, and not somebody else's client driving it. That route existed here
and was removed: speaking remote play means pairing, a UDP transport, ECDH with per-session
AES-GCM, Reed-Solomon FEC, two video codecs and Opus, and every one of those costs is paid to
talk to **unmodified** firmware. This project only ever talks to jailbroken targets, which
already run our code.

### Diffing

**A stream is useless for this, and the reason is the codec.** orbistoun's own oracle
list calls framebuffer diffing *the only cheap, mechanical correctness signal in the whole
codebase*, and obSCEne's GPU comparison already resolves differences of one ULP. A lossy
codec does not degrade that signal, it destroys it.

The answer is an **on-demand lossless frame grab**: one frame, exactly, when asked. Roughly
8 MB, under a second, a small payload using the `sceVideoOut` calls obSCEne already
declares.

**Designed now, built later** - when orbistoun's GPU work reaches for it. When it is built
the payload belongs in **obSCEne**, because homebrew that runs on the target and reports
what it saw is obSCEne's exact description. `pros-link::frames` is the client half, and
it is built.

The protocol, the acceptance criteria and the open questions target has to answer are in
[VIDEO.md](VIDEO.md). (D008)

### An open question obSCEne can answer

**Is a target encoder (`libSceVideoEnc`, the VCE block) reachable from an unsigned
payload?**

That single answer decides whether live watching exists at all - there is no second route
to fall back on, by choice. And
it is precisely obSCEne's kind of question: call it, record what came back, grade it by
what it ran on.

## Scope

### v1

- device registration and onboarding
- dependency checking, with repair
- shell over `shsrv`
- kernel log streaming
- payload deployment from the manifest
- file browse and transfer
- remote play via Chiaki

**Multi-target throughout.** Every operation names its target and the registry resolves
it. There is no ambient "current target" for an operation to inherit by accident.

### Deferred

- **The frame grabber**, as above.
- **Scripted controller input.** Ghostpad does it with a payload and 16-byte packets over
  TCP 6967, and covers two cases remote play cannot: injecting into a live local session,
  and deterministic timing for automated testing. It also patches `libScePad.sprx` in
  `SceShellCore`'s live memory, which is more invasive than anything else in the chain.
  Revisit when automated target testing needs it. Credit it if used.

### Out

*This section said cheats, avatars, saves and the game library were out of scope. All four
shipped - `pros saves`, `pros titles`, `pros library`, and a cheats table in
`pros-core/data`. The argument below is kept because it was a real one and the reversal is
worth seeing, not because it still describes the tool.*

The reasoning was that these are metadata products while this is target plumbing, with
almost nothing in common but a network address. What changed it: once a target is reachable
and its filesystem readable, the metadata is *already there* - the plumbing had made them
cheap rather than making them relevant.

### Explicitly not a concern

**Authentication and transport security.** Every target service here is unauthenticated
on the LAN by design - that is what a jailbreak payload chain is. Adding a login to a tool
that talks to an open FTP server and a raw shell would be theatre.

Recorded so that it is not re-argued in six months by somebody who has just noticed.

## Testing without a target

The hardest constraint on this project is that its subject is a physical object on a
network that is usually switched off.

`pros-link` being small and speaking five plain protocols makes the answer
straightforward: **a loopback fake**. A test server that accepts on the five ports and
replies the way each service does - including the awkward parts, which are the ones worth
testing:

- `klogsrv` streams and never ends, so the client stops on a window rather than an EOF.
- `shsrv` has no framing at all, so the client reads until quiet.
- the loader may or may not echo, so the client has to work when it does not.
- a port that refuses instantly and one that refuses slowly must produce different reports.

None of that needs target, and all of it is where the bugs are. What a fake cannot test
is whether the target agrees - that is what a registered target and a manual run are for,
and the difference between the two should stay visible in how results are reported.

## Risks, honestly

- **The payload chain is somebody else's.** Ports, names and behaviour can change under us.
  The manifest absorbs a URL change; a protocol change is a real break, and a check that
  reports *what each service unlocks* is what makes that legible rather than mystifying.
- **A path dependency across repositories is a coordination cost**, paid every time
  somebody clones one of them alone.
- **The standalone product and the diagnostic library want different things.** Every time
  the GUI wants something the library will not give it, the temptation is to put logic in
  the shim. That is the failure mode the shim rule exists to name.
- **Executing downloaded binaries with kernel-adjacent privileges** is the point of the
  tool and also its sharpest edge. The checksum is the only thing standing between a
  mirror and the target, which is why it is not optional.
