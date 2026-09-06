# D027 - Process control is prosperous work, not obSCEne's


**decided** · 2026-09-04

Restarting the user interface to clear a softlock, and closing a title to free what it holds
open, are two things the deploy loop needs constantly. Both were built inside `obscene-tool`
(`run_hw_restart_ui`, `run_hw_close_app`), each hand-parsing `procstat`/`ps` output and
encoding the signal policy inline.

That is the wrong home. The transport was never the duplication - both already ran over
`pros_link::shell`, prosperous's own channel. What leaked into obSCEne was the *capability*:
finding a process by name, knowing `SceShellUI` is the one to kill and that `SceSysCore`
respawns it, knowing a stopped title must be woken with `SIGCONT` before `SIGKILL` or it
leaves locked vnodes behind. That is target-management knowledge, and prosperous is the
target-management product. obSCEne had it only because it needed it first.

It does not go to `oops-libs`. That repository's admission rule (its D008) bars domain code,
and "which process is the console's UI" is domain code. It goes to `pros-core`, beside the
`Process` model and the `ps` parser that were already there.

The capability is expressed the way `launch` already is: pure builders and selectors in
`pros-core::system` (`shell_ui`, `of_title`, `kill`, `end`, a `Signal` enum), with the effect
- running the command over the shell - left to the shim. `end` is where the STOP-then-continue
policy lives, as data rather than as a branch at a call site, so it is tested against a
fixture without a target.

It is **first-class in the CLI**: `pros restart-ui` and `pros close <id>`, beside `launch`,
routed through `shsrv` like it. The GUI gains the same, and obSCEne's two functions become
thin calls into `pros-core` rather than a second copy of the recipe - so every hardware
interaction goes through prosperous, and the knowledge has one home.

The one thing not yet confirmed on hardware: obSCEne discovered the UI process with
`procstat -a` and this uses `ps`, because `ps` is what `pros-core::system::processes` already
parses. `ps` lists every process, so `SceShellUI` is present in it - but that the command
column carries exactly that string under `ps` is a reasonable expectation rather than a
measured fact, and it is marked so until a run proves it.
