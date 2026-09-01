# D003 - The fake blocks on accept and knocks to stop, because polling was measured and was terrible


**decided** - 2026-08-26 - after two tests failed intermittently and both blamed the client

Two tests in `against_a_fake` failed about a third of the time, in different combinations,
single-threaded as well as parallel. Both were of the form *the fake wrote something and the
client did not see it*, and both looked like a client that gave up too early.

### What the measurement said

A plain blocking `accept` in a thread, with no fake anywhere near it, delivered its first byte
**110 microseconds** after a client connected. The fake, doing the same thing, took between
**2 milliseconds and 487 milliseconds** - bimodal, mostly at the slow end.

So it was never the client, and never the machine.

### What was wrong

The listener was non-blocking and the accept loop polled it, sleeping between attempts. That
shape exists for one reason: a blocking accept cannot be interrupted, so stopping the fake
needs a way in. Polling made stopping easy and made **answering** unreliable.

Two separate costs came out of it:

1. `Fake::start` returned before its thread had been scheduled at all - measured at **230 ms**
   on this machine. The socket is bound by then, so a client connects successfully and waits
   in the backlog. **A fake that has not started is indistinguishable from a fake that is
   silent**, which is the defect this project keeps meeting, this time in the tool used to
   test for it.
2. Even once running, the poll loop took hundreds of milliseconds to notice a connection that
   was already there.

### What it is now

- **The listener blocks.** That is the fast path and it is now the only path.
- **`Drop` knocks**: it sets a flag and then opens one throwaway connection to its own port,
  which wakes the accept. The loop checks the flag immediately after accepting, so the knock
  is never answered as if it were a client.
- **`start` waits for a signal sent from inside the serving thread** before returning, so
  *started* means *serving*, not *spawned*.

Measured again afterwards: **111 microseconds**, indistinguishable from the plain listener.

### Why this is worth an entry rather than a commit message

The fake is shipped, not test scaffolding - two other projects will build their own tests on
it. A stand-in that answers a quarter of a second late teaches every one of those tests to
carry a generous timeout, and generous timeouts hide the very failures a transport crate
exists to catch. It also would have been extremely easy to "fix" this by widening the windows
in the two failing tests, which would have left every future test slow and the cause intact.

The rule it came from: **when a test is flaky, measure the thing it is testing against before
touching the test.**

