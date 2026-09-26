# D008 - Frame grab protocol

**Status:** decided
**Date:** 2026-09-26

The frame grabber is a resident service on its own port (9022) that answers `GRAB\n` with a
self-describing header, the pixels and an FNV-1a checksum. Format and stride pass through as the
target reports them; a non-zero status means no pixels follow; a short read is an error.

**Why:** diffing needs repeated grabs, and a resident listener is what every other service in the
chain already is. A frame whose stride was guessed fails as an emulator defect. The threat is a
truncated transfer on a local network, which a non-cryptographic checksum covers in a few lines of
freestanding C.

**Rejected:**
- A one-shot payload writing back over the loader's socket: that socket exists only when the
  loader started it, and each grab would need a re-send.
- A cryptographic digest: guards against substitution, which is not the threat here.
