# D024 - Three ways to make something run on a target, and three buttons


`run`, `install` and `launch` sit near each other in the same toolbar and mean entirely
different things. They were built at different times against different services, and until the
shell's own source was read one of them was documented from a guess.

**`run` sends bytes.** An ELF goes to `elfldr` on its own port and is spawned as a process. It
lives in memory until the next restart. Nothing is written to the target's disk.

**`install` hands over a package.** The target fetches a `.pkg` over HTTP and registers what it
finds. Afterwards there is an installed application where before there was a file - see
[D021](#d021---package-installation-was-a-shell-builtin-not-a-service).

**`launch` sends nine characters.** Read in `shsrv/bundles/launch/launch.c`:

```c
sceSystemServiceLaunchApp(argv[1], &argv[1], &ctx)
```

with the foreground user from `sceUserServiceGetForegroundUser`. **No file crosses the link.**
The target's own system service finds the installed application by identifier and boots its own
signed executable, exactly as selecting it on the home screen does.

So the middle one is the bridge: `install` is how a file becomes something `launch` can name.
The first shares no machinery with either.

### What reading the source cost

Two claims written into `pros_core::launch` from reasoning, both wrong, both found by reading a
file that had been on this disk the whole time.

**"A stray word makes the target launch whatever the first word names."** It does not. The
builtin passes `&argv[1]` - the identifier *and everything after it* - as the application's own
argv. A stray word starts the right title and hands it something nobody meant to pass. The
refusal stands; the reason was invented.

**"There is no reply that means it did not work."** There is. Every call is `perror`'d, so a
rejection arrives as `sceSystemServiceLaunchApp:` and a reason. Everything non-usage had been
reading as *asked* - including refusals - which is this project's own defect class exactly: an
output identical whether or not it worked. `Said::Refused` now carries it, and the exhaustive
match in the window forced the failing path to be handled rather than defaulted.

**What it does still not promise is that the title started.** An accepted request is not a game
appearing, and nothing on this side can see the difference. The word is *asked*.

### The same reading found four builtins nobody had recorded

`browse <URL>` is the same binary as `launch` and opens the web browser at a url, which bears
on the stream section. `hbldr <path>` and `hbdbg <path>` run an ELF **already on the target's
disk** through `elfldr_spawn`, the second waiting for a debugger - a fourth way to run
something, and one that needs no transfer. The core bundle also carries `mount`, `sfoinfo` and
`sfocreate`, which sit directly on top of the save questions in `ROADMAP.md` that are listed
there as unmeasured.

None are wired. They are recorded so the next person does not conclude from silence that they
do not exist.

### Said in the window, not only here

The distinction is invisible in a toolbar of one-word buttons, so it is written where somebody
is looking: **every section carries a line under its heading** saying what it is for, and the
`run`, `install` and `launch` hovers each say what the other two do not. A person who has to
open this file to find out which button sends a file has already guessed wrong once.

