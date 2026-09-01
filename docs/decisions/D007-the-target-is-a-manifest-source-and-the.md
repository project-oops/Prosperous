# D007 - The target is a manifest source, and the path is asked for rather than assumed


**decided** - 2026-08-26 - adding `pros manifest --from-target`

The payload manager keeps its own repository description, and this project copied that schema
rather than inventing one precisely so that **a target which is already configured is already
described**. Reading it is one command now.

It also does something no other command here can: it is the only way this project will find
out what that document actually looks like. Its field names are known; whether it is a list,
an object keyed by name, or a wrapper around either is not, and neither is the format of its
`checksum` field. Both open questions in D004 are answered by running this once against a real
target.

### The path is a parameter, not a default

`/data/pldmgr/repository_cache.json` is a good guess - the manager's autoload list is in that
directory - and a guess is what it would remain. A default carries the authority of a
measurement without having made one, and when it turned out to be wrong the failure would be
*this target has no repository* rather than *this program looked in the wrong place*.

So it is asked for. When somebody runs it against real target and it works, the path becomes
a default **with a measurement behind it**, and this entry is where that change gets its
justification.

### It reads over the file service rather than the dashboard

The manager also answers on a web port, and that would be the obvious route. It is the worse
one: the endpoint paths that service answers on have not been measured either, so that version
would be a guess wrapped around a guess. The file service needs no endpoint - a file has a
path, and the path is the thing being asked for anyway.

### What the test proves, and what it deliberately does not

A stand-in serves a manifest in *this project's* shape, and the command reads it, reports it,
and judges its entries exactly as it judges one read off a disk. That proves the plumbing.

**It proves nothing about the real file**, and the wording of the test says so, because a test
whose name suggests otherwise would be the same kind of claim this entry exists to avoid.

