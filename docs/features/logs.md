# Logs

Real-time streaming and inspection of console kernel and user-space telemetry (`klog`).

Every print from the operating system kernel, system daemons, and running homebrew titles is broadcast unbuffered over port `3232`. Prosperous connects to this socket and presents a live, searchable stream.

---

## GUI: Live Telemetry Streamer

Open the **Logs** tab from the main navigation panel.

```text
+-------------------------------------------------------------------------------+
|  Console Kernel Log (living-room)                                [_][O][X]    |
+-------------------------------------------------------------------------------+
| Filter: [ sceAgc                             ]  [Auto-Scroll: ON] [Clear]     |
|-------------------------------------------------------------------------------|
| 118: <118>[SceSystemStateMgr] Power Mode Change: BIG_APP                      |
| 119: <118>[SceLncService] SetControllerFocus(0x00006018)                      |
| 120: <118>[AvControl] entry process(appid:0x6018 pid:0xbf)                    |
| 121: [OOPS-GL] creating AGC universal queue...                                |
| 122: [OOPS-GL] sceAgcDriverCreateQueue rc: 0x0                                |
| 123: [OOPS-GL] hardware AGC RDNA2 pipeline initialized successfully           |
| 124: [OOPS-GL] flush-words: 0x39b submit-rc: 0x0 fence-hit: 0x1              |
+-------------------------------------------------------------------------------+
| [follow / stop]  [clear]  filter: [____] [x]regex  [copy] [save] [open folder] |
+-------------------------------------------------------------------------------+
```

![Prosperous Live Telemetry Streamer](screenshots/logs.png)
*(Screenshot placeholder: Live Kernel Telemetry Streamer)*

### GUI Controls:
- **follow / stop**: Open the log connection and show lines as they arrive, or close it. The view is pinned to the newest line while following. A log that has ended and one that has gone quiet look identical, so the end is said in words rather than left to a lack of lines.
- **Filter Box (+ regex)**: Live match without dropping the rest from the buffer - plain substring, case-insensitive, or a **regular expression** when the *regex* box is ticked. A pattern that does not compile shows every line and says *invalid regex* rather than blanking. The count shows both numbers (`10 of 90 lines`) so a filter cannot make a busy target look quiet.
- **Scrollback**: The view holds up to 20,000 lines and is virtualized (only the rows on screen are drawn), so a long watch does not slow it down. This is scrollback only; the full history is the kept file below.
- **clear / copy**: Forget the shown lines (the log keeps arriving), or copy them - filter and all - to the clipboard.
- **save**: Write what is shown (filter and all) to a `.log` file you choose, for attaching to a report or keeping past a target change.
- **Always kept anyway**: Separately from *save*, every line is appended as it arrives to a per-target file under the shared OOPS data root (`…/logs/<target>.log`, the previous one rolled beside it at 4 MB). **open folder** reveals it. This is captured whether or not anyone is watching, so the answer to *why did it fail* survives a restart.

---

## CLI: `pros logs`

To stream console logs directly to standard output:

```bash
# Stream continuously
pros logs

# Stream and write simultaneously to a local file
pros logs | tee-object -filepath target-session.log
```

