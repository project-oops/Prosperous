# D016 - The manifest format is one tool's file, and `port` is ours


**The format was not designed; it was found.** Every field except one was copied from the
`repository_cache.json` a payload manager keeps on the target. This project has seen exactly
one instance of that file, on one target. There is no published schema for it, no
specification, and no second implementation that was examined - so there is no evidence of a
community consensus behind it, and this project does not claim one.

Copying it anyway was right: interoperating with the file that is already on people's targets
is worth more than a cleaner format nobody else can read. The mistake would be forgetting the
provenance and citing it as a standard later, which is why it is written down here and in
`docs/manifest.schema.json`.

**`port` is an addition, and it is the field that makes presence answerable.** Whether a
payload is running is decided by connecting to something. Five services have ports measured
against a target; everything else was unknowable, and *unknowable* is honest but is not
useful when it fills twenty rows. An entry that declares a port makes itself measurable, and
because the list is a file rather than a table in the binary, widening what can be seen is an
edit rather than a release.

Three things follow, and each exists because its absence would fail quietly:

- **A description mentioning a port is not a port.** Reading a number out of somebody's prose
  is guessing at meaning, and the wrong guess reports one listener's state under another
  payload's name - a lie that looks like a measurement. The field is a deliberate line or it
  is nothing.
- **A merge with a target's repository does not erase it.** That file has no `port`, and an
  absence is not a correction. Without that rule, every port anybody wrote down would vanish
  at the next read and the vanishing would look like the repository being authoritative.
- **A dead target is not probed twenty-five times.** If none of the five known services
  answered, the target is not there; further probes spend a full timeout each to learn what
  is already established. Skipping them keeps an offline check at seconds rather than minutes
  as the list grows - the alternative being a wait that looks exactly like a hang.

The schema is checked against the type by a test, because a document that describes code
drifts away from it without anything going red.

