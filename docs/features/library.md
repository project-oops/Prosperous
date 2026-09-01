# The target's storage

What is actually on the console: titles, saves and packages, plus the file operations to get
things on and off it.

```bash
pros library --name living-room
pros ls     --name living-room /data
pros pull   --name living-room /data/report.txt ./report.txt
pros push   --name living-room ./payload.elf /data/payload.elf
```

The window shows the same three groups in its listing panel.

## Backup and restore

```bash
pros backup  --name living-room /data/savedata ./my-saves
pros restore --name living-room ./my-saves /data/savedata
```

A folder off the console and back again - a save, a title's data, anything. These are plain
recursive copies over the target's file service, not a save format Prosperous understands. It
moves bytes and does not interpret them.

**Restore does not merge.** It puts files where you told it. If something is already there, that
is between you and the console.

## Reading the system log

```bash
pros logs --name living-room --for 10s
```

The log **streams and never ends**, so a reader has to say how long to listen. There is no
length to read and no end to wait for.

**Nothing arriving is a result, not a failure.** A quiet log is a fact about the target. A tool
that reported silence as an error would make "the console had nothing to say" look like "the
tool is broken", and those need different reactions from you.

This is usually the fastest way to find out why a payload died rather than merely that it did.

## Running a command

```bash
pros sh --name living-room "ls -la /data"
```

The shell service has **no framing at all** - there is no marker saying a reply has finished, so
the reader stops when nothing more arrives. A command whose output pauses in the middle can
therefore be cut short. That is a property of the service, not a bug here, and it is why `sh` is
for looking rather than for scripting.

## What is stored locally

Nothing from this page. Listings, logs and directory contents are asked for each time and not
cached - a cached answer about a machine that power-cycles between questions is a wrong answer
waiting to be shown.

The only local state Prosperous keeps is the target registry and the staging directory. See
[targets](targets.md).
