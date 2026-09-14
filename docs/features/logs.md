# Logs

Real-time streaming and inspection of console kernel and user-space telemetry (`klog`).

Every print from the operating system kernel, system daemons, and running homebrew titles is broadcast unbuffered over port `3232`. Prosperous connects to this socket and presents a live, searchable stream.

---

## GUI: Live Telemetry Streamer

Open the **Logs** tab from the main navigation panel (or press `Ctrl+L`).

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
| [Pause Stream]  [Save Log to File...]  [Export Filtered View]                 |
+-------------------------------------------------------------------------------+
```

![Prosperous Live Telemetry Streamer](screenshots/logs.png)
*(Screenshot placeholder: Live Kernel Telemetry Streamer)*

### GUI Controls:
- **Filter Box**: Real-time regex and substring search. Filters lines instantly without dropping unseen messages from the background buffer.
- **Auto-Scroll Toggle**: Locks viewport to the bottom of the log stream as new lines arrive.
- **Pause Stream**: Freezes rendering to allow careful inspection of high-frequency debug bursts.
- **Save Log to File**: Dumps the complete session buffer to a timestamped `.log` file in `%APPDATA%\OOPS\prosperous\downloads\`.

---

## CLI: `pros logs`

To stream console logs directly to standard output:

```bash
# Stream continuously
pros logs

# Stream and write simultaneously to a local file
pros logs | tee-object -filepath target-session.log
```

