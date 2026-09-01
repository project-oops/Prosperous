# D014 - Downloading is built, because a target handed over digests to check it against


**decided** - 2026-08-26 - immediately after D013

Fetching a payload was the one thing this project refused to build, twice, and the refusal was
never really about the network. **It was that a download nobody can verify is worse than no
download**, because it looks exactly like one that worked - and until a target produced a
repository with real SHA-256 digests in it, nothing here could tell those apart.

That changed, so this changed.

### It is not an HTTP client

Mirrors are served over a secured transport, so a client means certificate verification, a
root store and a protocol implementation - a large dependency for a project that argues for
each of its three. Every machine this runs on already has a program that does it, so this
runs that, exactly as watching runs a player. **Command as data**, one line in
`fetch.txt`, `{url}` and `{into}` substituted, defaulting to `curl -fL` which ships with
Windows, macOS and essentially every Linux.

The same pattern twice is now a pattern: where a large, solved problem sits behind an
interface, this project **runs the solution rather than becoming it**, and keeps the command
somewhere a person can change.

### The order of operations is the whole design

1. **Refuse before downloading** if the entry states no digest this can check. There is no
   point spending somebody's bandwidth on a file nothing could say anything about afterwards.
2. Download to `incoming/`, **not** to the staging directory. That directory's promise is
   that everything in it was verified, and an unchecked file sitting in it for the length of
   a download is that promise being false for a while.
3. Verify, then move. **Nothing wrong is kept**, and the temporary copy is removed either
   way - a file that failed its digest must not be lying around looking like a payload.

### Proved against a real mirror, both ways

`elfldr` fetched from the url in a target's own repository: 397,024 bytes, digest matching
the repository exactly, `7f 45 4c 46` and `e_type 0x0003` - a payload, by this project's own
guard.

Then the case that matters. A manifest claiming a **different** digest for that same real
url: it downloaded, failed its check, named both digests, said *do not send this*, kept
nothing, and exited non-zero. The previously verified copy was untouched.

### What it makes possible in one table

    on   2 here   elfldr     running, second in the boot list, staged
    on   - here   shsrv      running, not in the boot list, staged
    off  - here   klogsrv    not running, not in the boot list, staged and verified

That last line is the tool doing the job it exists for: a service this target is missing,
now sitting here checked and ready to send, found by asking the target what it was missing
and the target's own repository where to get it.

