# Video

Two problems look alike and share no code. **Diffing** is a machine deciding whether one
frame matches another to the byte. **Watching** is a person seeing what the target is doing.
A lossy codec suits the second and destroys the first, so each has its own path:

| | Path | Port | Client |
|---|---|---|---|
| diffing | the frame grabber | 9022 | `pros-link::frames` |
| watching | Porthole | 9805 video, 9806 input | `pros-core::watch`, `pros-link::{pad,pads,feed}` |
| watching, any device | the Moonlight bridge | 47984-48010 | `pros-moonlight` |

The target-side payloads live outside this repository. This document specifies the wire
between them and Prosperous.

## Diffing

A framebuffer diff is the cheapest mechanical correctness signal an emulator has, and the
sibling projects resolve GPU differences down to one unit in the last place. Through a codec
two frames that differ in one pixel can encode identically, and two identical frames encode
differently depending on what preceded them. Diffing therefore reads raw frames.

### The grabber

The grabber is a resident payload with its own listening socket, like every other service on
the target (D008). It is sent once and asked many times, so a before-and-after pair needs no
reload, and it does not depend on the loader's socket duplication.

Its port is 9022, chosen next to the loader and clear of every port the chain uses. `grab`
takes the port as a parameter, so a registration can override it.

### Request

```
GRAB\n
```

The request is a line of text so it can be typed by hand over a raw socket.

### Response

A 32-byte header, little-endian:

| Offset | Size | Field | Meaning |
|---|---|---|---|
| 0 | 4 | magic | `PFRM` |
| 4 | 2 | version | 1 |
| 6 | 2 | status | 0 succeeded; anything else says why not |
| 8 | 4 | width | pixels |
| 12 | 4 | height | pixels |
| 16 | 4 | format | as the platform reports it, untranslated |
| 20 | 4 | stride | bytes per row |
| 24 | 8 | bytes | how many pixel bytes follow |

Then exactly `bytes` bytes of pixels, then a 4-byte little-endian checksum of them.

The client enforces four rules:

- **A non-zero status means no pixels follow.** It is an error (`NotAFrame::Refused`), never
  an empty frame, so a failed grab and a black one never look the same.
- **`stride` times `height` equals `bytes`.** A header that disagrees with itself is refused.
- **`bytes` is authoritative.** A short read is an error, never a smaller frame.
- **Format and stride are passed through.** The client never interprets or guesses them. A
  payload that cannot determine the format reports a status and sends nothing.

### Checksum

The checksum is 32-bit FNV-1a over the pixel bytes (basis `0x811c9dc5`, prime `0x01000193`).
It guards against truncation and corruption on a local network, not substitution, and it is
a few lines of freestanding C. The payload manifest uses a cryptographic digest instead,
because it guards code about to run.

### Client

```rust
pub fn grab(address: &str, port: u16, patience: Duration) -> Result<Frame, NotAFrame>;
pub fn read_frame(source: &mut impl Read) -> Result<Frame, NotAFrame>;
pub fn differences(left: &Frame, right: &Frame) -> Result<usize, Mismatch>;
```

`differences` counts the bytes that differ. Frames of different geometry or format are a
`Mismatch`, never a resize or a partial compare.

At 1920x1080 and four bytes per pixel a frame is 8.3 MB; at 3840x2160 it is 33 MB.

## Porthole

**Porthole** is a first-party stream and input path: a payload on the target encodes its
output in hardware and reads controller state, over two plain sockets. Both ends are ours, so
the format is a decision rather than a protocol to recover, and watching a target needs no
vendor protocol, pairing or account.

```
target                                   this machine
------                                   ------------
capture the composited output
encode it in hardware
9805  ---- H.264, Annex B -------------> pros-core::watch: count it, pipe it to a player
9806  <--- PPAD records ---------------- pros-link::feed: keyboard or pad state
```

The target is the server on both ports. There is no negotiation, no pairing and no
encryption: the same posture as every other service on the target, on a trusted wired LAN.
The transport is TCP, so a lost packet stalls the stream rather than degrading it.

### Video

The payload writes H.264 exactly as the encoder emits it: Annex B, each unit preceded by a
`00 00 01` or `00 00 00 01` start code, with no container, length prefix or header. Any media
player reads that directly.

Prosperous never decodes. A decoder would be a large C dependency through FFI in a
workspace that forbids unsafe code. `pros-core::watch` connects to 9805, passes every read
through `pros-link::stream` on its way to the player's standard input, and counts bytes,
units and keyframes. The player is one line in `player.txt` in the target directory:

```
mpv --demuxer=h264 --profile=low-latency --untimed --no-cache -
```

The counts separate the faults a player cannot: nothing arrived, bytes arrived that did not
frame, units arrived with no keyframe, or the stream is fine and the player has gone. A
stream of dependent pictures with no keyframe decodes to nothing and looks exactly like no
stream at all.

`pros-link::stream` reads the unit type from the low five bits of each unit's first byte:
1 is a dependent picture, 5 a keyframe, 6 supplemental, 7 sequence parameters, 8 picture
parameters. A unit is complete only when the next start code arrives, so one split across
reads is held rather than emitted short.

### Input

One fixed 24-byte record per update, little-endian:

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | magic `PPAD` |
| 4 | 2 | version, 1 |
| 6 | 1 | slot, 0-3 |
| 7 | 1 | reserved, zero |
| 8 | 4 | buttons, one bit each |
| 12 | 4 | left x, left y, right x, right y: unsigned, 128 is centre |
| 16 | 2 | left and right trigger pressure |
| 18 | 2 | reserved, zero |
| 20 | 4 | sequence number |

The framing is ours. The button bits and the stick range are the target's own pad structure,
taken from Ghostpad's published measurements ([ACKNOWLEDGEMENTS](../ACKNOWLEDGEMENTS.md)):

- A trigger sets its bit and its pressure byte. The target reads both, and the bit alone
  does not register.
- `0x0002_0000` is unassigned. It produces an unintended Cross press.
- A zeroed record is not a pad at rest: both sticks would be held up and left.
  `Pad::rest()` centres them.

The format rules:

- **Fixed size.** Records go at display rate; a text parser would drop inputs.
- **Absolute state, never deltas.** The newest record supersedes every older one, so a
  receiver behind by three applies the last and drops two. A dropped state is wrong for one
  frame; a dropped delta is wrong forever.
- **The slot is in the record.** Four pads share one socket, and the sequence number is per
  slot. A slot of 4 or above is refused, not clamped.
- **Reserved bytes are zero and checked.** Gyro, touchpad and rumble go there under a new
  version.

A `Sender` owns the slot and sequence of every record it writes. It sends on every update
while the pad is off rest and stays silent once it has sent a pad at rest.

`pros-link::pads` maps input sources onto the four slots. A slot is filled by the keyboard,
a physical controller or nothing, assigned per slot, so unplugging one controller empties its
slot without renumbering the rest. Each slot has its own key map, and a key bound twice is
reported as a conflict rather than silently unbound. `pros-link::feed` sends the records to
9806 and reports idle, sending, lost and refused as distinct states.

## The Moonlight bridge

`pros-moonlight` presents Porthole to any Moonlight client as the GameStream protocol. It is a
second consumer of 9805 and 9806, beside `watch`, and changes nothing on the target: the
protocol runs on this machine, where a Rust TLS stack is available, rather than in a
freestanding payload. `pros moonlight` runs it with one app per registered target.

```
target                  this machine (pros moonlight)               Moonlight client
------                  -----------------------------               ----------------
9805 --H.264 Annex B--> split into frames, RTP, Reed-Solomon  --UDP 47998-->  video
9806 <--PPAD----------- decrypt, translate to PPAD            <--ENet 47999--  gamepad
                        mDNS _nvstream._tcp, HTTP 47989, HTTPS 47984, RTSP 48010
```

The client leg:

1. **Discovery.** mDNS advertises `_nvstream._tcp`, and `serverinfo` answers on HTTP 47989
   and HTTPS 47984.
2. **Pairing.** The four-phase PIN handshake: SHA-256 salted key, AES-128-ECB challenges,
   RSA-signed commitments. The self-signed certificate is generated once and kept, so a
   client's pinning survives a restart.
3. **Session.** `launch` and `resume` record the mode and keys and return the RTSP URL. RTSP
   on 48010 runs OPTIONS, DESCRIBE, SETUP, ANNOUNCE and PLAY. The description advertises
   H.264 video only.
4. **Video.** On PLAY the bridge reads Annex B from 9805, groups units into frames, and
   packetises each into RTP with the NV video header and Reed-Solomon parity shards, sent to
   UDP 47998.
5. **Control.** An ENet channel on 47999 carries AES-128-GCM control messages. A controller
   update (type `0x0206`) becomes a `PPAD` record sent to 9806. Moonlight's XInput-style
   button bits are translated bit by bit, and its signed 16-bit axes are rescaled to
   unsigned bytes centred on 128, with the vertical axes inverted.

### Security of the two legs

| | Target to bridge (9805, 9806) | Bridge to client (47984-48010) |
|---|---|---|
| transport | TCP | UDP for video, ENet for control |
| loss tolerance | none; TCP stalls | Reed-Solomon forward error correction |
| authentication | none; trusted LAN | PIN pairing, per-client certificate |
| encryption | none | AES-GCM after RTSP negotiation |

The target leg is plaintext on the trusted LAN, as Porthole is everywhere. The client leg is
paired, encrypted and loss-tolerant because Moonlight clients require it. The bridge is the
seam between the two.

### No decoding

The bridge never decodes, re-encodes or transcodes a frame.
`nal.rs` groups units into frames using `pros-link::stream`, the same reader `watch` counts
with, so keyframe detection has one owner. Parameter sets travel in the frame of the
keyframe that follows them.

### Provenance and licence

The GameStream protocol has no published specification. It is defined by the behaviour of
moonlight-common-c, Sunshine and Wolf, all GPLv3; Prosperous implements that behaviour and
copies and links none of their code. The bridge's pairing crate choices, RTSP flow, RTP and NV
packet layout, FEC scheme and AES-GCM control channel are adapted from Moonshine
(BSD-2-Clause). Its notice is in [THIRD-PARTY-LICENSES.md](../THIRD-PARTY-LICENSES.md), and
each source file that closely follows a Moonshine file says so.

### Testing on one machine

`pros fake-target` stands in for Porthole: it loops an Annex B clip on 9805 and prints each
`PPAD` record it receives on 9806. With it, a stock Moonlight client on the same LAN drives the
whole bridge, and no target is involved.
