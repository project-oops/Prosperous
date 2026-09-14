# Prosperous User Guide

Cross-cutting reference for how Prosperous behaves under the hood — where files live, portable mode, how target communication is routed, zero-telemetry privacy, and daemon ports. Per-feature pages live alongside; this page addresses system-wide behavior.

---

## Paths and Portable Mode

Prosperous persists target registrations and session state to a root directory resolved at startup. There are two operational modes.

### Default Mode: `%APPDATA%\OOPS\` (Windows) or `~/.local/share/OOPS/` (Linux)

Shared across all OOPS projects, so registering a console in Prosperous makes it immediately available to Orbistoun and obSCEne without duplicate configuration.

```text
%APPDATA%\OOPS\ (Windows) or ~/.local/share/OOPS/ (Linux)
    targets.txt         <- Registered consoles and port overrides (shared)
    prosperous/
        history.log     <- Command execution history
        downloads/      <- Retrieved logs, dumps, and saved data
```

### Portable Mode

Drop a `.portable` directory (or sentinel file) next to the Prosperous executable, or set `PROS_PORTABLE=1` / `OOPS_PORTABLE=1`:

```text
<wherever you put it>/
    pros.exe            (or pros-gui.exe)
    .portable           (sentinel directory or file)
    targets.txt         (stored right beside the binary)
```

In portable mode, **zero data is written to the host user profile**. Everything stays self-contained on a USB stick or portable directory. Portable mode is also automatically activated if the executable name contains `portable` (e.g. `pros-portable.exe`).

---

## Network Architecture & Target Daemons

Prosperous communicates directly with standard homebrew services running on jailbroken hardware:

| Daemon | Port | Role in Prosperous |
| :--- | :--- | :--- |
| **`elfldr`** | `9021` | Injects bare ELF memory payloads directly into target RAM. |
| **`ftpsrv`** | `2121` | Remote filesystem access for staging titles, packages, and saves. |
| **`klogsrv`** | `3232` | Raw broadcast socket streaming unbuffered kernel and user `klog`. |
| **`shsrv`** | `2323` | Remote root command shell. |
| **`pldmgr`** | `8084` | Manages resident daemon chains and payload reloads. |

---

## Data & Privacy

Prosperous is strictly **local-first**:
- **Zero Telemetry**: Prosperous never phones home. No crash reporting, analytics, or tracking pings.
- **No Cloud Accounts**: All target credentials and addresses remain strictly on your local machine.
- **Direct LAN Communication**: All traffic flows directly between your PC and the target console on your local network.

