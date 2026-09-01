# Video

**Two problems that look like one and share no code.**

*Watching* is a person seeing what the target is doing. *Diffing* is a machine deciding
whether one frame matches another to the byte. They are answered separately, and the reason
is the codec: a lossy stream is fine for the first and destroys the second.

**Both are ours.** This project used to hold a third answer - launching somebody else's
remote-play client and supervising it - and that is gone. It was the right call while the
target side of [part three](#part-three-porthole) was hypothetical, and it stopped being the
right call the moment this project had its own stream: two ways to watch, one of which needed
pairing, a vendor account's worth of protocol and an AGPL dependency, is one way too many. A
target this project cannot watch is a target whose payload has not landed, and that is a
sentence rather than a reason to carry a second implementation.

Part of this is built and part is not - the split is called out where it falls, and
[part three](#part-three-porthole) is the built half. What follows is the design, so that
building the rest is an afternoon rather
than a fortnight of deciding.

---

## Part two: diffing

### Why a stream cannot be used for this

orbistoun's oracle list calls framebuffer diffing **the only cheap, mechanical correctness
signal in the whole codebase**. obSCEne's GPU comparison already resolves differences of one
unit in the last place.

A lossy codec does not degrade that signal. It destroys it. Two frames that differ in one
pixel encode to the same bytes; two frames that are identical encode differently depending
on what preceded them. Every measurement taken through it would be a measurement of the
encoder.

### The shape: a resident grabber, not a one-shot

A payload sent by the loader **could** grab a frame and write it back over the socket the
loader duplicated onto its output. That is the obvious design and it is wrong twice:

1. It rests on the loader's socket duplication, which is a **convenience and never a
   mechanism**. A payload started any other way has no such socket.
2. Diffing needs *repeated* grabs - before and after, frame N and frame N+1 - and a
   one-shot means re-sending and re-running a payload between every pair.

So the grabber is a resident payload that opens **its own listening socket**, exactly as
every other service in the chain does. Sent once, asked many times.

### The port

**9022, chosen and not measured.** Adjacent to the loader so the two are memorable
together, and outside every port the chain used as measured on 2026-08-25: 9021, 2121,
3232, 2323, 8084, and 6967 for scripted input. If it turns out to collide with something,
this is a one-line change and a note here saying what it collided with.

### The request is a line of text

```
GRAB\n
```

That is the whole request. **Deliberately typeable**, because most of what goes wrong in
this project is diagnosed by hand with a socket and a keyboard, and a binary request format
would cost that for nothing - there is one command.

### The response describes itself

A fixed 32-byte header, little-endian - the platform's own byte order, so the payload writes
structures it already holds rather than swapping bytes it might swap wrongly:

| offset | size | field | meaning |
|---|---|---|---|
| 0 | 4 | magic | `PFRM` |
| 4 | 2 | version | 1 |
| 6 | 2 | status | 0 succeeded; anything else is why not |
| 8 | 4 | width | pixels |
| 12 | 4 | height | pixels |
| 16 | 4 | format | **as the platform reports it**, untranslated |
| 20 | 4 | stride | bytes per row, which is not width times four |
| 24 | 8 | bytes | how many pixel bytes follow |

Then exactly `bytes` bytes, then a 4-byte checksum of them.

Four rules make this worth writing down:

- **The format is reported, never assumed.** A diff against a frame whose stride was
  guessed fails, and it fails as *the emulator is wrong* rather than as *the client
  guessed*. That is a day lost to the wrong question.
- **A non-zero status means no pixels follow.** *It did not work* and *it worked and
  produced nothing* must not look the same - the defect this project keeps meeting.
- **`bytes` is authoritative and a short read is an error.** A truncated transfer must not
  arrive as a smaller frame, because a smaller frame diffs perfectly well and says nothing
  true.
- **The format field is passed through, not interpreted.** If the payload cannot determine
  the format it reports a status and sends nothing, rather than labelling the pixels with a
  guess.

### The checksum is not cryptographic, and that is not an oversight

The threat here is a truncated or corrupted transfer over a local network, not an adversary
substituting a frame. FNV-1a over the pixel bytes is six lines in freestanding C and
catches everything that is actually likely.

This is the opposite call from the payload manifest, where the checksum guards a download
that is about to be executed with kernel-adjacent privileges. Different threat, different
answer, and the difference is worth stating so neither gets changed to match the other.

### Size and time, from what has been measured

At 1920x1080 and four bytes per pixel a frame is **8.3 MB**. The file service was measured
at 13-23 MB/s on 2026-08-25, so a frame is **0.4 to 0.6 seconds** if the grab socket
performs like the file one. At 3840x2160 it is 33 MB and four times that, which is a fact
worth knowing before anyone diffs a 4K title in a loop.

---

## The client half

```rust
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub format: u32,   // as the platform reported it
    pub stride: u32,
    pub pixels: Vec<u8>,
}

pub fn grab(address: &str) -> Result<Frame>;
```

And one operation that is the entire point:

```rust
impl Frame {
    /// How many pixels differ. Refuses frames of different shape.
    pub fn differences(&self, other: &Frame) -> Result<usize, Mismatch>;
}
```

**Comparing two frames of different geometry or format is an error, not a resize and not a
partial compare.** A tool that quietly compares the overlapping region of a 1080p and a 4K
frame will report a difference count, and that number will be believed.

---

## Where each half lives

The **payload** belongs in obSCEne. Homebrew that runs on the target and reports what it
saw is that project's exact description, it already declares the `sceVideoOut` names, and
its principles are the ones this needs: announce before attempting, and leave out anything
whose signature is uncertain rather than guessing an arity.

The **client** is `pros-video`, here.

That split is why this document is in this repository and the code will not all be.

---

## What "built" means

Five checks, none of which require believing anything:

1. **Two grabs of a static scene are byte-identical.** If they are not, nothing below this
   line means anything.
2. **`stride` x `height` equals `bytes`.** A header that disagrees with its own payload is
   a header that cannot be trusted about anything else.
3. **A grab with nothing displayed returns a status, not a frame of zeros.** Black and
   absent are different, and a frame of zeros diffs against another frame of zeros
   perfectly.
4. **A deliberately truncated transfer is refused**, by cutting the connection mid-frame and
   confirming the client errors rather than returning what arrived.
5. **A frame diffed against itself is zero, and against a one-pixel change is exactly one.**
   The measurement instrument gets measured before it is used.

---

---

## Part three: Porthole

**Porthole** is the stand-in. Parts one and two are a client we launch and an instrument we
read; this is neither - **our own stream and our own input, over our own payloads**, so that
watching and playing a target does not require speaking the vendor's protocol at all.

The name plays on the vendor's own remote-play handheld the way `obSCEne` plays on the platform
owner's initials: a *port*-shaped opening, and the one kind of opening that is also a window you
watch a screen through. It names what you do with the portal, not merely that there is one.

It exists because the alternative is expensive. Remote play means pairing, a UDP transport,
ECDH with per-session AES-GCM, Reed-Solomon FEC, two video codecs and Opus - and the open
client that does all of it is AGPL, so embedding it relicenses this project. **Every one of
those costs is paid to talk to *unmodified* firmware**, and this project only ever talks to
jailbroken ones. Driving somebody else's client instead was tried and removed: it worked, and
it made watching a thing this project explained rather than a thing it did.

We are not talking to unmodified firmware. The target is jailbroken and runs our code. So the
whole protocol is a decision rather than a specification to reverse.

### The shape

```
target                                          this machine
------                                          ------------
capture the composited output                   \
      |                                          |  one payload
   encode it in hardware                         |  serving two ports
      |                                         /
  9805  ---- encoded video, framed --------->  read it, count it, pipe it to a player
  9806  <--- controller state, 60/s ---------  read a pad, or the keyboard
```

Two sockets, two directions, no negotiation. A LAN, a jailbroken target, and a person who
already trusts both.

### Which half exists

**This side is built. The target side is not.** That split is the whole state of part three
and it is worth saying plainly, because it is easy to read a design document as a plan and
miss that half of it already runs.

| | here | on the target |
| --- | --- | --- |
| video | `pros-core::watch` connects, counts, pipes to a player | nothing |
| input | `pros-link::{pad,pads,feed}` maps and sends | nothing |
| the panel | the stream section is these controls | - |

The stream panel is **not** a description of a Porthole that might exist. It is Porthole's
own controls, pointed at a port nothing is serving yet. Pressing *watch* today produces
*connection refused*, naming the port - which is a more precise statement of what is missing
than any paragraph, and becomes a working stream the day the payload lands with nothing here
to change.

### Video: the payload encodes, and nothing here decodes

**The target already encodes video in hardware, continuously, for its own recordings.**
`libSceVencCore` and `libSceVideoRecording` drive that block; obSCEne's corpus names all 38 of
their symbols and marks them callable. Whether an unsigned payload can reach them is the open
question at the bottom of this document, and it is the question the whole of part three rests
on.

If it can, the client is almost nothing. **Encoded frames on a socket are something every
media player already reads**, so this pipes them to one:

```
mpv --demuxer=h264 --profile=low-latency --untimed --no-cache -
```

That is the same arrangement `stream.txt` and `fetch.txt` already use elsewhere in this
project: name the program in an editable line, run it, do not reimplement it. A decoder here
would mean a substantial C or C++ dependency reached through FFI, in a workspace that
**forbids** unsafe code rather than discouraging it - to display a picture that `mpv` displays
for free.

**But the socket is opened here and the bytes are piped in, rather than the player being
pointed at the address.** That is one extra hop and it buys the only thing a player cannot
give: a player answers *is there a picture*, and says exactly the same thing whether nothing
arrived, something arrived that was not video, or video arrived carrying no keyframe. Those
are three faults in three different places, and the third is the one that hides - a stream of
nothing but dependent pictures **decodes to nothing and looks exactly like no stream at all**.

So `pros-core::watch` connects, feeds every read through `pros-link::stream` on the way to the
player's standard input, and counts bytes, units and keyframes as they pass. The player shows
the picture; the counts say what went by. Neither has to be trusted about the other's job.

**So the framing is chosen to be the one players already accept**, not the one that would be
tidiest to write: Annex B, start-code delimited, exactly as the encoder emits it. No container,
no length prefixes, no header of ours. A byte stream a player can be pointed at directly is
worth more than a format that needs our client to be running.

`pros-link::stream` reads the same framing, for when something here does need to know what is
in the stream - counting frames, finding the first keyframe, confirming a payload is emitting
anything at all. **Reading is not decoding**, and the split is deliberate.

### Input: small, and entirely ours

Nothing is being reverse-engineered here. The pad report layout is public - the vendor's
controller has an in-tree Linux driver and an open userspace implementation - and both ends of
the wire are ours to write, so the format is a decision rather than a guess.

A fixed 24-byte record, little-endian, one per update:

| offset | size | field |
|---|---|---|
| 0 | 4 | magic `PPAD` |
| 4 | 2 | version |
| 6 | 1 | slot, 0-3 |
| 7 | 1 | reserved, zero |
| 8 | 4 | buttons, one bit each |
| 12 | 4 | four stick axes, unsigned, 128 is centre |
| 16 | 2 | trigger pressures |
| 18 | 2 | reserved, zero |
| 20 | 4 | sequence number |

**The button bits and the stick range are not ours to choose.** They are the ones the target's
own pad structure uses, confirmed empirically by the Ghostpad project against real hardware -
see `ACKNOWLEDGEMENTS.md`. An earlier draft of this invented both, which would have produced a
controller where every button was a different button and both sticks rested hard left and up.

Two consequences worth stating, because each is a press that silently does not happen:

- **A trigger sets its bit *and* its pressure byte.** The target reads both, and the bit alone
  does not register - which looks like a dead button and gets diagnosed as a network fault.
- **`0x0002_0000` is left unassigned.** It is documented as producing an unintended Cross
  press, and it is exactly the bit somebody counting upwards would reach for next.

Four choices worth stating, because each has an obvious wrong alternative:

- **Fixed size, not a text line.** This goes 60 times a second or faster. The rest of this
  project prefers text for things diagnosed by hand with a keyboard; a pad is not one of them,
  and a parser that has to find field boundaries at 250 Hz is a parser that drops inputs.
- **A sequence number, and the receiver may skip.** Input is a *state*, not an event: the
  newest record supersedes every older one, so a receiver behind by three should apply the
  last and drop two. A queue that delivered all three would replay stale sticks.
- **Absolute state, never deltas.** A dropped delta is wrong forever; a dropped state is wrong
  for sixteen milliseconds.
- **The slot is in the record, not in the connection.** Four pads over one socket, each
  saying which it is. A payload inferring the slot from which connection carried it would put
  pad two's input on pad one the first time sockets reconnected in a different order - and it
  makes the sequence number per-slot, which is the only way *behind by two* is answerable.
- **A slot beyond the fourth is refused, not clamped.** A record for a fifth pad is a sender
  believing something untrue, and delivering it to the fourth makes one person's input arrive
  as another's.
- **The reserved byte is zero and is checked.** Gyro, touchpad and rumble are the obvious
  additions, and a version bump into room already reserved is cheaper than a second format.

### What this deliberately is not

- **Not secure.** No pairing, no encryption, no authentication - two open ports on a LAN. That
  is the same posture as every other service this project talks to, and it is stated rather
  than implied. Do not put a target on an untrusted network and do not leave it running.
- **Not lossy-network tolerant.** TCP, so a lost packet stalls the stream rather than degrading
  it. Remote play uses UDP with forward error correction precisely because that matters over
  wifi. **On a wired LAN this is fine and over wifi it will stutter**, and the fix is not a
  small one - it is most of why the vendor's protocol is the size it is.
- **Not audio, yet.** Video and input first. Audio is a third socket and the same argument,
  and adding it before either of the others works would be building on nothing.
- **Not for an unmodified target.** This exists *because* the target is ours and runs our
  code. A console without the payload is not watched by this project at all, which is a
  smaller claim than the one it replaced and an honest one.

### What has to be true for any of it

One question, and it is on the target:

**Can an unsigned payload reach the encoder?** If `sceVencCoreCreateEncoder` and
`sceVencCoreGetAuData` answer, part three is a few hundred lines at each end. If they refuse,
the fallback is part two's raw grabs - 8.3 MB per 1080p frame, which at the file service's
measured 13-23 MB/s is **two frames a second**. That is an instrument, not a stream, and the
honest answer would be that live watching is not something this project offers.

Everything above is designed so that answering that one question decides the rest, and so the
answer costs one session on hardware rather than a redesign.

---

## Open questions, for target to answer

- **Is a target encoder (`libSceVideoEnc`, the VCE block) reachable from an unsigned
  payload?** This decides whether live streaming ever needs to be more than Chiaki, and it
  is precisely obSCEne's kind of question: call it, record what came back, grade it by what
  it ran on.
- **Is the display buffer reachable at all from an unsigned payload, and in what colour
  space?** If it is not, this design is worth nothing and the answer is worth having early.
- **Does grabbing perturb what is being measured?** A grab that stalls the display pipeline
  changes the thing under test, and a diffing harness built on it would be measuring its own
  interference.

Each of these is a reason this is designed now and built later: **the design costs an
afternoon and the answers cost target time**, and doing them in that order means the
target time is spent on questions rather than on discovering which questions to ask.
