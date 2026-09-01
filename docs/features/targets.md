# Targets

A target is a console Prosperous knows how to reach: a name, an address, and any ports that
differ from the defaults. Everything else in the tool takes one as its subject.

## Registering one

```bash
pros register 192.168.1.211 --name living-room
pros list
pros forget living-room
```

In the window, the same three are the sidebar's target list and the **register** dialog.

A registration is **a name and an address and nothing else**. That is deliberate: it is the
smallest thing that can be wrong, and everything else about a console can change between one
power cycle and the next. What a target can currently *do* is never stored - it is asked.

Registering a name that already exists replaces the address and **keeps any port overrides**.
Re-registering is how somebody corrects a typo, and losing their ports at the same time would
be a silent edit to a file they did not open, discovered later as transfers going somewhere
unexpected.

## Where the registry lives

`targets.txt` in the collection's directory - `%APPDATA%\OOPS\` on Windows,
`~/.local/share/OOPS/` on Linux. **Shared with the sibling projects**, so a console registered
here is the console the others can reach.

It was briefly `~/.config/prosperous/` on every platform, chosen over `%APPDATA%` because a tool
running inside a packaged container has its writes there redirected into a per-package cache and
made invisible. That hazard is real and belongs to *packaged* applications - this is a plain
executable, so the platform's own directory is right.

The file is one line per target - a name, an address, optional `service:port` overrides - and is
meant to be edited by hand. If Prosperous cannot work out where to write it, it says so rather
than putting it somewhere you will never look.

## Asking what a target can do

```bash
pros check --name living-room
```

```
living-room (192.168.1.211)
  up   elfldr    :9021  send a payload to the target and run it
  up   ftpsrv    :2121  retrieve reports, stage payloads and packages
  up   klogsrv   :3232  read the system's own log
  up   shsrv     :2323  run commands without loading a payload
  --   pldmgr    :8084  inspect and reload the payload chain  (1510ms)
```

Each service says **what it buys you**, not just whether a port is open - a table of numbers
would be a worse tool than the numbers written down somewhere.

The loader is checked first, because its failure has a different remedy from every other
failure here: re-run the jailbreak, rather than reload a payload.

**A slow answer is reported.** The `(1510ms)` above is a service that did not refuse - it timed
out. Refused and timed-out look identical if you only print "down", and they mean different
things: a refused port is a service that is not running, a timeout is usually a firewall or a
sleeping console.

For the detail behind a check:

```bash
OOPS_LOG=pros_link=trace pros check --name living-room
```

## When it does not work

**Nothing is registered.** `pros list` prints nothing and exits cleanly. That is the state
everyone starts in, not an error.

**Every service is down.** The console is off, asleep, on another network, or has not been
jailbroken since its last restart. Prosperous cannot tell those apart from the outside and does
not guess.

**Only the loader is down.** The jailbreak has not been run, or has been lost to a restart.
Everything else being up is normal in that state.
