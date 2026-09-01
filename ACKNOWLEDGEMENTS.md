# Acknowledgements

Prosperous is plumbing. Almost everything it talks to was written by somebody else, and
this file exists from the first commit rather than being added when somebody asks.

**Nothing here is forked.** Payloads are used unmodified, fetched from their own
distribution rather than redistributed, and credited. Prosperous is an
emulator-development instrument that shares plumbing with existing tools - not a
competitor to any of them.

## The payload chain

The target services this tool speaks to are not ours. Each is a separate project with its
own authors and licence, and Prosperous ships none of their binaries - only a manifest
saying where to get them.

| project | what it provides |
|---|---|
| `ps5-payload-dev` payloads (`elfldr`, `ftpsrv`, `klogsrv`, `shsrv`) | the services this tool is a client for. GPL-3.0 |
| `pldmgr` | the payload manager, its dashboard, and the repository schema this project copies rather than invents |
| `ShadowMountPlus`, `ps5upload`, `nanodns`, `kstuff-lite` | the rest of the chain a working target runs |
| `linkdev` (`ps5-payload-dev`) | remote-play registration without a vendor account |

The repository schema in `pldmgr`'s `repository_cache.json` is adopted directly. Copying a
working schema is better than inventing a second one, and it means a target already
configured is already described.

## Remote play

**Chiaki / chiaki-ng.** Remote play is used rather than reimplemented. The protocol
involves pairing, a bespoke UDP transport, per-session authenticated encryption,
forward error correction, two video codecs and an audio codec - which is why Chiaki is
large, and why writing a second one would be a project rather than a feature.

## Ghostpad, for the controller layout

**Its published measurements, not its code.** `crates/pros-link/src/pad.rs` uses the button
bitmap and the stick range that project confirmed empirically against a real target, and its
`virtualDS5research.md` records how each bit was established. It credits shadPS4's `pad.h` for
the underlying enum, and that chain is worth repeating rather than flattening.

**The distinction is deliberate and it is also a licence.** Ghostpad's payload is
GPL-3.0-or-later; this workspace is MIT or Apache-2.0, so none of that code can be taken and
none has been. What was taken is a set of facts about a vendor structure - which bit is which
button, that a stick axis is an unsigned byte centred on 128 - and facts are not the code that
found them.

Three of those facts corrected a draft written without them, and each would have failed in a
way that looks like something else:

- the button numbering here was **invented**, so every button would have been a different one;
- the sticks were signed and centred on zero, so a neutral pad would have rested hard left and
  up, and the extra precision was thrown away by the wire in any case;
- a trigger set only its pressure byte, and the target reads the bit as well - a press that
  never registers and reads as a dead button.

`0x0002_0000` is left unassigned here for the same reason that document gives: it produces an
unintended Cross press, and it is the next bit somebody counting upwards would take.

**What is still deferred is the technique.** Ghostpad reaches a virtual pad by patching a
system library in live memory and injecting a thread into the pad daemon, which is more
invasive than anything else in this chain and is firmware-specific by its own account. Nothing
here does that. If that changes it is credited again, and separately.

## orbistoun, for three input decisions

Its `orbistoun-input` grew a controller model for its own reasons, and reading the two side by
side found three things wrong here:

- **Level or edge.** Its window reads a key as down if it is held *or* was pressed this frame.
  This one read only what was held, so a press and release landing inside a single frame was
  invisible - and the shortest real tap on a fast display is close to that. A controller that
  misses inputs and cannot say why.
- **Conflicts reported, not resolved.** Binding a key here used to take it from whatever else
  had it, which was the same failure it meant to prevent seen from the other side: the button
  it silently unbound was dead and nothing said so.
- **A key map per port**, kept when the port changes hands. One shared layout does not make a
  second keyboard player awkward, it makes them impossible.

Its `ShellButton` - tap against hold, with elapsed time passed in rather than a clock read -
is the better pattern for that problem and is deliberately **not** copied: this project
forwards absolute state and the target decides what a hold means, so interpreting one here
would be duplicating the target's own logic in a place that cannot see the result.

The two projects also disagree on purpose about how a pad is represented, and both are right.
See `docs/DECISIONS.md`.

## Sibling projects

**orbistoun** and **obSCEne** are the two consumers, and obSCEne was also the reference: its
`tool/src/target.rs` is where this project's transport, its registry design and its `e_type`
guard come from. Those decisions were paid for once and are adopted rather than reconsidered.

**That file no longer exists**, and the reason is the point of this project: obSCEne deleted it
and took `pros-link` by path instead, so the code it lent is now the code it consumes. The
credit stands - the design was theirs first - but do not go looking for the path.

## Documentation and reference

Where a lawful public reference settles a fact, it is cited at the point of use rather
than summarised here. Where nothing settles it, the code says so and the question is
recorded as open - which is the sibling projects' rule and is adopted with the rest.
