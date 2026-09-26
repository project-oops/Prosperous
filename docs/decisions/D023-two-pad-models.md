# D023 - Two pad models

**Status:** decided
**Date:** 2026-09-26

`pros-link`'s `Pad` is the target's layout: unsigned bytes centred on 128 and one bit per button.
Orbistoun's `PadState` stays host-shaped floats. Only measured button positions are a candidate for
sharing; the encoding, the keyboard mapping and tap-versus-hold are not.

**Why:** matching the measured layout means the payload does no arithmetic, which is arithmetic
that cannot be wrong where nobody looks. An emulator decides what a long press means; Prosperous
forwards absolute state and the target decides.

**Rejected:**
- One shared model: one of the two projects then converts at the wrong layer.
- Tap-versus-hold here: duplicates the target's own logic where the result cannot be seen.
