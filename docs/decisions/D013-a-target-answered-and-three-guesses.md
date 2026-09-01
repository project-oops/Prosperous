# D013 - A target answered, and three guesses became measurements while two rules turned out wrong


**decided** - 2026-08-26 - the first run against real target, at a target on the local
network, read-only throughout

### The tests are opt-in, and their default state is *ignored* rather than *passed*

`crates/pros-core/tests/against_a_target.rs`, marked `#[ignore]` with a reason, run by
`./prosperous.sh target` with `PROS_TARGET` set. Two properties matter:

- An ordinary `cargo test` **reports them as ignored**, which is visible in the output. A
  suite that quietly passed them would be claiming evidence it does not have, and the whole
  point of separating these is that a stand-in cannot produce this kind.
- **Running them with no address set fails.** They were asked for by name; doing nothing and
  reporting success is the one outcome that must not be available.

Nothing in them writes to the target. A suite that can alter the machine it measures has
ambiguous failures, and this one runs against somebody's actual target.

### What was confirmed

- **The boot list is at `/data/pldmgr/autoload.txt`** and reads.
- **The repository is at `/data/pldmgr/repository_cache.json`** - the path D007 called a good
  guess and refused to make a default. It is a default now, with a measurement behind it,
  which is exactly the change that entry said it was waiting for.
- **The repository is a plain JSON array of 25 entries**, carrying the ten fields this
  project copied rather than invented. Copying was worth it.
- **Its digests are 64 bare hexadecimal characters - SHA-256**, the one algorithm this
  project verifies. Every entry read as verifiable. That was the open question in D004, and
  it is now closed by a target rather than by a guess.
- **Titles are at `/user/app`**, six of them, each a folder named exactly as an identifier -
  and the shape matcher read every one.
- **The listing format is what this client expected**: every line of the root parsed, sizes
  and names correct. This server sends no `total` header at all, so that filter is harmless.
- **A missing directory is refused**, `550 No such file or directory`, as a typed error.

### What was wrong

**Two defects, both in the boot list, both invisible to any stand-in.**

1. **`!3000` lines are instructions to the manager, not payloads.** A real list interleaves
   them between entries. Reading them as payloads turned a boot order of six into one of
   twelve with every real position doubled.

2. **Names carry versions.** The list says `elfldr_v0`, `kstuff-lite_v1`, `ps5upload-4`. An
   equality test called every one of them **absent from a list they were plainly in** - which
   in the boot column means telling somebody their loader will not come back after a reboot
   when it will.

   The first fix was too loose and produced a third defect immediately: accepting any
   separator made **`kstuff` match `kstuff-lite_v1`**, reporting one payload as another. A
   version has to look like a version - a digit, or a `v` and a digit - and the repository
   describing both `kstuff` and `kstuff-lite` is what made that visible.

And **one guess was simply wrong**: packages are in `/data/pkg`, not `/user/data`. Corrected.

### What the target showed that the design predicted

`shsrv` is **running and not in the boot list**. That is the case the boot column exists for,
and the first real target asked produced it without being arranged to: the shell is there
until somebody turns the target off, and nothing but that column says so.

### Saves are deeper than a default can reach

`/user/home/<user>/savedata_prospero`. The starting point stays `/user/home`, because which
user is a question this project cannot answer on somebody's behalf, and the browser exists to
walk the two steps.

