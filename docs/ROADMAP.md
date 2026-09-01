# Where this is up to

A review, current at 2026-08-27. Written down because the alternative is somebody asking
"what is left?" and being answered from memory.

Four lists: what is **wrong**, what is **built and not reachable**, what is **missing**, and
what is **unmeasured**. The last is not a lesser version of the third - an unmeasured thing is
one this project has deliberately declined to guess about, and closing it means asking a
target rather than writing code.

---

## Wrong

### Open

Nothing known.

### Closed, and worth recognising on sight

Every one had the same signature: **an output that looks identical whether or not it worked.**

| What it did | Why nobody noticed |
|---|---|
| Backup walked into `.` and out through `..`, copying system files into `.config` | It copied steadily, with progress that looked like a large folder |
| Two panes drawn as separate lists, not merged | With an empty folder, merged and stacked render identically |
| `install`/`send`/`run` meaning different things in different places | Each button was individually correct |
| Payload table pushed the target pane off screen | `set_width` is a request; nothing clipped |
| Scroll areas that never scrolled | An unbounded one grows to fit and silently never needs to |
| Groups laid out sideways | `allocate_ui` inherits the parent's direction |
| A tick on a folder navigated instead of selecting | One `bool` meant both "ticked" and "clicked" |
| Cheats' "none of these" notice under every other section | The answer outlived the question |
| Title names fetched over the network, then discarded | Nothing displayed them; nothing said so |
| `open folder` opened nothing | It wrote the path to the status line, which reads as success |
| Sending a payload left the check screen red | The screen reported a measurement that had stopped being true |
| The payload dropdown found one payload in ten | It read the top level; they live one folder down |
| Install was wired to a form measured not to work | A path and a url both "worked" until one was tried with a real package |
| A test opened the user's Documents folder on every run | It spawned a file browser, then deleted the folder it was pointed at |
| An install read 512KB of a 62MB package and reported success | It returned a content identifier either way |
| A refused `launch` reported as *asked* | Every non-usage reply read the same, refusals included |
| Double-clicking a folder never opened it | In egui a double click **is** a click, so the second `if` overwrote the first - and the row still highlighted and ticked |
| The stream's last unit was never counted, so a one-keyframe stream reported *no keyframe* | A unit is only whole when the **next** start code arrives. Live, that is one frame of lag nobody times against; at the end it made the diagnosis accuse a correct stream |
| Input stopped whenever the controllers panel was not on screen | The pump lived inside that panel's drawing. A feed sending nothing and a feed never polled look identical |
| A held key sent one record and then went quiet | A held key produces no events, so nothing woke the window. It read as a pad that drops inputs |
| The fake payload's pacing knob turned nothing | A unit is under forty bytes, so a whole short stream fitted in one four-kilobyte write and the delay between writes landed once, at the end. The rate it existed to produce could not be produced |
| Selecting four and pressing *send* sent one | It started the first, unticked **that one**, and broke out of the loop. The three that were dropped stayed ticked, which reads as "not done yet" rather than "never asked for" |
| Packages opened on a folder holding no packages | `/data/homebrew` is the **parent** of where they are. One subfolder and no files looks like an empty folder |
| Two buttons captioned *homebrew* and *pkg* | Labels were derived from the path. That reads as a real choice, and neither word names a tool or a purpose, so it cannot be made |
| Check offered to **download** a payload the target already had | The payload scan only ran when somebody opened the autoload screen, so the panel that gives the advice had never looked. Downloading it would then have offered to send a second copy back to the machine it came from |
| One target's payload list shown under another's name | `payloads_there` was the one arrival-clear that got missed |

Four of these were mistakes in **reasoning**, not code, and every one was settled by reading a
file that was already on the disk:

- *"A bare path and `file://` reach the same code"* - concluded from both giving an identical
  complaint. They were identical because **both fail**.
- *"A `#` line may stop the boot chain"* - a guess that prevented a feature for a week. The
  manager's own source logs the unresolvable name and carries on.
- *"A stray word makes `launch` start whatever the first word names"* - it starts the right
  title and passes the rest to it as arguments. The refusal was right for the wrong reason.
- *"Nothing `launch` says means it failed"* - every call is `perror`'d. Refusals had been
  reporting as successes.

The last two are worth a note beyond their own module: both were written **about a program
whose source was sitting in a sibling directory**, and both survived until somebody asked a
plain question about what the button did. The cost of reading it was four minutes.

The general fix for staleness is [`Job::disturbs`]: every job declares what it may have
invalidated, matched exhaustively, so the next one added cannot forget.

The last three were found the same way, and it is worth naming: **the stand-in's two halves
had unit tests and no seam between them had ever been exercised.** `pros-link::standin` plays
the payload that does not exist yet, and found the keyframe defect on its first run. A piece
that is individually correct is what every row in this table was.

---

## Built and not reachable

Written, tested, and with no way to get at it from the window or the command line.

- **`graft`** - putting one save's contents into another save's container, which is the whole
  region-swap feature. 6 unit tests plus one against real samples. **No caller.**
- **`sfo::set`** - editing a parameter file in place. **No caller.**
- **`graft::set_account`** - rewriting `ACCOUNT_ID` so a shared save is taken as yours. **No
  caller.**

`handover` is no longer on this list: the install action uses it.

That is the offline half of save retargeting, complete and inert. The other half - mounting
and re-encrypting an `sdimg_` container - belongs to `garlic-savemgr` and has not been
integrated.

---

## Missing

### Named in the design and absent

- **`pros-video`** - the third crate in `DESIGN.md`. The frame-grab half is being built in
  `crates/pros-link/src/frames.rs` by another session.
- **Media transfer** - a sync section pointed at a media folder. Mostly a path and an entry.

### Reachable now that the shell has been read

Read from `shsrv`'s own source rather than found by poking, which is why the list below
includes things this program has never called.

- **`launch <APPID>`** - **built.** A toolbar action in the titles section. It sends an
  identifier to `sceSystemServiceLaunchApp` and no file at all - see
  [D024](decisions/D024-three-ways-to-make-something-run-on-a.md)
  for why that is a third button rather than a variant of `run`.
- **`hbldr <path>`** - **built**, and reachable from the check screen as *run there*. Runs an
  ELF **already on the target's disk**, through the same `elfldr_spawn` the loader port uses.
  The only one of the four ways to run something that moves no bytes at all.

  It exists because the advice beside it was wrong: a service that was not answering, whose
  payload was already sitting on the target, was met with an offer to **download** it.

  **`hbdbg`** is the same binary waiting for a debugger, and is deliberately not offered:
  nothing on this side can attach, so a payload stopped before its first instruction is a
  target that looks hung.
- **`browse <URL>`** - the same binary as `launch`, opening the web browser at a url. Bears
  directly on the stream section as a way of putting something on the target's own screen -
  which is a different thing from the stand-in, and still worth having.
- **`notify`** - a message on the target's own screen. Worth having as *"this is the machine
  you think it is"* before something destructive.
- **`mount`**, **`sfoinfo`**, **`sfocreate`** - in the core bundle. These sit on top of the
  save questions listed under **Unmeasured** below, several of which are unmeasured precisely
  because mounting a container was assumed to need `garlic-savemgr`. Whether these reach an
  `sdimg_` container is itself unmeasured, but it is now a question with a place to ask it.
- **`sum`** - **tried and abandoned.** It answers `50507 /path`, a 16-bit checksum, and none
  of BSD, System V, plain, CRC-16 (IBM, CCITT, XMODEM), CRC-32 or Adler-32 reproduces it for a
  file whose bytes are in hand. A number this side cannot compute cannot verify anything: the
  point was to compare two ends, and that needs both to agree on the arithmetic.

  Verifying a send from both ends is still possible by **reading the file back and comparing
  bytes** - definitive, needs no agreement, and costs the transfer twice. Worth offering for a
  payload; not for a sixty-megabyte package.

### Gaps in what exists

- **Removing a directory from a target** is refused rather than implemented. Emptying one is a
  walk that deletes things nobody listed - the backup bug with the consequences reversed. A
  deliberate refusal.
- **No progress for a single large file.** A folder copy reports per file; one 250MB file
  reports nothing until it lands.
- **The activity record is session-only.** Right for capabilities, arguably wrong for a
  transfer history somebody wants tomorrow.

---

## Unmeasured

- **Whether a game accepts a grafted save.** The cryptography demonstrably does not stand in
  the way; a game checking its own build or region internally still might. Per-game, and no
  systematic testing exists publicly.
- **What verifies the `PARAMS` HMAC in `param.sfo`.** `garlic-savemgr` cannot compute it and
  copies one from a local save, so it is per-hardware rather than per-save. Which component
  checks it, and what happens when it is wrong, is undocumented.
- **Whether a target rejects `ACCOUNT_ID = 0` outright.** Every tool rewrites it, so the
  question is untested in public.
- **Whether an FTP round-trip of an `sdimg_` container restores.** The account gate assumes a
  matching account is a plain copy. No save has been put back and loaded.
- **Ports for payloads outside the five known services.** The `port` field exists so a list can
  close this without a rebuild.

### Closed since the last review

- `pkg_install` installs from an http url. Proven against a real package: a real `content_id`
  from a url, an empty one from `/data/...`.

  **The conclusion drawn from that was too strong**, and reading the builtin says so. It does
  no scheme checking at all - `metainfo.uri = argv[1]` straight into
  `sceAppInstUtilInstallByPackage`, in `shsrv/bundles/pkg_install/pkg_install.c` - and etaHEN's
  own writeup of that API says its `url` accepts local paths as well as http. So *"it cannot
  read a path on the target's own disk"* is not what was measured. What was measured is that
  **the one path form tried did not work.**

  There is an untried form, and a reason to think it matters: `/user/data` and `/data` are the
  same store under two names, and the installer is a system service that may not share the
  shell's view of the tree. `pkg_install /user/data/pkg/<name>.pkg` has never been run. It is
  an install, so it needs somebody's say-so rather than being tried quietly.
- A package served from this machine installs. Measured end to end **twice, with different
  packages**: `IV0002-ITEM00001_00-STOREUPD00000000`, and on 2026-08-27 a 41MB store package in
  22.0s reporting `IV0002-NPXS39041_00-STOREUPD00000000`. Two different content identifiers
  from two different files is the part that matters - one could have been a constant.
- **Where packages are kept is not a property of the machine.** On a target running one
  particular upload tool, `/data/pkg` and `/data/homebrew/pkg` both held the same three
  packages and `/data/homebrew` held none. Both are made by that tool, so a target running
  something else has neither, and the section asks rather than asserts.
- **A target fetches a package in ranges, and an install that ignores them never transfers it.**
  `libhttp/12.40 (PlayStation 5)` asks `bytes=0-65535` first, then grows. Answering every
  request with the whole file and a `200` made it retry that first chunk eight times and stop
  at 512KB - reporting a content identifier for a package it had barely read. Honouring the
  range walks the whole file to its last byte.

- `!N` in the startup list is milliseconds - `atoi` then `usleep(n * 1000)`, read in the source.
- Comments in the startup list are safe: an unresolvable name is logged and skipped.
- `pkg_install` success is a populated `content_id`; failure is an empty one.
- The keystone is `HMAC-SHA256(key, passcode)`, static per edition. It covers no save contents,
  no parameter file, no title, no account.

---

## Questions worth putting to somebody

1. **Is save retargeting worth the `garlic-savemgr` integration?** The offline half is done.
   The other half is a payload with an HTTP API on port 8082 and a mount/unmount lifecycle - a
   meaningful piece of work for a feature whose per-game success rate nobody has measured.
2. **Nothing is committed.** No commit on the branch; every file staged or untracked.
3. **How far to go with unpairing from the scene?** [CAPABILITIES.md](CAPABILITIES.md) is the
   design. The evidence it is real: the shipped payload list already carries three FTP servers
   and several autoloaders, and this program hardcodes one of each - four paths and three file
   formats belonging to `pldmgr` alone. Staged so the first two steps are worth having on
   their own; steps three to five are a genuine piece of work.

---

## Numbers

- 361 tests, 15 of which need a target or local samples and are `#[ignore]` by default
- `PROS_TARGET=<address> cargo test -p pros-core --test against_a_target -- --ignored`
- 23 decisions in `DECISIONS.md`
- Gate: `bin/prosperous check` - format, clippy at `-D warnings`, tests, `cargo doc` as errors

[`Job::disturbs`]: ../pros-gui/src/state.rs
