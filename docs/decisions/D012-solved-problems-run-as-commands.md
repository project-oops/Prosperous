# D012 - Solved problems run as configured commands

**Status:** decided
**Date:** 2026-09-26

Where a large, solved problem sits behind an interface, Prosperous runs an existing program for it
rather than embedding one. The command is one line of text in a file beside the registry, with
placeholders substituted: `player.txt` (`{address}`) for a video player, `fetch.txt` (`{url}`,
`{into}`) for downloading.

**Why:** a decoder or a secure-transport client is a large dependency, and a video decoder would
need foreign code in a workspace that forbids unsafe. Every machine already has such a program,
and its path and arguments differ between machines, so the command belongs in data.

**Rejected:**
- Embedding a decoder or an HTTPS client: a large dependency for a problem already solved.
- A hard-coded command line: works on one machine.
