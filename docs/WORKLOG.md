# Worklog

One entry per milestone. Between milestones, commit messages are the record.

## 2026-09-22 - Restore sends only what changed

- `restore` and `probe` skip a file whose local digest matches what was last verified landing
  at that path, provided a directory listing still shows it there; `--all` sends everything.
- The record is kept locally in `deployed.json` because the target unwraps a signed container
  on read, so any size or hash it reports is of the unwrapped file (D034).
- A container is verified by presence and a plain file by size (D031).
- The shell takes a bare `\n`; it does not strip `\r`, so `launch <id>\r` resolves no title.

## 2026-09-22 - Process control in both programs

- `pros ps`, `pros top`, `pros kill`, `pros close` and `pros restart-ui`; the window's system
  panel has the same actions, a memory column, a sort chooser and opt-in auto-refresh.
- The target's `ps` prints memory as `current / peak` MiB and has no CPU column; memory is read
  from the end of the row because the title column is blank for a non-title process.
- The target's `kill` builtin takes `-s <number>`, not `-9`; a stopped process is woken before
  it is killed so it runs its exit teardown.

## 2026-09-21 - The probe loop is one command

- `pros probe <id> <dir>` closes the title, restores the build into `/data/homebrew/<id>`,
  waits for the title to register, attaches the log follower, launches, and follows until the
  title parks, exits or `--seconds` elapses.
- Following before launching matters: a probe can finish its output within the first second.
- A parked big-app ignores signals, so a close cannot free a slot a parked title holds.

## 2026-09-11 - Moonlight bridge

- `pros-moonlight` makes a target a Moonlight host: mDNS, pairing over HTTP and HTTPS, RTSP, RTP
  video with Reed-Solomon parity, and an ENet input channel forwarded to the target as `PPAD`
  records. `pros fake-target` stands in for the target.
- The streaming half is adapted from Moonshine (BSD-2-Clause); the notice is in
  `THIRD-PARTY-LICENSES.md`.
- The pairing handshake hashes fields in an exact order; a byte out of place fails as a wrong
  PIN, so the server's own source is the specification.

## 2026-09-07 - Frame watch on Windows

- A timed-out read is `WouldBlock` on Unix and `TimedOut` on Windows; the frame pump treats
  both as a pause between frames.

## 2026-09-01 - Remote management

- `pros` and `pros-gui` register a target, check its services, fetch and verify payloads,
  send them, read the kernel log, run shell commands and move files.
- `pros-link` is the transport over `std::net`, used by obSCEne as well.
