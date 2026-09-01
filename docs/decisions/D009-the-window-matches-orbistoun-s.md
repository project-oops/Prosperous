# D009 - The window matches orbistoun's, deliberately, down to the version


**decided** - 2026-08-26 - building `pros-gui`

`eframe` 0.29, default features off, `wgpu` backend, `egui` 0.29. **The same versions and
the same backend as the sibling project's shell**, chosen by looking at its manifest rather
than by picking what was current.

The reason is a thing worth having later: the two windows do the same kind of work over the
same library, and a shared GUI crate is a sensible eventual move. It is only a *move* if the
two are already the same underneath; otherwise it is a port, and a port is a project.

wgpu earns its place here on its own terms too. A lossless frame grab has to be displayed
somewhere, and that is this repository's own future need rather than a borrowed one.

### What is tested, and what could not be

A window cannot be looked at from a test, so the crate is split so that most of it is not a
window:

- `state.rs` - the rules. One job at a time, because two racing would interleave into a
  window showing half of each. **A failed job clears the panel it would have replaced**,
  because a report left on screen after a refresh that failed is a claim about a target
  that has stopped being true - the caching mistake, made in pixels. An answer arriving when
  nothing was asked is dropped.
- `work.rs` - the asking, on its own thread. A check is five ports at a second and a half;
  done on the drawing thread the window stops repainting, and **a window that has stopped
  repainting is indistinguishable from one that has crashed**.
- `app.rs` - drawing only. The one thing tested is the **wording**, which must match what
  `pros` prints for the same finding. Two front ends over one library that word a verdict
  differently is how a shared library quietly stops being shared.

### It was run, and that changed something

The claim in the last entry was that a window could not be verified from here. That was
wrong: the operating system will say whether a process owns a window and whether it is
answering, and the screen can be captured and looked at.

Doing so found a real defect that no test would have. The build stamp read
`built 1787734934` - seconds since the epoch, chosen deliberately because it needs no
dependency and no locale, and entirely correct. **It is also unreadable, and a stamp exists
to be read at a glance.** It is now computed to `YYYY-MM-DD HH:MM UTC` in the build script,
in twenty lines of the standard era-based conversion, still with no dependency. Always UTC
and it says so, because a stamp in local time is ambiguous the moment somebody elsewhere
pastes it into a report.

The stamp is assembled in one place and used by both front ends, so `pros --version` and the
window's footer cannot disagree about which binary somebody is looking at.

**The rule this is the second instance of today:** when something can be looked at, look at
it. Reasoning about the fake's accept loop said it was fine; measuring said 487 ms. Reasoning
about the stamp said it was correct; looking at it said nobody could read it.

