# D001 - Prosperous exists, and it is a library with two consumers


**decided** - 2026-08-25 - before any code, which is the point

Two projects need to talk to a target and neither is a target tool.

orbistoun needs target because most of what it does not know can only be settled by
asking the platform - what a function returns, whether a symbol exists, what a frame
actually looked like. obSCEne needs it because it *is* a program that runs on the target
and reports what it saw, and getting it there is the whole delivery problem.

Both had begun building the same transport. obSCEne already has it: `tool/src/target.rs`,
around 300 lines of `std::net`, register / list / check / logs / send / sh. It works, and
its design decisions are right. The wrong next step is a second copy of it inside
orbistoun.

So: one library, in its own repository, with the tooling that sits on top of it as a
product in its own right.

### What is being decided

- **`pros-link` is std-only.** *(Superseded by D025: it now carries `tracing` and nothing
  else. The reasoning below stands for what was known then.)* obSCEne's tool carries three
  dependencies, each argued for in its own manifest, and `forbid(unsafe_code)`. A transport crate with a runtime or a
  serialisation framework in it could not be taken by that project without breaking a
  policy it holds on purpose. Checksums and manifests therefore live one layer up, in
  `pros-core`, which obSCEne never depends on.
- **Relative path dependencies, no feature flags, no duplicated code.** The alternative -
  publishing `pros-link` so obSCEne can depend on a version - trades a checkout convention
  for a release process, and a release process is a worse thing to owe.
- **The known cost is accepted, not discovered later.** `obscene-tool` stops building from
  a standalone clone of `obscene`. The projects are checked out as a set under OOPS, and
  this entry is where somebody who did not choose that finds out why.
- **The shims hold nothing.** `pros-cli` and `pros-gui` are interaction surfaces over the
  crates, which is Orbistoun's shim rule adopted as a starting condition rather than
  arrived at after the first drift.

### The name

`orbistoun` embeds *orbis*; `obSCEne` embeds *sce*; `PROSPERous` embeds *prospero*, and is
the only common English word that contains the whole codename. That is a low profile in
the sense principle 2 means it - not concealment, since the subject is obvious from the
first paragraph of any file, but no reason to repeat brand names either. The binary is
`pros`.

### Carried over from the reference implementation

Three of obSCEne's decisions are adopted rather than reconsidered, because each was paid
for once already:

- **A registration is an address and a name, and nothing else.** Capabilities do not
  survive a power cycle, so a cached one is a claim that expires without notice. Probe on
  every use.
- **A check reports what each service unlocks**, not up or down, and separates required
  from optional - they fail differently and call for different work.
- **Slow answers are surfaced.** A port refusing instantly and one refusing after 1500 ms
  mean different things.

### Three facts the target taught, now design constraints

- `elfldr` is launched by `pldmgr`, and so is everything else - including `elfldr`. When it
  dies nothing can restart it and the dashboard keeps answering. It is checked first, and a
  check that finds it down says *re-run the jailbreak* rather than *reload a payload*.
- A vendor module and a plain ELF share their first four bytes, so a loader accepts either
  and then dies silently on the one it cannot run. `pros-link` guards on `e_type` at offset
  `0x10` before sending, once, for everybody.
- The loader duplicates its connection socket onto the payload's output. That is a
  convenience and never a mechanism: a payload started any other way has no such socket,
  and nothing above `pros-link` may assume it.

### Payloads are fetched, never shipped

A manifest of where to get them, editable outside the source tree. GPL-3.0 payloads make
redistribution an obligation and a pointer none; URLs rot; and obSCEne's CI already refuses
a tracked `.elf`. The schema is copied from `pldmgr`'s own `repository_cache.json` rather
than invented, which also lets Prosperous read a target's existing repository as a source.

**Checksum verification is not optional and the ordinary path offers no way past it.** The
tool downloads from a mirror and then executes the result with kernel-adjacent privileges.
That is the whole risk surface, in one sentence.

### Two video problems, and neither of them is remote play

**Reversed on 2026-09-01.** This originally read *only one of them is remote play*, and said
watching was Chiaki's job. Driving somebody else's client was built, worked, and has been
removed - module, panel, registration helper and all.

It was the right call while this project had no stream of its own. It stopped being the right
call the moment it did: two ways to watch, one of which needs pairing, a vendor's worth of
protocol and an AGPL dependency to talk to firmware this project never talks to, is one way
too many. **A target this project cannot watch is a target whose payload has not landed** -
which is a sentence, not a reason to carry a second implementation of watching.

What remains is two problems, both ours:

- **Watching** is Porthole. The target encodes and serves its own stream; this reads it,
  counts it, and pipes it to a player - `docs/VIDEO.md` part three.
- **Diffing** cannot use a stream at all: a lossy codec destroys a signal that obSCEne
  already measures to one ULP, and orbistoun's oracle list calls framebuffer diffing the only
  cheap mechanical correctness signal it has. That needs a lossless single-frame grab, whose
  client half is built in `pros-link::frames` and whose payload half belongs in obSCEne.

Left open for obSCEne to answer by asking the target: **is the target encoder reachable from
an unsigned payload?** That now decides whether live watching exists at all, rather than
whether it needs to be more than Chiaki.

### Not a concern, recorded so it is not re-argued

**Authentication and transport security.** Every service here is unauthenticated on the LAN
by design; that is what a payload chain is. A login on a tool that drives an open FTP
server and a raw shell would be theatre.

