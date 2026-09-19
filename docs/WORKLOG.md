
## Two staging bugs the same restore surfaced: an id rewrite and a store that lied

A restore to `/data/homebrew/MESA00001` landed at `/data/homebrew/PPSA00001`, on top of an
existing title, and it reported success while the console's `eboot.bin` kept its old size. One
click, two independent defects, both in the staging path, both now fixed.

**The guard rewrote a title id it was never asked to (D030).** `guard::check` had grown a second
job beside catching an inert `/user/app` destination: it judged the title's *prefix*, and any id
not `PPSA`/`CUSA`/`FAKE` was rewritten to `PPSA<suffix>` - so `MESA00001` became `PPSA00001`, a
Sony id nobody asked for that collided with a real title, and `-y` accepted the rewritten path. A
prefix is not a defect and the homebrew folder is where a non-Sony id belongs, so any destination
that is not inert is now accepted as written, the redirect for an inert one keeps the id verbatim,
and `sanitize_title_id`/`is_supported_prefix`/`SUPPORTED_PREFIXES` are gone. The prefix rule was
*reasoned*, not measured - the module asserted a ShadowMountPlus constraint nothing here had
measured - and a reasoned default that overwrites a title on `-y` is the plausible-wrong-default
this project exists to refuse.

**A `STOR` the target acked was counted as a file, replaced or not (D031).** `upload` trusted the
completion code. A mounted title answers `226` and keeps the old bytes, so the restore counted
seven files and replaced none of them - the size on screen was the earlier `pros push`, not what
restore claimed. Now each store is followed by a `SIZE` read (`Session::size`, new in `pros-link`)
and a mismatch is recorded as not-copied, so the summary is incomplete and the caller exits
non-zero naming the file, instead of printing success. `say::copied` and the window already failed
on an incomplete summary; the gap was upstream, in believing the reply.

The surprise worth keeping: **the fake already knew.** Its `Store` note says in as many words that
"a store that worked and a store that reported success are different things, and only the contents
afterwards tell them apart" - so the fake grew a `swallows_stores` mode that acks a write and keeps
nothing, and the new test drives exactly the mounted-title failure through it. The transport had
been trusting the reply its own fake was built to warn about.

## The oops-apps payloads and titles join the catalogue, by their relative paths

The payloads pane reads the manifest, not the chains file, so a payload named only in `chains.json`
is invisible there and unfetchable - the catalogue is where prosperous *knows* a payload, and it
is the app-store master list the whole tracking/downloading/multi-source machinery hangs off. Added
the first-party oops-apps builds to it, `source_local` (a repository-relative path) as the primary
source, exactly as `pltauth-patch` already was (D-era local-build resolution, commit that added
"conventional path resolution"): `sandbox-daemon` to `recommended.json`, and `gallery`, `net-tool`,
`pad-viz`, `seashell` to `titles.json`. Each resolves under `oops-apps/src/<name>/dist/` at runtime
via `fetch::local_build`, with a `github.com/project-oops/oops-apps` release as the remote fallback
and a pinned digest so that fallback is verifiable.

**No absolute path is written anywhere** - `source_local` is relative and resolved against the OOPS
root discovered at runtime, which is the whole reason it can be a tracked source rather than a
machine-specific one.

Two surprises worth keeping:

- **`gl-cube` and `tracer` were held back**, and a test is why. `every_shipped_entry_can_be_fetched_and_verified`
  requires every shipped entry to carry both a url and a digest - "nothing ships with a url it cannot
  check." Neither has a `-title-prospero.zip` built yet (`gl-cube`'s dist ships only an eboot,
  `tracer`'s is empty), so there is nothing to hash and nothing to ship. The invariant is right: the
  master list must not advertise an artifact that does not exist. They go in once built.
- **The manifest was already configurable without a rebuild.** `recommended.json`/`titles.json` are
  compiled in for a useful fresh install, but `Tracked::read` merges an on-disk `payloads.json` /
  `titles.json` beside the registry over the shipped defaults and writes the merge back - so the new
  shipped entries fold into a machine's existing file on next launch, and anyone can add one to that
  file by hand without recompiling. The compile is for the default, not the ceiling.

**Follow-up: the payloads-pane refresh now reconciles instead of reading raw.** A new entry not
showing up turned out to be a stale binary - the shipped catalogue is compiled in, so an
already-running window (a debug build a day old) knew nothing of it, and its `payloads.json` on
disk was untouched, which is exactly what a current binary would have rewritten. That was the
diagnosis. But it also exposed a real papercut: the pane's *refresh* button read `payloads.json`
raw (`Manifest::from_file`), so it could only ever show what the file already held, never a payload
learnt since. Refresh now goes through `Tracked::read`, the same path startup uses - shipped merged
over the file and written back - so it reconciles rather than re-displays. The lesson worth keeping:
"the manifest is being ignored" was really "the binary is older than the manifest", and the on-disk
file's mtime is what said so.

## `pros ps` and `pros kill`, so process control needs no raw shell

Ending a process by pid was reaching for `pros sh "kill …"`, and the target's `kill` builtin
rejects the `-9` shorthand - it wants `-s <number>`, the form `pros_core::system::kill` has always
built. So the knowledge to do this cleanly was already here (D027's primitive - `Signal`, `kill`,
`end`, which wakes a stopped process before killing it); what was missing was a verb that used it
instead of a person guessing shell syntax. Added `pros kill <pid>`, over a new
`system::by_pid` selector, doing exactly what `pros close` does but aimed by pid rather than by
title: find the one process, run `end`, read the target again, say whether it is gone.

`pros kill` on its own would have been half a tool - **you cannot kill a pid you cannot see, and
the command line had no way to list them** (the window's system panel did, the CLI did not). So
`pros ps` came with it: the same `ps` the panel reads, parsed by `system::processes`, printed as
a table. That closes the principle-3 gap the other direction too - and the window gained the
matching half, an *end* button on each non-title process, beside the *close* it already had on
titles.

The surprise worth keeping: **the fix was a verb, not a primitive.** Everything needed to end a
process correctly - the right signal number, the wake-first-if-stopped order, all measured - had
been sitting in `system` since D027, used only by `close` and `restart-ui`. The raw-shell
workaround people reached for was worse than code that already existed; it just had no name a
person could type.

## A chain carries its files, so export and deploy keep the settings around the list

`export chain` wrote a payload order and nothing else, so a chain read off a working console
came back in the right order and behaved differently - the console's settings, the switch that
decides whether the list runs at all, were never in the chain. Now a chain carries files: a
`{path, content}` copy of each declared companion file. Export reads them off the target and
folds them into the preset; deploy puts each back verbatim (`Step::Place`), after writing the
list and before the autoload switch is guaranteed.

**Which files are worth carrying is declared in `chain.json`, not written in code** (a top-level
`capture` block, `baseline::Capture`), with `{device}`/`{usb}` expanded the way a list's places
are. That is the whole point of the shape: the pldmgr settings path lives there as data, so a
name that moves or a second file worth keeping is a JSON edit, not a rebuild - and it declares
*paths only*, so no console's settings are shipped in this repository. The one thing in code is
the switch, which stays as the protocol guarantee that a list nobody reads is pointless (D029).

The surprise worth keeping: **the gate was already red on this toolchain, in three crates, before
any of this.** `cargo clippy --all-targets -- -D warnings` under clippy 1.98 aborts a crate at
its first lint, so `pros-cli`'s `run` (102 lines) hid behind a `collapsible_if` the stabilised
let-chains now flag; `pros-gui` had an `assigning_clones`; and `cargo doc -D warnings` had never
been green on `pros-moonlight` (a handful of intra-doc links to `Ports`/`Pad` that do not
resolve from where they were written). None of it was this change - it surfaced because the full
gate was run. The incidental fixes are all mechanical (clippy's own rewrites, full-path doc
links, one arm of `run` extracted into a `push` helper the file's own principle 3 wanted anyway),
and the whole gate is green again: fmt, clippy, tests, doc.

## The Moonlight bridge streams: video and input, on Moonshine's shoulders

The bridge now does the whole of part four. Video: the target's Annex-B off 9805 is grouped into
frames (`nal`), each packetised into RTP with the NV video-packet header and Reed-Solomon parity
(`video`, `fec-rs`), and sent to the client's 47998 (`session::pump_video`) - proven by a test that
runs a fake source through the real pipeline to a UDP sink. Input: the client's controller packets
arrive over an AES-128-GCM ENet channel on 47999 (`control`, `rusty_enet`), and each becomes a
`PPAD` record forwarded to the target's 9806 (`input`) - which is exactly what the fake target
prints. The RTSP handshake (`rtsp`) ties launch to play, and PLAY starts both.

This half is **adapted from Moonshine** (Hans Gaiser, BSD-2-Clause), not merely informed by it, so
the attribution grew to match: its copyright notice is retained verbatim in
`THIRD-PARTY-LICENSES.md`, `ACKNOWLEDGEMENTS.md` names it the base, and each streaming source file
says which Moonshine file it follows. BSD-2 permits the derivation and asks only that the notice
travel with it, which it now does.

Two surprises worth keeping. First, **the protocol's own source is the only reliable spec** - the
RTP `fec_info` word packs shard index, data-shard count and FEC percentage at bit offsets 12, 22
and 4, and the control nonce is the sequence followed by zeros and the bytes `HC`; none of that is
guessable, and reading Moonshine (which had already read Sunshine and moonlight-common-c) is what
made it exact. Second, **the two halves need different transports for a reason**: video is
fire-and-forget RTP/UDP with FEC to survive loss, input is an ENet channel because a dropped button
is wrong forever - the same split part three drew between a state you resend and a delta you cannot.
What a headless test still cannot do is show the final picture in a real client; that is the one
step left, and it needs a Moonlight app on the LAN rather than more code.

## The Moonlight bridge, built up to pairing, in a crate of its own

Prosperous can now be a Moonlight host. The whole thing lives in a new crate, `pros-moonlight`,
reached by two `pros` verbs - `fake-target` and `moonlight` - and it stays out of `pros-core` and
`pros-link` on purpose: it needs a TLS stack, a self-signed X509 certificate, AES/RSA/SHA, an mDNS
responder and (for the streaming half still to come) Reed-Solomon FEC and an ENet port, none of
which orbistoun or obSCEne should inherit by taking those two crates. So the heavy list stops in
the new crate, and the crate map is `moonshine`'s (BSD-2, credited): `rustls` with the `ring`
provider because it builds here with no cmake or nasm, `rcgen`/`x509-cert`, `rsa`/`sha2`/`aes`,
`mdns-sd`.

What is built and tested: the **fake target** (serves an Annex-B clip on 9805, prints the `PPAD`
records that arrive on 9806 - the "fake input" the design's item 5 calls for); the **pairing
crypto**, mirroring Sunshine's `nvhttp.cpp` byte for byte - a PIN-salted SHA-256 AES key, ECB
challenges, RSA-signed commit-and-reveal on both sides; and the **server** - mDNS `_nvstream._tcp`,
`serverinfo` and the four pairing phases over HTTP (47989), the final leg over HTTPS (47984)
presenting the very certificate the client pinned. Twenty-four tests, pedantic clippy clean.

The surprise worth keeping: **the hard part was the crypto exactness, not the plumbing.** Every
phase concatenates specific fields in a specific order and hashes them, and a byte out of place
fails silently as "wrong PIN" with nothing to point at. Getting it right meant reading the server's
own source (Sunshine) rather than a client's description of it, because the client and server are
mirror images and only one of them is the thing being written. Once the four-phase handshake passed
against a test client that plays Moonlight's exact steps, standing it up over real sockets - HTTP
parsing, TLS with the pinned cert - was ordinary. The verification that a real client will pair is
that test: it *is* the Moonlight algorithm, driven against the bridge's own routing over real TCP,
plus the running binary answering `serverinfo` on both ports. What is left is the session: RTSP,
RTP video with FEC on 47998, and the ENet input channel on 47999 mapped to `PPAD`.

## A quiet socket ended the watch on Windows, and the pump now knows both names for a timeout

Reviewing the Porthole wire contract against `pros-core::watch` found the pump reading with a
half-second timeout and treating only `WouldBlock` as the pause between frames. That is the
name Unix gives a timed-out read. Windows gives it `TimedOut`, so on Windows the first
half-second with nothing arriving ended the stream - and said so with a line about the
connected party failing to respond, which reads as a fault on the target's side. The
transport's own log reader had already learned this (`wire::is_quiet` matches both names); the
pump had not.

Two tests pin it. The unit test feeds a source that answers every read with one name and then
the other, with no socket, and requires the pump to go round rather than settle. The standin
test opens a silent fake with a short timeout and lets the platform choose the name, which is
the reading that failed here before.

**The surprise worth keeping:** nothing in the suite had ever exercised the timeout arm. Every
socket test read with a five-second timeout against a fake that answered within milliseconds,
and the one test that did wait on silence read the socket directly and matched both names in
its own assertion - the knowledge was in the test file and not in the code beside it.

The same review found the target side of the same seam blocking on its input socket, so that
the video stalled whenever a pad was at rest; that is fixed in Porthole, in oops-apps, and
recorded there.

## Process control moved in from obSCEne, first-class in both programs

Restarting the user interface and closing a title were built inside `obscene-tool`, each
hand-parsing `ps`/`procstat` and encoding the signal policy inline. The transport was never
the duplication - both already ran over `pros_link::shell` - but the *capability* had leaked
into a consumer: which process is the interface, that the system respawns it, that a stopped
title needs a wake signal before a kill or it leaves locked vnodes behind. That is
target-management knowledge, so it came here. (D027)

It sits in `pros_core::system` the way `launch` does: pure builders and selectors - `shell_ui`,
`of_title`, `kill`, `end`, a `Signal` enum - with the effect left to the shim. `end` is where
the wake-then-kill order lives, as data rather than a branch at a call site, so it is tested
against a `ps` fixture with no target in the room.

First-class in the CLI as `pros restart-ui` and `pros close <id>`, and in the window as a
*restart UI* button and a per-title *close*, both leaving the process list showing what is
running now. obSCEne's two functions became thin calls into `pros-core`, so the recipe it used
to carry is gone and the knowledge has one home.

**The surprise worth keeping:** obSCEne found the interface with `procstat -a`; this uses `ps`,
because `ps` is what `system::processes` already parses. `ps` lists every process so the UI is
in it, but that its command column reads exactly `SceShellUI` under `ps` is an expectation, not
a measured fact - flagged in D027 rather than asserted, and the one thing a hardware run should
confirm.
## contents(), which was made testable and then not tested

8 tests in `crates/pros-core/tests/walking.rs`. `transfer.rs` went from **71.60% to 80.17%**
of regions covered.

`transfer::contents` carries a doc comment saying it is *"separated from the sending so it can
be tested, and so a caller can show what is about to go before any of it does"* - and nothing
tested it. A seam introduced for testability and left untested is the cost of the seam without
the benefit.

It is also the half of a restore that decides what a restore *is*. `upload` takes a live
session and cannot be exercised without a target; `contents` decides the file list that session
is handed, so a folder missed here is a file that never goes back.

What is pinned: paths come back relative (an absolute one would carry this machine's directory
onto the target), the order is sorted rather than the filesystem's, directories are walked
rather than listed, an unreadable folder is an error and **not** an empty list, and the depth
bound stops a deep tree while still reporting what was inside it. The last one is asserted at
the boundary in both directions - twelve levels found, thirty not - because an off-by-one
there either loses a legitimate file or walks a level further than the rule says, and neither
shows up on a shallow folder.

`upload`, `configured` and `temporary` stay uncovered: they need a live session or the user's
own config directory, and a test that reached for either would be testing this machine.

## Why `run` was greyed on every payload, and the two folders behind it

Reported from the window: a payload ticked in the payloads pane, and `run` refused. The refusal
itself was correct - `Offer::Run` sends bytes *from here*, and `Entry::here` was `None` - but
four things around it made that unreadable, and one of them was a second directory nobody had
noticed was second.

**`open folder` opened a different folder from the one being judged.** It revealed
`manifest::staging()`, which is `cache_directory()/payloads`, under a comment stating that is
"where the payloads table sends from". It is not, and had not been for a while: the row action
sends from `local_path`, which is `data_root()/payloads`, and so does the toolbar, and so is
where a download is written. Somebody opening the folder to find out why `run` was greyed was
shown a directory that has no bearing on the answer. Now it opens the one the pane uses.

**The table had twelve cells per row and eleven headings.** There is no `"run"` heading, so
every label sat one column left of its data - `name` over the run buttons, `running` over the
sizes, `version` over the on/off marks. It reads as a table that is simply wrong about itself,
and it is invisible until somebody compares a value with the word above it.

**An empty local folder presented as thirty dead buttons.** `library::here` reports a folder
that does not exist as an empty one, which is right - a machine where nothing has been
downloaded is not a failure - but nothing said so, and the only explanation was on the hover of
a control that looked broken. It now says it once, above the table.

**`run a file...` opened wherever the last dialog had been.** It passes `local_path` to `rfd`,
which ignores a directory that is not there - and on a first run that directory does not exist,
because nothing has created it yet. Made before the dialog opens.

The surprise worth keeping: **none of these was the refusal being wrong.** Every one was
something around it disagreeing about which folder the question was about, and together they
made a correct refusal look like a broken button. Two names for one directory is enough to do
that on its own; a comment asserting the wrong one of the two is what made it survive.
