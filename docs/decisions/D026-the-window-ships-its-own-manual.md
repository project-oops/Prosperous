# D026 - The window ships its own manual


**decided** · 2026-08-30

`help -> documentation...` opens a reader over pages embedded in the binary by `include_str!`,
via the shared `oops-docs`. Because they ride in the executable they are always accurate to the
build somebody is running: there is no version to keep in step and nothing to fetch, which
matters for a tool whose job is talking to hardware on a network that is often switched off.

Three pages: targets, payloads, and the target's storage. They are the *manual* - `DECISIONS.md`
and `WORKLOG.md` stay in the repository, where they are for whoever changes this, not for whoever
uses it.

The registry lives here rather than in the shared crate because `include_str!` resolves relative
to the file it is written in, so no other crate can embed these. That constraint puts the line in
the right place anyway: the reader is shared, the list of pages is not.
