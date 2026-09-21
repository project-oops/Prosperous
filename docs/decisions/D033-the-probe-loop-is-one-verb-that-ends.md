# D033 - The probe loop is one verb, in the CLI, that ends on exit or a cap


**decided** · 2026-09-21 · principle 3

Deploying a homebrew probe, running it, and reading its output was three commands across two
windows - a `restore`, a `launch`, and a `logs` in a second terminal - run dozens of times a
session. `pros probe <id> <build-dir>` is the one command: close the title if it is running,
restore the build dir into `/data/homebrew/<id>` overwriting what is there, launch it, and follow
its log until it ends.

**It lives in the CLI, like `logs`, not in `pros-core`.** Principle 3 keeps behaviour out of the
shims, and this bends it the same way `logs` already does: every *piece* is in `pros-core` and
tested there - `transfer::upload`, `launch::command`/`read`, `system::of_title`, the new
`guard::homebrew_path` - and what is in the shim is only the interactive tie-together: a
background `ps` poll on shsrv while the foreground drains klogsrv to stdout, two connections that
do not contend. That is a stream-and-poll loop, not a builder, and a builder is the only shape
`pros-core` holds. The GUI has the pieces as separate actions (restore, launch, the log tail) and
does **not** get the one-shot loop yet - the same call `supervise`, `moonlight` and `fake-target`
already make, where a long-running interactive orchestration is a command-line verb first. A GUI
"deploy and watch" is a later request, and `guard::homebrew_path` is shared so it can reuse the
destination.

**The launch waits for the title to be registered, not just restored.** A restore lands the tree
under `/data/homebrew/<id>` at once, but ShadowMountPlus has to mount it and the shell has to
register it before it appears in the appmeta list (`/user/appmeta`, the one `pros titles` reads)
and before `launch` will resolve it. The hand-run recipe never noticed, because typing the second
command gave registration a few seconds; the verb runs the steps back to back and would launch
into that gap. So after the restore it polls the appmeta list for the id - the very thing the
launch resolves against, not the filesystem, which says yes immediately - up to a minute, and
refuses to launch (saying the title restored but never registered) rather than firing a launch
that fails.

**The follower is attached before the launch, not after.** The first cut launched and then opened
the log stream, and lost everything for the titles this is for: a Mesa probe does its whole job in
the first second or two and then parks silently, so all of its output landed in the gap between
the launch returning and the stream opening - the run succeeded and its capture was empty, every
time (measured, oops-mesa, 2026-09-21). The connection is the subscription: `log::follow` returns
once the klogsrv socket is open, and everything klogsrv emits after that is buffered until it is
read, so following first and launching second is the fix. A short settle after the follow is
insurance on top of the ordering, not the mechanism - a margin so the stream is certainly live
before a launch whose output arrives and parks within a second. This is also what makes the park
sentinel (below) usable: the line a finished probe prints is caught rather than lost in the same
gap.

**The watch ends on the park sentinel, the process leaving the list, or a cap - because a finished
probe parks.** A big-app cannot return from its entry point, so its conforming ending is to print
a last line and idle (oops-sdk's park). Such a title never leaves the process list, so "return
when it exits" cannot be the only rule or the verb would wait forever. So: oops-sdk prints a
sentinel immediately before it parks, and the follow ends at once when it sees that - a run that
is over does not wait out its cap. Failing the sentinel, the watcher waits for the title to
*appear*, then stops the moment it *vanishes* (an exit or a crash); failing that, the `--seconds`
cap ends the follow and says the title is still running - probably parked. A title never seen by
the cap is reported as a third case (exited instantly, or did not start), not conflated with
parking.

**The close is best-effort, and the verb says so.** A parked big-app ignores every signal
(oops-mesa's b1e4), so the close clears a killable process and no more; if a previous run is still
parked and holding the slot, the launch reports the slot unavailable rather than the verb
pretending the close worked. A restore that does not land cleanly stops the loop before the
launch, so a half-deployed title is never run.
