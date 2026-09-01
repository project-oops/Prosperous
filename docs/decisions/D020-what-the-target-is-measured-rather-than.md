# D020 - What the target is, measured rather than looked up


Firmware version decides nearly everything else on this platform: which jailbreak works, which
payloads run, whether a game needs backporting. It was the one fact this tool could not answer
and everybody had to find elsewhere.

The shell answers `sysctl`, `df` and `ps`. Each was run against a target and the parsers were
written from **what came back**, not from a manual page for a different system with commands of
the same name. `sysctl` prints a hex dump rather than a value, so a fact is reassembled from
the bytes - not from the dump's own rendering column, which substitutes a dot for anything
unprintable and cannot be un-substituted afterwards.

Two keys are absent from the list on purpose: `machdep.idle` and `hw.physmem` answered *no such
file or directory*, so asking for them would show a permanent gap that looks like a fault.

**The measurement corrected the design twice.** `df` returned 1183 filesystems, which read as a
parser bug and was not: 22 are the machine and 1161 are bind mounts inside running
applications' sandboxes. They are counted and shown behind a fold rather than discarded -
they are real - but listing all 1183 flat would bury the figures somebody came to read. And
`ps` marks a title only for processes that have one, so the column is blank for most rows;
finding it by counting across would have put a memory figure where an identifier goes.

The target test asserts shapes and prints only field lengths. Firmware strings, model numbers
and account identifiers are facts about somebody's target, and a test that pinned them would
fail on everybody else's.


