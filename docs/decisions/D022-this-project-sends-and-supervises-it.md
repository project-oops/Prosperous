# D022 - This project sends and supervises; it does not drive


A probe on real hardware answers questions by calling functions whose arity nobody has
established, so faulting is its normal case rather than its exceptional one. Its protocol says
plainly that restarting afterwards is out of scope, and names the restarter as *"a person on a
console"*.

`pros supervise` is that person. It watches the serving port, re-sends the same bytes through
the loader when nothing answers, refuses to send while the probe is alive, and gives up after
three consecutive dead starts. Faults with a working session between them are unlimited,
because that is the useful case and a cap on the total would stop it rather than the broken
one.

That belongs here for the same reason the transport does: **this is already the project that
reaches a target.** A supervisor and the sender being one program is what makes the restart
unattended, and no other project is both.

**Driving the protocol does not belong here, and a client for it was written and removed.**

The reasoning is worth keeping because the mistake was reasonable. The probe's specification
is written to be copied out and used by a consumer that does not have that repository checked
out, and its captured transcripts are published as a fixture. That is a genuine invitation, and
a client built against it passed every transcript.

It was still wrong. The invitation explains why a second client is *permitted*; nothing
explains why one is *wanted*. The probe already has a driver, that driver already reaches
consoles through this project's transport, and the questions come from a third project
entirely. A second implementation would be a third place that could disagree about what `died`
means - and the whole reason that record exists is that a command which did not answer must
never be recorded as having answered.

The division that survives: **orbistoun asks, obSCEne answers, this puts the instrument on the
metal and keeps it there.**

