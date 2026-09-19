# D032 - A SELF container is verified present, not by size, because the target unwraps it


**decided** · 2026-09-19 · principle 6 · refines D031

D031 had `upload` read each stored file's size back and compare it to the bytes sent, so a store
the server acked but did not keep is caught. That is right for a plain file and wrong for a SELF
container. On a jailbroken console the kernel VFS hook unwraps a fake-signed SELF on access, so
`SIZE` reports the *decrypted ELF payload* - a legitimately different, usually larger number than
the container that was sent. Comparing the two condemned every correct SELF deploy: a restore of
`MESA00001` reported "sent 136256 bytes but the target reports 168560 afterwards - it was not
replaced" and "this copy is incomplete" about a file that was byte-for-byte the intended one
(oops-mesa REQ-20260917T1500Z-3e57). Worse than noise: it taught the operator to distrust a
working deploy and led to hand-pushing raw unwrapped ELFs over `eboot.bin`, which is going around
SELFish - the thing `AGENTS.md` forbids.

So the check now splits on the sent file's first four bytes:

- **A SELF container** - `selfish_abi::Generation::from_container_magic` says so - is verified by
  **presence**: a size came back, so a file is there. Its value is not compared, because the value
  on the target is the unwrapped payload's, not the container's. This still catches a store that
  landed *nothing* (no file, or a swallowed write leaving nothing), which is the failure D031
  exists for; it cannot catch a same-name file left in place, and that is the accepted cost of the
  target transforming the bytes.
- **A plain file** is size-checked exactly, as D031 wrote it.

**Why not compute the unwrapped size and compare that.** A first fix tried it: a hand-rolled
reader that walked the SELF header and the embedded ELF's program table to sum the payload size.
It was wrong - the kernel presents the whole decrypted file, not the max segment end, so the
number never matched - and it was a reimplementation of the SELF+ELF layout inside `transfer.rs`,
which is the format-reinvention principle 6 exists to refuse. The layout is SELFish's; all this
side needs is *is this a container the target will unwrap*, which is a four-byte magic question
SELFish already answers. `selfish-abi` (zero dependencies) is taken directly for it, and the
guessing parser is gone.

The measurement that the target unwraps on access stays here, in Prosperous, where a fact about a
running target belongs (principle 6); the format recognition is SELFish's. D031's size read and
its fake `swallows_stores` mode are unchanged - this only stops the comparison firing on a file
whose size the target was always going to change.
