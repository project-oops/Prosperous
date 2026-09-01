# D008 - The frame grabber is a resident payload with its own port, and its protocol is written down before it is built


**decided** - 2026-08-26 - `docs/VIDEO.md`

The brief asked for this to be designed and deferred. Design without a protocol is a
paragraph of intent, so here is what was actually settled. Everything in `docs/VIDEO.md` is
**chosen**, not measured, and it is marked as such there.

### Resident, not one-shot

The obvious build is a payload the loader sends, which grabs a frame and writes it back over
the socket the loader duplicated onto its output. It is wrong twice. It rests on that socket
duplication, which is a convenience and never a mechanism - a payload started any other way
does not have one. And diffing needs *repeated* grabs, so a one-shot means re-sending a
payload between every pair of frames.

A resident payload with its own listening socket is what every other service in the chain
already is.

### Self-describing, and refusing rather than guessing

The response header carries width, height, format and stride, and **the format is passed
through as the platform reports it** rather than translated. A diff against a frame whose
stride was guessed fails as *the emulator is wrong* rather than as *the client guessed*, and
that is a day spent on the wrong question.

Three rules follow from the same instinct: a non-zero status means no pixels follow, so *it
did not work* and *it worked and produced nothing* cannot look alike; the byte count is
authoritative and a short read is an error, because a truncated frame diffs perfectly well
and says nothing true; and a payload that cannot determine the format sends a status instead
of labelling pixels with a guess.

### A non-cryptographic checksum, deliberately

The threat is a truncated transfer on a local network, not an adversary substituting a
frame, so FNV-1a over the pixel bytes is the right size of answer and is six lines in
freestanding C.

**This is the opposite call from the payload manifest**, where the digest guards a download
about to be executed with kernel-adjacent privileges. Different threat, different answer,
and both are written down so that neither gets changed to match the other by somebody
tidying.

### A port number that is a choice

9022: adjacent to the loader, and outside every port the chain used as measured on
2026-08-25. If it collides with something, that is a one-line change and a note saying what
it collided with - which is the whole reason for saying out loud that it was chosen rather
than found.

### Why write acceptance criteria for something not built

Five of them, in `docs/VIDEO.md`, and the first is *two grabs of a static scene are
byte-identical*. **The measurement instrument gets measured before it is used.** A diffing
harness that has not been diffed against itself will produce numbers, and those numbers will
be believed.

