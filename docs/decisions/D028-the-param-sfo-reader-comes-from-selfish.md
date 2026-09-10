# D028 - The `param.sfo` reader comes from SELFish; the in-place writer stays


**decided** · 2026-09-10 · principle 6

`pros-core::sfo` carried its own `\0PSF` parser - a header read at offset 20, sixteen-byte index
entries, a `Value` enum, `read`. `param.sfo` is a platform format, and principle 6 says
Prosperous reads the platform's formats from SELFish and invents none of its own. A second copy
of a layout table is the exact thing that principle exists to prevent, so the parser is the one
piece here that should not have been here.

It was filed as a cross-project request (Prosperous's inbox `REQ-...f7ad`) and SELFish delivered
`selfish_title::sfo::Sfo` with a `bytes(key)` accessor - the one thing missing for reading
`ACCOUNT_ID` as its raw eight bytes rather than as text it happens to resemble. So the reader is
gone from here, and the three consumers - `origin` and `saves` reading an account, `graft`
reading a save's parameters - go through `Sfo` now. What Prosperous *adds* stays: `account_in`
and `account_id` render the id as hex (the shape it is compared in, never a number), over
SELFish's read.

**The writer does not move with the reader.** `sfo::set` edits one parameter **in place** -
nothing is rebuilt, a value is written over the old one within the room the file already left,
and everything it does not understand is untouched by construction. That is a deliberate
opposite of `Sfo::to_bytes`, which re-serialises the whole file; `graft::set_account` depends on
the in-place behaviour, because a save this project reassembled rather than edited would still
parse and be refused only later, by a target, with nothing pointing at the wrong byte. So the
writer is Prosperous's own concern layered over SELFish's reader, and the module note says so.

**The cost, stated:** `pros-core` now depends on the `selfish` sibling repository by path, the
way it already depends on the `oops-*` crates - so a standalone clone of Prosperous alone no
longer builds, the collection is checked out as a set. No new heavy dependency: `selfish-title`
brings `serde`/`serde_json`, which `pros-core` already carries for the manifest.

**Not a bug fix.** SELFish's resolution surfaced two defects in *its* parser - a whole-file
failure on a non-UTF-8 `ACCOUNT_ID`, and trailing zeroes trimmed off the unterminated format.
Prosperous's own parser was immune to both, because it bucketed anything that was not `0x0204`
text or `0x0404` number as raw bytes, and read `ACCOUNT_ID` through a single accessor. So this
removed a correct duplicate, not a wrong one; the value is one home for the format, not a repair.
