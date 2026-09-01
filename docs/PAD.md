# The pad, and where it should live

**A proposal, not a decision.** Nothing here is built and nothing here has moved. It exists so
that the move, if it happens, is an afternoon rather than a fortnight of deciding - and so that
the reasons survive whether or not it does.

It follows from a question worth quoting: *"a shared pad code base would be good, but I don't
see how it's not already under orbistoun-inputs."*

The short answer is that `orbistoun-inputs` is a **host input** crate, and what this project
needs from it is a **hardware fact**. Those look like the same thing and are not.

---

## What is duplicated today

Three repositories in the sibling layout touch a controller:

| | what it holds | why |
| --- | --- | --- |
| `orbistoun-inputs` | `PadState` - host-shaped floats | describes what a person's hands are doing |
| `pros-link::pad` | `Pad` - unsigned bytes, centre 128 | what the target's own structure looks like |
| `pros-link::pads` | slots, key layouts, conflicts | mapping a keyboard onto four pads |

The overlap that is real: **which buttons exist**, what each is called, which keyboard key
means which button by default, and how to notice that one key has been bound to two things.

The overlap that looks real and is not: the encoding. That distinction is the whole of D023 and
it is still right, though its reasoning has moved - see below.

---

## Why it is not already in `orbistoun-inputs`

Because that crate solves the half this project does not have, and stops exactly where this
project starts.

```
       orbistoun-inputs                       what prosperous needs
       ----------------                       ---------------------
  gamepad / keyboard                          keyboard
        |                                          |
   PadState (floats)  <-- shared shape? -->   Pad (measured bytes)
        |                                          |
  read by an emulator                         24 bytes on a socket
  that owns both ends                         parsed by a payload
```

`orbistoun-inputs` maps host input to a pad model **that orbistoun itself reads**. It owns the
producer and the consumer, so the model between them can be whatever is convenient - and floats
are convenient, because that is what a gamepad axis is.

This project's `Pad` is not read here at all. It is written to a wire and parsed by a payload on
the target, so its layout is not ours to choose. **It is a measurement, and the reason it is
bytes centred on 128 is that this is what the target's own structure is.**

So the thing that is missing from `orbistoun-inputs` is missing because that crate was never
about the hardware. Its own documentation says so.

---

## What changed since D023

D023 says two pad models exist on purpose, and it is worth being precise about which part of it
has aged.

**The conclusion holds.** Host-shaped floats and measured wire bytes are different things,
neither should become the other, and neither is a worse version of the other.

**One premise has not.** D023 quotes `orbistoun-inputs` saying the guest-facing layout *"is
unmeasured and this type deliberately does not guess at it."* That was true when written. It is
no longer: the button bits have since been measured, credited to Ghostpad in
`ACKNOWLEDGEMENTS.md`, and three real defects in this project's table were found against them.

That does not make orbistoun wrong to have declined to guess. It makes the guess unnecessary,
which is a different situation and one D023 could not have described.

So there is a **third** thing, which is neither of D023's two models: the measured facts
themselves. Not a format, not a mapping - a small pile of things that are true about the
hardware, which both projects would otherwise have to learn separately and could disagree
about.

### An inconsistency in D023, while we are here

D023 lists tap-versus-hold among what *is* shared, and then argues in its last paragraph that
tap-versus-hold **does not belong here** - because this project forwards absolute state sixty
times a second and lets the target decide.

The last paragraph is the right one. The list should not name it. Worth fixing when D023 is
next touched, rather than left as two sentences that disagree.

---

## The proposal

A crate holding **measurements about the hardware's controller, and nothing else**.

Working name `orbistoun-pad`, because the naming and provenance conventions are orbistoun's and
this belongs beside `selfish` in spirit: facts about the hardware, from citable evidence, usable
by anything that needs them.

### What goes in

- **Which buttons exist**, and the bit each one is. Measured.
- **That `0x0002_0000` is poisoned** - it produces a phantom Cross press, so no button may
  claim it. This is the single most valuable thing in the pile: it is a fact that is *only*
  learnable by having been burned by it, and a second project counting bits upwards would be
  burned identically.
- **That the axes are unsigned bytes centred on 128**, so a zeroed structure is not a resting
  pad but both sticks held hard left and up.
- **That a trigger has both an analogue byte and a digital bit**, and that setting one without
  the other produces a pull the target half-notices.
- **The names and the glyphs** - the word for files and logs, the shape for screens.

Every one of those is a statement about the hardware that is true regardless of who is asking.

### What stays out

- **The wire format.** `PPAD`, the version field, the slot byte, the sequence number: **we
  invented all of it**, both ends are ours, and it is not a fact about anything. It stays in
  `pros-link`.
- **Keyboard layouts, conflict detection, slots.** These are about a keyboard and a person,
  not about the hardware. orbistoun has its own reasons for its own bindings and should keep
  them.
- **`PadState`.** It stays exactly as it is. Nothing here asks orbistoun to change a type.
- **Tap-versus-hold.** See above - it is an emulator's problem, and implementing it here would
  duplicate the target's own logic in the one place that cannot observe the result.

### Why this shape and not a bigger one

Because a crate of facts depends on nothing, and therefore **cannot create a cycle**. That was
the objection to the obvious version of this - prosperous depending on orbistoun while
orbistoun reaches hardware through prosperous's transport - and it disappears entirely once the
shared thing has no behaviour to depend on anything.

It also means orbistoun takes on no obligation. It can adopt the crate when it builds the
boundary where guest code reads a pad, and until then the crate is simply a place the
measurements are written down correctly.

---

## What it costs

Honestly: a third repository in the sibling layout, for something currently about two hundred
lines. That is the argument against, and it is not nothing.

The argument for is one measurement. **The Ghostpad table was wrong here in three separate
ways** - invented bits, signed sticks, a trigger missing its digital bit - and each was found
by reading somebody else's implementation rather than by any test passing. If that table is
ever wrong again, it should be wrong in one place.

## What has to be true before it happens

- **Provenance, per orbistoun#D242.** The bits are measured and their source is Ghostpad,
  already credited. The crate must carry that per-fact rather than as a file header, because a
  crate of facts whose facts are not individually attributable is a crate of assertions.
- **A decision on whether `selfish` is the right home instead.** It already holds *the
  hardware's file formats from citable sources only*, which is close to this charter. The
  argument against folding it in is that a controller is not a file format; the argument for is
  that one repository of measured hardware facts beats two. **Unresolved, and it should be
  resolved before anything moves.**
- **Nothing in flight in either repo's input code.** Moving a type while somebody is editing it
  is how the three defects get reintroduced.

---

## If it does not happen

The duplication stays, and this document becomes the thing that stops it drifting: the bits are
here, the poisoned one is named, and the reason `Pad::rest()` is not `Pad::default()` is
written down. **That is most of the value at a fraction of the cost**, which is worth saying
plainly rather than treating the crate as inevitable.
