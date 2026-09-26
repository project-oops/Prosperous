# D016 - The manifest format is one tool's file, plus `port`

**Status:** decided
**Date:** 2026-09-26

The payload manifest copies the fields of the repository cache a payload manager keeps on the
target, and adds one: `port`, which makes a payload's presence measurable. A port is never read out
of a description, a merge with the target's repository keeps it, and the schema is tested against
the type.

**Why:** interoperating with the file already on people's targets is worth more than a cleaner
format. It is one tool's cache, not a standard, and is not cited as one. A declared port turns an
unknown row into a measurement by editing a file rather than releasing a binary.

**Rejected:**
- A format of this project's own: nobody else can read it.
- Parsing ports out of prose: a wrong guess reports one listener's state under another name.
- Letting the target's repository overwrite `port`: an absence is not a correction.
