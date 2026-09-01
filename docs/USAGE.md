# Using it

What `pros` reports and how to drive it: reading a check, fetching and verifying payloads, and
working with titles, saves and packages on the target's storage.

This was the middle of the README. It is reference for somebody already running the tool, which
is not what a person arriving at the repository needs first.

## What a check reports

Not up or down. **What each service unlocks**, and whether its absence blocks anything:

```
living-room (192.168.1.206)
  up   elfldr    :9021  send a payload to the target and run it
  up   ftpsrv    :2121  retrieve reports, stage payloads and packages
  --   klogsrv   :3232  read the system's own log - why a payload died, not just that it did  (1504ms)
  up   shsrv     :2323  run commands on the target without loading a payload
  --   pldmgr    :8084  inspect and reload the payload chain  (1508ms)

usable, but klogsrv and pldmgr are not loaded, so something will be invisible if a run goes wrong
```

Three things in that output are decisions rather than formatting:

- **The loader is first**, because it is the one failure with a different remedy. The payload
  manager launches everything through it - including itself - so when it dies nothing can
  bring anything back and the dashboard keeps answering. A check that finds it down says
  *re-run the jailbreak*, not *reload a payload*.
- **A slow answer is said out loud.** A port that refuses instantly and one that takes 1500 ms
  mean different things, and they look identical in a column of up and down.
- **Nothing is cached.** A jailbreak does not survive a power cycle, so anything stored about
  what a target can do is a claim that expires without notice. A registration is a name and
  an address, and capability is asked every time.

Exit codes are part of the interface: **0** worked or answered-not-ready, **1** the tool could
not do what it was asked, **2** the target is blocked.

## Payloads are described, never shipped

This repository contains no payload binaries and CI refuses to let one be tracked. It ships a
manifest of where to get them, in the payload manager's own schema - so a target that is
already configured can be read as a source rather than typed in again:

```
pros payloads --from-target
```

The path is a default rather than a requirement, and it earned that: it was **measured on a
target** rather than guessed at. Give one to look elsewhere. Before the measurement it was
asked for every time, on the reasoning that a default carries the authority of a measurement
nobody has made - which was right then and is answered now.

**Checksum verification is not optional.** You are downloading from a mirror and then running
the result with kernel-adjacent privileges. An algorithm this cannot check is refused by name
rather than passed over, because a tool that reports success for something it never looked at
is worse than one that says it cannot help.

```
pros payloads --from-target --check   # what is described, what runs, what boots, what is here
pros fetch elfldr --from-target       # download it, keep it only if the digest matches
pros stage ~/Downloads/elfldr.elf --as elfldr    # or keep one you already have
```

A target's own repository carries urls **and** SHA-256 digests, which is what makes fetching
worth doing: **nothing is kept unless it is what the manifest says it is.** A download that
fails its check names both digests, keeps nothing, and exits non-zero.

There is no HTTP client here. Mirrors are served over a secured transport, and rather than
take on a security stack this runs the one your machine already has - `curl` by default, one
editable line in `fetch.txt` otherwise. The download is not the interesting part; the
verification is.

```
off  2 here   elfldr           0.30       send it a payload, it runs it
?    -      ! ShadowMountPlus  -          mounts things

columns: running / boot-list position / staged here / verifiable
```

Two things in that table are decisions. **`?` is not `off`** - a payload with no port this
project knows cannot be found by probing, and saying it is absent would be inventing a
measurement. And the boot column is a **second question**: a service can be answering now and
absent from `/data/pldmgr/autoload.txt`, which means it is there until the target is turned
off, and that is usually the thing somebody needed to know.

## Titles, saves and packages

Everything on the target's storage, over the file service alone:

```
pros titles                        # what is installed, by name rather than identifier
pros saves                         # saves, named by the game they belong to
pros library /user/app             # titles, packages, what else is there
pros backup /user/home/PPSA02664-SAVE00 --into save-backup
pros restore save-backup /user/home/PPSA02664-SAVE00
```

**A backup that quietly missed a file is worse than no backup**, so a copy that skipped
anything lists every one, says *do not treat this as a backup*, and exits non-zero. One
unreadable file does not end the walk. Links are not followed - one can point at its own
parent - and they appear in the skipped list like everything else.

Installing a package **is** built, and the note that used to sit here saying it was not was
left behind by the measurement that settled it. There is no service and no port: the shell has
a builtin, `pkg_install URL`, and it means URL - a bare path and a `file://` both return an
empty content id. `pros_core::install` is the implementation and
`crates/pros-core/tests/against_a_target.rs` exercises it.

**The window drives it; this command line does not.** `pros-gui` calls `pros_core::install`
and `pros` has no subcommand for it, so package installation is the one thing the two programs
do not both do.
