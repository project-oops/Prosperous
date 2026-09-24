# Prosperous User Manual & GUI Guide

Welcome to the **Prosperous** user manual.

This guide provides complete, step-by-step operational instructions for both the **`pros` command-line interface** and the **`pros-gui` desktop application**, designed for homebrew operators, game testers, and developers managing physical console hardware.

For the socket protocols, the transport crates and the decision records, see the **[Technical Reference](README.md)** and **[DECISIONS.md](DECISIONS.md)**.

---

## Table of Contents

1. [Quickstart: Connecting to Your Target](#1-quickstart-connecting-to-your-target)
2. [CLI Command Reference (`pros`)](#2-cli-command-reference-pros)
   - [Checking Target Health (`pros check`)](#checking-target-health-pros-check)
   - [Staging & Transferring Files (`pros restore`)](#staging--transferring-files-pros-restore)
   - [Launching & Supervising Titles (`pros launch`)](#launching--supervising-titles-pros-launch)
   - [Streaming Kernel Logs (`pros logs`)](#streaming-kernel-logs-pros-logs)
   - [Executing Shell Commands (`pros sh`)](#executing-shell-commands-pros-sh)
3. [`pros-gui` Desktop Window Walkthrough](#3-pros-gui-desktop-window-walkthrough)
   - [Target Browser & Health Matrix](#a-target-browser--health-matrix)
   - [Live Kernel Telemetry Streamer](#b-live-kernel-telemetry-streamer)
   - [Remote File Explorer & Title Stager](#c-remote-file-explorer--title-stager)
   - [Title Launcher & Process Supervisor](#d-title-launcher--process-supervisor)
4. [Troubleshooting & Network Diagnostics](#4-troubleshooting--network-diagnostics)

---

## 1. Quickstart: Connecting to Your Target

Prosperous stores registrations in `targets.txt` at the shared OOPS data root - `%APPDATA%\OOPS\` (on Windows) or `~/.local/share/OOPS/` (on Linux) - so a console registered here is one Orbistoun and obSCEne can reach too.

### 1. Register a Target Console
```powershell
pros register 192.168.1.211 --name living-room
```

### 2. Verify Available Targets
```powershell
pros list
```

---

## 2. CLI Command Reference (`pros`)

### Checking Target Health (`pros check`)
Whenever communication is uncertain, run `pros check`. It tests all 5 standard services:

```powershell
pros check --name living-room
```

```text
living-room (192.168.1.211)
  up   elfldr    :9021  send a payload to the target and run it
  up   ftpsrv    :2121  retrieve reports, stage payloads and packages
  up   klogsrv   :3232  read the system's own log - why a payload died, not just that it did
  up   shsrv     :2323  run commands on the target without loading a payload
  up   pldmgr    :8084  inspect and reload the payload chain

ready
```

Each row says **what the service unlocks**, not just whether the port is open. A service that is
down but required is marked `DOWN`; an optional one that is down is marked `--`; and a port that
answered slowly carries its timing, e.g. `(1508ms)`. The last line is a sentence, not a status
word: `ready`, or `usable, but ...`, or a blocked verdict that names the remedy.

---

### Staging & Transferring Files (`pros restore`)
Upload titles, packages, or raw assets to the target's internal storage via FTP:

```powershell
# Upload a complete game title folder into /data/homebrew/
pros restore build/title/GLCB00001 /data/homebrew/GLCB00001

# Copy a single package file into /data/pkg/
pros restore build/game.pkg /data/pkg/game.pkg

# Force every file across, ignoring what was sent last time
pros restore build/title/GLCB00001 /data/homebrew/GLCB00001 --all
```

`restore` skips a file it already put there unchanged - it records what it verified landing on each
target and re-sends only what differs, so a rebuild that changed one file uploads one file. It never
skips a changed file, and `--all` forces the whole tree across. See
[D034](decisions/D034-a-restore-does-not-resend-an-unchanged-file.md).

---

### Launching & Supervising Titles (`pros launch`)
Launch an installed or staged title by its Title ID:

```powershell
pros launch GLCB00001
```

To close a running title:
```powershell
pros close GLCB00001
```

---

### Streaming Kernel Logs (`pros logs`)
Stream live kernel and application telemetry (`klog`) directly to your terminal:

```powershell
# Stream continuously to stdout
pros logs

# Stream and simultaneously pipe to a local file
pros logs | tee-object -filepath ps5-session.log
```

---

### Executing Shell Commands (`pros sh`)
Execute remote commands directly on the target without deploying an ELF:

```powershell
pros sh "ls -la /data/homebrew"
```

---

## 3. `pros-gui` Desktop Window Walkthrough

Launch the desktop interface with:
```powershell
pros-gui
```

### A. Target Browser & Health Matrix

The main dashboard provides real-time reachability monitoring for all registered hardware:

```text
+-------------------------------------------------------------------------------+
|  Prosperous Target Browser                                       [_][O][X]    |
+-------------------------------------------------------------------------------+
| Targets           | Status | elfldr | ftpsrv | klog | shsrv | pldmgr | Action |
|-------------------+--------+--------+--------+------+-------+--------+--------|
| (*) living-room   | ONLINE |   OK   |   OK   |  OK  |  OK   |   OK   | [Open] |
| ( ) lab-ps5-pro   | OFFLINE|   --   |   --   |  --  |  --   |   --   | [Poll] |
+-------------------------------------------------------------------------------+
| Selected Target: living-room (192.168.1.211)                                  |
| OS: Prospero FW 12.40 | Storage: 412.8 GiB Free                               |
| [ + Register New Target ]   [ Refresh Health ]   [ Open Remote Shell ]        |
+-------------------------------------------------------------------------------+
```

![Prosperous Target Browser UI](screenshots/pros_gui_targets.png)
*(Screenshot placeholder: Target Browser & Health Matrix)*

---

### B. Live Kernel Telemetry Streamer

Stream, filter, and search real-time system logs with zero buffering:

```text
+-------------------------------------------------------------------------------+
|  Console Kernel Log (living-room)                                [_][O][X]    |
+-------------------------------------------------------------------------------+
| follow/stop  clear   filter: [ sceAgc          ] [ ] regex   copy  save  folder |
|-------------------------------------------------------------------------------|
| 118: <118>[SceSystemStateMgr] Power Mode Change: BIG_APP                      |
| 119: <118>[SceLncService] SetControllerFocus(0x00006018)                      |
| 120: <118>[AvControl] entry process(appid:0x6018 pid:0xbf)                    |
| 121: [OOPS-GL] creating AGC universal queue...                                |
| 122: [OOPS-GL] sceAgcDriverCreateQueue rc: 0x0                                |
| 123: [OOPS-GL] hardware AGC RDNA2 pipeline initialized successfully           |
| 124: [OOPS-GL] flush-words: 0x39b submit-rc: 0x0 fence-hit: 0x1              |
+-------------------------------------------------------------------------------+
```

![Prosperous Kernel Log Streamer UI](screenshots/pros_gui_logs.png)
*(Screenshot placeholder: Live Kernel Telemetry Streamer)*

The **filter** matches plain text (case-insensitive) or, with the **regex** box ticked, a regular
expression; an invalid pattern shows every line and says so. **save** writes what is shown (filter
and all) to a `.log` you choose, and **folder** opens the always-on per-target capture. The view
holds up to 20,000 lines and draws only the rows on screen, so a long watch stays smooth; the full
history is the kept file.

---

### C. Remote File Explorer & Title Stager

Drag-and-drop file browser connected directly to the console's `/data/` and `/user/` partitions:

```text
+-------------------------------------------------------------------------------+
|  Remote Storage Browser - living-room                            [_][O][X]    |
+-------------------------------------------------------------------------------+
| Path: /data/homebrew/GLCB00001/                                               |
|-------------------------------------------------------------------------------|
| Name                 | Size      | Type      | Permissions | Modified         |
|----------------------+-----------+-----------+-------------+------------------|
| [..]                 | --        | Directory | drwxr-xr-x  | 2026-09-14 10:00 |
| eboot.bin            | 412.5 KiB | Executable| -rwxr-xr-x  | 2026-09-14 11:30 |
| sce_module/          | --        | Directory | drwxr-xr-x  | 2026-09-14 11:30 |
|   libc.prx           | 128.0 KiB | Shared Lib| -rwxr-xr-x  | 2026-09-14 11:30 |
| sce_sys/             | --        | Directory | drwxr-xr-x  | 2026-09-14 11:30 |
|   param.json         | 1.2 KiB   | Metadata  | -rw-r--r--  | 2026-09-14 11:30 |
|   icon0.png          | 64.0 KiB  | Image     | -rw-r--r--  | 2026-09-14 11:30 |
+-------------------------------------------------------------------------------+
| [ Upload Directory... ]  [ Download Selected ]  [ Delete ]  [ Launch Title ]  |
+-------------------------------------------------------------------------------+
```

![Prosperous File Explorer UI](screenshots/pros_gui_files.png)
*(Screenshot placeholder: Remote File Explorer)*

---

### D. Title Launcher & Process Supervisor

View currently running applications, active PIDs, and kill or restart titles with one click:

```text
+-------------------------------------------------------------------------------+
|  Title Supervisor (living-room)                                  [_][O][X]    |
+-------------------------------------------------------------------------------+
| Running Titles:                                                               |
|   * GLCB00001 - "GL-Cube 3D Demo" (AppID: 0x6018, PID: 191) [RUNNING]        |
|                                                                               |
| Staged Titles (/data/homebrew):                                               |
|   [Launch] GLCB00001  - GL-Cube 3D Demo (v1.00)                               |
|   [Launch] OBSC00001  - obSCEne Conformance Suite (v2.4)                       |
|   [Launch] WIPE00001  - WipEout Model Viewer (v1.00)                          |
+-------------------------------------------------------------------------------+
| [ Terminate Running Title (SIGKILL) ]   [ Relaunch (F5) ]                     |
+-------------------------------------------------------------------------------+
```

![Prosperous Title Supervisor UI](screenshots/pros_gui_titles.png)
*(Screenshot placeholder: Title Supervisor UI)*

The system panel lists every process with its memory (the figure the target's own `ps` reports; no
CPU column, because it reports none), a *close* on each title and an *end*-by-pid on everything
else, plus *sort* and an opt-in *auto-refresh*. On the command line the same lives in `pros ps`,
`pros top` (the live, refreshing view), `pros close` and `pros kill`.

---

## 4. Troubleshooting & Network Diagnostics

### Target Unreachable / Timeout
1. Verify host and PS5 are on the same subnet (e.g. `192.168.1.x`).
2. Verify Wi-Fi / Ethernet connection status on the console settings.
3. Test connectivity with `ping 192.168.1.211`.

### "Directory creation refused: 226"
- Ensure you have built the latest `pros` release. Our updated client accepts all RFC 959 2xx success replies from embedded console FTP daemons.

### "elfldr down (Connection refused: 9021)"
- The console was restarted and the volatile jailbreak payload is inactive. Re-run the browser jailbreak stage to restart `elfldr`.

