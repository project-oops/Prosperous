# Acknowledgements

The sources Prosperous consulted, and what each was consulted for. Code derived from a source
carries that source's licence notice in [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md).
Payloads are used unmodified and fetched from their own distribution; Prosperous ships none
of their binaries.

## The payload chain

| Source | Consulted for |
|---|---|
| `ps5-payload-dev` payloads (`elfldr`, `ftpsrv`, `klogsrv`, `shsrv`), GPL-3.0 | the services this tool is a client for, and their protocols |
| `pldmgr` | the payload manager and its dashboard; its `repository_cache.json` schema is the manifest format, adopted unchanged |
| `ShadowMountPlus`, `ps5upload`, `nanodns`, `kstuff-lite` | the rest of the chain a prepared target runs |
| `zftpd`, `ftpsrv-drakmor`, `etaHEN` | alternative providers of the same capabilities, which shape the capability model |

## Remote play

| Source | Consulted for |
|---|---|
| **Chiaki / chiaki-ng** (AGPL) | the shape and cost of the vendor's remote-play protocol: pairing, UDP transport, per-session encryption, forward error correction, codecs. Porthole exists so that none of it is needed |
| `linkdev` (`ps5-payload-dev`) | remote-play registration without a vendor account |

## The GameStream protocol

The protocol has no published specification. Prosperous implements the behaviour these
projects define and copies or links none of their code:

| Source | Consulted for |
|---|---|
| **moonlight-common-c** (Moonlight), GPLv3 | the client core every Moonlight client speaks |
| **Sunshine** (LizardByte), GPLv3 | the reference host: what a host must send |
| **Wolf** (games-on-whales), GPLv3 | an independent second host: what the protocol requires, as against what one host does |

**Moonshine** (Hans Gaiser, BSD-2-Clause) is the base of `crates/pros-moonlight`: its pairing
crate choices, and the RTSP handshake, RTP and NV video packet layout, Reed-Solomon FEC scheme
and AES-GCM control channel of the streaming half. This is derivation, so its notice is
reproduced in [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md), and each source file that
closely follows a Moonshine file says so.

## Ghostpad, for the controller layout

`crates/pros-link/src/pad.rs` uses the button bitmap and stick range Ghostpad confirmed
empirically against a real target, as recorded in its `virtualDS5research.md`. Ghostpad in
turn credits `shadPS4`'s `pad.h` for the underlying enum. From it:

- the bit for each button;
- sticks are unsigned bytes centred on 128;
- a trigger sets both its pressure byte and its digital bit;
- `0x0002_0000` produces an unintended Cross press and stays unassigned.

These are facts about the target's structure, not code. Ghostpad's payload is
GPL-3.0-or-later and none of it is used, nor its technique of patching a system library at
run time.

## Orbistoun, for input handling

Orbistoun's `orbistoun-input` controller model was consulted for three patterns Prosperous
follows:

- A key reads as down if it is held or was pressed this frame, so a tap inside one frame is
  not lost.
- A key bound to two things is reported as a conflict, not resolved by unbinding one.
- Each pad slot keeps its own key map.

Its tap-against-hold `ShellButton` is not followed: Prosperous forwards absolute state and the
target decides what a hold means. The two projects represent a pad differently on purpose
(D023).

## obSCEne, for the transport

obSCEne's tool was the reference for this project's transport, its registry design and its
`e_type` guard. That tool takes `pros-link` by path.

## References at the point of use

Where a lawful public reference settles a fact, it is cited in the code that uses it.
