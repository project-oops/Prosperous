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

The **client** is here - not a `pros-video` crate (see [DESIGN.md](DESIGN.md)): the frame-grab client is `pros-link::frames`, and Porthole's counting-and-piping half is `pros-core::watch`.

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
their symbols and marks them callable. Whether an unsigned payload can reach them was the
question the whole of part three rested on - and hardware answered it (see "What has to be true",
below): the encoder sysmodule loads, and its symbols are reached by walking the loaded module's
exports.

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

- **Not secure - on the target leg.** No pairing, no encryption, no authentication - two open
  ports on a LAN. That is the same posture as every other service this project talks to, and it
  is stated rather than implied. Do not put a target on an untrusted network and do not leave it
  running. [Part four](#part-four-the-moonlight-bridge) adds a *client-facing* leg that **is**
  paired and encrypted, because Moonlight requires it; the target leg described here is unchanged.
- **Not lossy-network tolerant - on the target leg.** TCP, so a lost packet stalls the stream
  rather than degrading it. Remote play uses UDP with forward error correction precisely because
  that matters over wifi. **On a wired LAN this is fine and over wifi it will stutter**, and the
  fix is not a small one - it is most of why the vendor's protocol is the size it is. That fix is
  exactly what [part four](#part-four-the-moonlight-bridge) buys on the client-facing leg (RTP/UDP
  with Reed-Solomon FEC); this target leg stays plaintext TCP, by the argument above.
- **Not audio, yet.** Video and input first. Audio is a third socket and the same argument,
  and adding it before either of the others works would be building on nothing.
- **Not for an unmodified target.** This exists *because* the target is ours and runs our
  code. A console without the payload is not watched by this project at all, which is a
  smaller claim than the one it replaced and an honest one.

### What has to be true for any of it - answered, and it is a go

One question, and it was on the target: **can an unsigned payload reach the encoder?**

**Answered on hardware, 2026-09-01.** obSCEne's `106-encoder` section loaded the sysmodules by id,
and the two Porthole needs came in clean: the video **encoder** (`VENC`, id `0xa0`) and **recording**
(`VIDEOREC`, id `0x81`) both returned `0x0` - loaded - from an unsigned payload, while the decoders
refused (`0x805a1000`). So the VCE block comes into the process.

The catch is the one every obSCEne payload meets, not one specific to the encoder: the `sceVencCore*`
symbols do **not** auto-bind. The census read all of them `unresolved`, and opening the `.sprx` by
path returns `0x80020002` (no entry). But the module *is* loaded - the run saw two modules and a live
module handle - so its symbols are reached the way obscene#D277 already describes: **walk the loaded module's
export table by base+vaddr and resolve them**, rather than relying on a bound import table.

So `porthole_encoder_open` is three known steps: `sceSysmoduleLoadModule(0xa0)` (proven), self-resolve
`sceVencCore*` from the loaded module (the next real piece), then `sceVencCoreCreateEncoder`. The
part-two raw-grab fallback is not needed for reachability - the door opens. What is still unmeasured
is the two questions below it: the display buffer, and whether grabbing perturbs the pipeline.

---

## Part four: the Moonlight bridge

**Part three makes one client — ours, on a PC. This makes every client, on every device**, for
the cost of a bridge on the machine that already runs Porthole's host half. It changes nothing on
the target and nothing in the payload: it is a *second consumer* of 9805 and 9806, sitting beside
the `mpv` pipe rather than replacing it.

The decision that shapes it is the operator's, dated 2026-09-10: **the Moonlight protocol is not
implemented on the console.** Putting TLS, RTSP, ENet, AES-GCM and Reed-Solomon into a freestanding
C payload — whose whole job is to push hardware-encoded frames out of a socket — is the opposite of
what part three exists to argue. Prosperous already owns the host side, already has a registry of
targets, and runs where a Rust TLS stack is free. So the protocol lives here, one LAN hop from the
target, and every existing Moonlight client works the day the payload lands.

### The shape

```
target                     this machine (Prosperous bridge)              any Moonlight client
------                     --------------------------------              --------------------
9805 --H.264 Annex-B-->     split NALs, packetise, RS-FEC     --RTP/UDP 47998-->  phone / Deck / TV
9806 <--PPAD 60/s------     map controller packets to PPAD    <--ENet 47999-----  its gamepad
                            pair (HTTPS 47984 / HTTP 47989), RTSP 48010, mDNS _nvstream._tcp
```

The target leg is exactly part three's two sockets, unchanged. The client leg is the NVIDIA
GameStream protocol as Moonlight reverse-engineered it and Sunshine re-implemented it: discovery
over mDNS, an HTTP/HTTPS pairing and session handshake, RTSP to negotiate the stream, then RTP
video out and an ENet control-and-input channel back.

### Two legs, two security postures, and both are honest

Part three's "[what this deliberately is not](#what-this-deliberately-is-not)" describes the
*target* leg, and every word of it still holds — that leg is unchanged. The **client** leg is a
different posture because Moonlight requires it to be:

| | target ↔ bridge (9805/9806) | bridge ↔ client (47998/47999/…) |
|---|---|---|
| transport | plaintext TCP | UDP for video, ENet for control |
| loss tolerance | none — TCP stalls | Reed-Solomon FEC, wifi-tolerant |
| authentication | none — trusted LAN | 4-digit-PIN pairing, per-client cert |
| encryption | none | AES-GCM after RTSP negotiation |

The target leg is plaintext on a trusted LAN **by the same argument as before**: the target is
ours, on a wire we control, and pairing it would be securing the half of the path that does not
need it. The client leg is encrypted and FEC-protected **because the protocol says so** — a
Moonlight client will not pair with a host that answers otherwise, and the whole reason to speak
this protocol is that clients already do. So the bridge is the seam where a trusted plaintext LAN
segment meets a paired, encrypted, loss-tolerant one, and that is stated rather than smoothed over.

### The bridge does not decode video, ever

**"[Reading is not decoding](#video-the-payload-encodes-and-nothing-here-decodes)" holds here too.**
The bridge reads Annex-B off 9805, finds NAL boundaries and keyframes, and packetises — it never
decodes a frame. `pros-link::stream`, already the NAL splitter and keyframe finder part three uses
for its counts, is the same code the packetiser reuses: one reader that knows where a NAL starts
and whether it carries an IDR, feeding both the byte/unit/keyframe counts (still the oracle for "is
there a picture" that a client cannot give) and the RTP packetiser. No re-encode, no transcode, no
buffering beyond one frame. **H.264 only** for now — that is what the target's VENC path is
expected to emit; the RTSP `DESCRIBE` advertises it and nothing else until a second codec is measured.

### What the client dictates, and what the target does not yet expose

A Moonlight client owns the encoder's controls: it asks for a keyframe on loss and it sets
resolution, fps, bitrate and codec at launch. Porthole's 9806 carries only `PPAD` input today, so
the bridge has nowhere to send either. **The target-side shape is filed, not built** —
oops-apps `REQ-20260910T2326Z-8c12` proposes a 24-byte `PCTL` control record on 9806 (op 1 =
request-keyframe, op 2 = set-mode), dispatched beside `PPAD` by magic. Until it is answered the
bridge maps the client's *request IDR* and *invalidate reference frames* messages to op 1 and its
`launch` mode to op 2, **sends them, and logs the ones it could not honour** — honest failure, not
a silent freeze.

### Provenance and licence, up front

The GameStream wire protocol has no published spec; it is defined by three GPLv3 codebases:
**moonlight-common-c** (the Moonlight client core), **Sunshine** (LizardByte, the reference host),
and **Wolf** (games-on-whales, an independent second host — the useful cross-check for what the
*protocol* requires versus what Sunshine happens to do). Prosperous implements the **behaviour**
those describe and cites them in `ACKNOWLEDGEMENTS.md`; it copies no code and links no GPL library,
for the same reason part three gives for not embedding the AGPL remote-play client. Implementing
somebody else's documented behaviour is the one kind of protocol work this project does (principle
1); relicensing it by linking is not.

### Built in the order it can be tested without a console

The whole client leg is verifiable on this machine alone, against a stock `moonlight-qt` on the
same LAN, with **no target involved** — which is how the mesh wants it, since obSCEne alone touches
hardware. It lives in its own crate, `pros-moonlight`, reached by two `pros` verbs: `fake-target`
and `moonlight`. The **fake target** serves an Annex-B H.264 file on 9805 in a loop and sinks 9806,
printing the `PPAD` records it receives. Then, in order:

1. **Discovery + pairing + app list — built and verified.** The bridge advertises
   `_nvstream._tcp`, serves `serverinfo` over HTTP (47989) and HTTPS (47984), runs the full
   four-phase PIN pairing (SHA-256 salted key, AES-128-ECB challenges, RSA-signed commitments)
   exactly as Sunshine does, and offers **one app per registered target**. The self-signed RSA
   cert is generated once and persisted so a client's pinning survives a restart; the HTTPS port
   presents that same cert for the final pair challenge. Proven by tests that play a real client
   through every phase over real sockets (`crates/pros-moonlight/src/serve.rs`), and by the running
   `pros moonlight` answering `serverinfo` on both ports.
2. **Session + video — next.** Answer `launch`/`resume` and the RTSP handshake; read Annex-B from
   the fake 9805, packetise into RTP with the Moonlight video header, add RS-FEC parity, send on
   47998; a picture appears in `moonlight-qt`.
3. **Control + input — next.** Bring up ENet on 47999, decode controller packets, map them through
   the existing pad state into `PPAD` on 9806; the fake sink prints them when the client's gamepad
   moves.

Wiring to the real payload is a later request, once oops-apps ships the `PCTL` end and the encoder
path emits frames. Audio is deferred exactly as it is in part three — Moonlight requires **Opus** at
48 kHz, there is no pure-Rust Opus encoder, and that is the one piece likely to need an FFI
dependency; the workspace's unsafe gate decides it *then*, not now.

### House rules that bind this

Behind a `bin/prosperous` / `pros` verb, not a script (principle 3). Pure-Rust dependencies where
they exist — rustls, aes-gcm, reed-solomon-erasure, an mDNS crate, a pure-Rust ENet port — each
justified in the manifest the way `pros-link` justifies its own (principle 4), and the workspace's
unsafe gate decides the rest: if the gate makes a piece impossible, that piece is refused *by name*
rather than the gate weakened.

---

## Open questions, for the hardware to answer

- **Is the hardware's encoder reachable from an unsigned payload? - Answered (2026-09-01): yes.**
  obSCEne's `106-encoder` loaded the encoder and recording sysmodules (`VENC` `0xa0`, `VIDEOREC`
  `0x81`) with `0x0` from an unsigned payload. The `sceVencCore*` symbols do not auto-bind, but the
  module loads, so they are reached by the obscene#D277 export-table walk rather than a bound import table.
  See "What has to be true for any of it" above.
- **Is the display buffer reachable at all from an unsigned payload, and in what colour
  space?** If it is not, this design is worth nothing and the answer is worth having early.
- **Does grabbing perturb what is being measured?** A grab that stalls the display pipeline
  changes the thing under test, and a diffing harness built on it would be measuring its own
  interference.

Each of these is a reason this is designed now and built later: **the design costs an
afternoon and the answers cost hardware time**, and doing them in that order means the
the hardware's time is spent on questions rather than on discovering which questions to ask.
