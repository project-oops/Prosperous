# D034 - A restore skips a file it already put there unchanged, on a record it keeps itself


**decided** · 2026-09-22 · principles 2 and 6

Restoring a title re-sent every file, every time. On a large title that is minutes of transfer to
leave most of it byte-for-byte as it was - the deploy loop `probe` runs dozens of times a session
rebuilds one `eboot.bin` and pushes the whole tree again. A restore should send what changed and
skip what did not.

**The obvious way - ask the target what it holds and skip what matches - does not work here, and
the reason is D032 turned around.** A skip decision made against the target needs the target's copy
compared to the local one, by hash or by size. But the kernel VFS hook unwraps a fake-signed SELF
on access, so what the file service reports for `eboot.bin`, a `.prx` or a `.sprx` is the decrypted
ELF, not the container that was sent. A digest the server computed would be over bytes this side
never holds, and would disagree for every SELF - which is exactly the large file a title is mostly
made of, so the scheme would re-send the big files and skip only the small ones. Size fails the
same way and for the same reason (D032). Whether `ftpsrv` even exposes a hash command is unmeasured
and beside the point: the unwrap defeats a remote comparison of the files that matter regardless.

**So the record is the one this side can keep truthfully: what this program itself verified
landing.** After a store passes the presence-and-size check (D031, D032), the digest of the bytes
that were *sent* is recorded against the path they went to, in `deployed.json` beside the registry,
keyed by target name over remote path. A later restore hashes each local file and skips it when
that digest is what the record holds for its path **and** the target still reports the file
present. The digest is of the local bytes - the only side that can be hashed without the unwrap in
the way - so it is `pros_core::checksum` doing the hashing, not a second copy of it, and no new
dependency (principle 6: the measurement about a running target stays here; the format work stays
in the crate that owns it).

**The presence half is not optional, and it reads a listing, not a size.** A record is not a
promise the file is still there - a delete, a wipe or a hand-edit can remove it while the local
source is unchanged - so a matching digest alone does not skip; the target must still show the file.
Presence is read from a **directory listing**, not `SIZE`: `SIZE` proved an unreliable existence
signal on a real target - it answered a size for a title that had been deleted by hand, so the skip
wrongly kept and *nothing* re-sent - whereas a listing is the same truth `pros ls` shows and the
name in it does not change when the target unwraps a SELF. It is also cheaper at scale: each folder
is listed once and remembered, so a title costs a listing per folder rather than a `SIZE` round trip
per file, which on a many-file title over the network is the difference between a quick restore and
one that looks hung. A folder that will not list (it was removed) reads as empty, so everything in
it is sent again.

**It is a cache, and it only ever errs toward re-sending.** Nothing is skipped that was not both
recorded from a verified landing and confirmed present. Every verified store updates the record;
every store that does not verify forgets it, so a failed landing is re-sent next time rather than
skipped on a stale note. `restore --all` (and `probe --all`) ignore the record and send
everything - the escape hatch for the one case this cannot see, something other than this program
rewriting a file on the target at a path whose local source has not moved. A fresh target has no
record and sends everything. The one failure a restore must never have is a skipped change, and
every way this can be wrong lands on the safe side of that line.

**Skip-unchanged is the default; the window gets it too.** Re-sending an unchanged tree was the
cost worth removing, so the default is to skip and `--all` forces the full push. `pros-gui`'s
restore skips unchanged files the same way (principle 3 - a capability in only one shim is one that
drifts); the force-all toggle is command-line only for now, which is a smaller gap than the
optimisation being absent from the window entirely. An unchanged file is reported as its own
outcome - neither copied nor skipped - so a restore that moved little because little changed reads
as the success it is, not as a backup that copied nothing.
