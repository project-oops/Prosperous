# Payloads

A payload is a program you send to a jailbroken console for it to run. Prosperous describes
them, verifies them, and delivers them - it does not ship any.

## Why none are bundled

**Payload binaries are never shipped, only described.** What Prosperous carries is a manifest:
names, URLs, checksums and what each thing is for. The binaries stay where their authors put
them.

That keeps two questions separate. "Where did this come from" is answered by the manifest, which
is text you can read. "Is this the thing it claims to be" is answered by a digest, checked before
anything is executed with kernel-adjacent privileges.

```bash
pros payloads
```

shows what is described, what can be trusted, and what is already on the target.

## Fetching, and why a checksum comes first

```bash
pros send --name living-room <payload>
```

A payload with **no usable checksum is refused before anything is downloaded**. There is no
point spending your bandwidth on a file that could not be checked when it arrived, and a
download that cannot be verified is not a smaller problem than one that fails verification - it
is the same problem, discovered later.

Nothing that fails its digest is kept.

If you want to watch the verification:

```bash
OOPS_LOG=debug pros send --name living-room <payload>
```

A mismatch is logged with both digests - the one expected and the one found - because "checksum
failed" without them tells you nothing about whether you have the wrong file or a corrupted
download.

## Staging and delivery

Fetching puts a verified file in the staging directory. Sending hands it to the loader on the
target and runs it.

Those are separate steps because they fail for unrelated reasons: a fetch fails because of the
network or a bad digest, a send fails because the loader is not running. Collapsing them would
produce one error message covering two remedies.

The window's payload view offers the same, and will fetch into **a folder you name** - a window
that shows you a folder and offers to fill it must fill *that* folder. A file verified into some
other directory is on disk, correct, and invisible, which is indistinguishable from the download
never having happened.

## Supervising

```bash
pros supervise --name living-room <payload>
```

Keeps a probe alive on the target while something else drives it. This is what obSCEne uses when
it wants a console to answer questions rather than run a program to completion.

## What Prosperous will not do

**It does not modify a payload.** What is sent is byte-for-byte what was verified.

**It does not decide a payload is safe.** A digest says a file is the one the manifest describes.
Whether the manifest describes something you should run on your console is your call, and no
amount of checksumming answers it.
