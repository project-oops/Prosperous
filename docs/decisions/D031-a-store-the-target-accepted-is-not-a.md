# D031 - A store the target accepted is not a file copied until its size reads back


**decided** · 2026-09-17 · principle 5

`transfer::upload` counted a file the moment `STOR` returned a completion code. That is not the
same as the bytes landing. A target with the title mounted, or an overlay that swallows the
write, answers `226` and leaves the old file in place - so a restore reported *7 files* and a
console kept an `eboot.bin` at its previous size, and the person restoring chased the wrong thing
for a morning because the tool had told them the copy succeeded.

So an upload now **reads the size back after each store** (`Session::size`, `SIZE`) and compares
it to what was sent. A match counts the file; a mismatch, or a size the target will not report,
is recorded in the summary as *not copied*, with what was sent and what came back. Nothing else
changes: `say::copied` and the window already fail on an incomplete summary, so the effect is
that a restore which did not replace a file now exits non-zero and names the file, instead of
printing success.

This is principle 5 - *honest failure over plausible output* - applied to the one gap the
transport had left: `pros-link`'s [`crate::fake`] note has always said "a store that worked and a
store that reported success are different things, and only the contents afterwards tell them
apart", and the fake carries a `swallows_stores` mode for exactly this fault; the real path was
trusting the reply the fake warns about. Size, not a re-download, because the reported failure is
a size that differs and a re-read of a large `eboot` on every file is a cost a restore should not
pay to catch it; a same-size corruption is a rarer fault for a future check, not this one.

`Session::size` lands in `pros-link` rather than `pros-core` because it is one line of the
protocol - `SIZE`, a `213` reply - and reading a target's own answer is what that crate is for
(principle 4 is about not growing a *runtime*, not about withholding a verb the transport
already implies).
