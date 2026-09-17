# Titles

Title launch supervision, process monitoring, and lifecycle control.

Prosperous interfaces with platform launch daemons and shell services to start applications in full retail `BIG_APP` context, verify screen focus, and terminate processes cleanly.

---

## GUI: Title Supervisor

Open the **Titles** tab from the main navigation panel.

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

![Prosperous Title Supervisor](screenshots/titles.png)
*(Screenshot placeholder: Title Supervisor)*

### GUI Controls:
- **Running Titles Panel**: Shows the currently focused game title ID, its assigned `AppID`, operating system `PID`, and status.
- **Staged Titles List**: Automatically scans `/data/homebrew/` for valid titles, reading `param.json` to present human-readable title names and versions.
- **Launch Button**: Dispatches launch request to the target, triggering the OS `Power Mode Change: BIG_APP` transition and transferring HDMI controller focus.
- **Terminate Running Title**: Sends clean termination signal to the active process without crashing the console kernel.

---

## CLI: `pros launch` & `pros close`

```bash
# Launch a title by Title ID
pros launch GLCB00001

# Close a running title
pros close GLCB00001
```

