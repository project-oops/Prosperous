# D002 - A target's answer is read on its own terms, and a refusal is not a failure


**decided** - 2026-08-25 - while adding the file service and the manager's web service

The first three services are one exchange each: connect, maybe say something, read until a
clock says stop. The last two are not, and each brought a way of being wrong that the
existing shape could not express.

### The error type grew two variants, and the split is the point

- **`Rejected`** - the target understood and said no. A missing file, a read-only mount,
  a path that is not there. **Nothing is broken.** A caller browsing a filesystem meets
  several of these per session, and one that treats them as the link failing reconnects to
  fix a typo.
- **`Unintelligible`** - the target answered in a shape this crate could not read. The
  remedy is the opposite: `Rejected` is usually the operator's path, this is usually **this
  crate being wrong** about a server it was written against second-hand. It carries what
  was actually said, because that is the only useful thing to put in a report.

Four variants would have been three too few and six would be inventing distinctions. These
two are each a different next action for the person reading them.

### The address a server names for a transfer is discarded

A passive reply carries six numbers: four of address, two of port. **Only the port is
taken.** A small server behind any kind of translation reports the address it believes it
has, which is regularly not the one that reached it - and a client that dials it reaches a
machine on somebody else's network, or nothing.

The host already in hand is reachable **by proof**: there is an open connection on it. This
is the same reasoning as everywhere else in the sibling projects - prefer the thing that has
been demonstrated over the thing that has been asserted.

It is the most important test in the file, because a client that dials the claimed address
works perfectly on a bench and fails in a house.

### Binary mode is a condition of opening a session, not a request made in passing

A transfer in the default text mode rewrites line endings. The transfer still completes, the
byte count still looks right, and the payload at the other end no longer runs, with nothing
anywhere recording that four bytes changed in the middle.

So a server that will not agree to binary **fails the session outright**. There is no
partial-use path, because the useful thing to do with this service is move a payload and
that is exactly what text mode breaks.

### A listing line that was not understood is kept, marked, and refused as a path

The long-form listing is a server's choice, not a standard. A parser that drops what it
cannot read reports a directory as emptier than it is - which is a worse answer than a line
somebody has to look at. So an unreadable line becomes an entry carrying the server's text
verbatim, marked so it cannot be used as a path.

### The web client knows no endpoint paths

Which paths the manager answers on **has not been measured**. A plausible constant would be
a guess wearing the clothes of a fact, which is the failure the sibling projects grade
evidence to avoid. The caller passes the path. When they are measured they belong with the
code that interprets what comes back, one layer up.

Chunked bodies are reassembled rather than refused, for the same reason the shape guard
exists: handing back a body with the piece sizes still in it is success reported for data
that is wrong.

### What the fake taught, which was about the fake

A connection accepted from a non-blocking listener **inherits the non-blocking mode**. The
listener has to be non-blocking so the accept loop can be stopped - so every read in the
fake was returning immediately whether or not anything had arrived, and the loop that
existed to wait for a client to go quiet was not waiting at all.

It passed anyway, because the payloads in the tests are small enough to arrive within the
first few reads. That is the recurring defect exactly: **a mechanism whose "did nothing"
output is identical to its "changed nothing" output.** Recorded here because the fake is
shipped, so this was a defect in a tool two other projects will trust.

