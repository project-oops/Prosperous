<p align="center">
  <img src="assets/logo.png" alt="Prosperous" width="200">
</p>

# Prosperous

**Remote Hardware Management CLI, GUI, and Transport Library for Prospero.**

Prosperous (`pros`) is the remote target management tool and communications bridge for 8th and 9th generation console software (Orbis and Prospero), written in Rust. It provides discovery, deployment, execution control, and real-time kernel telemetry streaming over standard local network sockets.

Site: **[project-oops.github.io/Prosperous](https://project-oops.github.io/Prosperous/)**

| 📖 **[User Manual & GUI Walkthrough](docs/USER_GUIDE.md)** | ⚙️ **[Technical Reference & Protocol Specs](docs/README.md)** |
| :--- | :--- |
| *CLI cheatsheet, `pros-gui` manual with wireframes, and file staging.* | *Socket protocols (shsrv, klogsrv), state machines, and transport crates.* |

---

## Role in THE LOOP

Within the [OOPS ecosystem](../docs/THE_LOOP.md), Prosperous is the **Physical Transport Bridge**:

```
Developer / Build System (oops-apps, obSCEne)
                     │
                     ▼
┌─────────────────────────────────────────────────┐
│ Prosperous (pros) CLI / Library                 │
│ - Stages title directories: pros restore        │
│ - Launches retail BIG_APPs: pros launch         │
│ - Sends bare memory payloads: pros send         │
└────────────────────┬────────────────────────────┘
                     │ LAN Sockets (2121, 9021, 3232)
                     ▼
┌─────────────────────────────────────────────────┐
│ Physical PS5 Console (192.168.1.211)            │
│ (FW 12.40 jailbroken running elfldr & ftpsrv)   │
└────────────────────┬────────────────────────────┘
                     │ Real-time Telemetry
                     ▼
┌─────────────────────────────────────────────────┐
│ pros logs (Kernel Log Streamer)                 │
│ - Streams klog to terminal and files            │
│ - Feeds silicon ground truth to obSCEne & agent │
└─────────────────────────────────────────────────┘
```

1. **Deploying Known Testbed Titles**: Transports applications built in [oops-apps](../oops-apps/) (e.g. `gl-cube`) to the `/data/homebrew/` scan root so the console OS mounts them cleanly.
2. **Executing Hardware Probes**: Pushes [obSCEne](../obscene/) conformance probes onto real silicon to settle unmeasured questions.
3. **Real-Time Telemetry**: Captures kernel diagnostics and draw completion fences directly over the network, closing the loop without needing an HDMI capture card for logs.

---

## Developer Quickstart

### 1. Build and Verify
```bash
./bin/prosperous check    # compiles crates and runs unit tests
```
The compiled CLI binary lives at `target/release/pros.exe` (Windows) or `target/release/pros` (Linux/macOS).

### 2. Common Hardware Operations

#### Target Discovery & Registration
```bash
# Check reachability of all configured targets
pros.exe check

# Register a console IP with a friendly name
pros.exe register 192.168.1.211 --name ps5
```

#### Deploy and Launch an Application
```bash
# Stage a title directory to /data/homebrew scan root
pros.exe restore oops-apps\src\gl-cube\build\title\GLCB00001 /data/homebrew/GLCB00001

# Launch the title as a retail BIG_APP
pros.exe launch GLCB00001

# Stream real-time console kernel logs
pros.exe logs --seconds 15

# Terminate running title
pros.exe close GLCB00001
```

#### Send a Bare Memory Payload
```bash
pros.exe send payload.elf
```

---

## Architecture & Crates

Prosperous is designed as a library first, with CLI and GUI frontends layered on top:

| Crate | Purpose | Dependencies |
|---|---|---|
| **`pros-link`** | Low-level target socket protocols (`elfldr` :9021, `ftpsrv` :2121, `klogsrv` :3232, `shsrv` :2323, `pldmgr` :8084). Shared by `obscene-tool` and `orbistoun`. | `std::net`, `tracing` only |
| **`pros-core`** | Target registry, payload manifest hashing, title staging, and check workflows. | `pros-link`, `serde` |
| **`pros-moonlight`** | Moonlight/GameStream host bridge in front of Porthole's ports, so any Moonlight client can pair with and stream a target (`pros moonlight`, `pros fake-target`). See [VIDEO.md](docs/VIDEO.md) part four. | `pros-link`, `rustls` |
| **`pros-cli`** | The `pros` command-line executable. | `pros-core`, `pros-moonlight`, `clap` |
| **`pros-gui`** | Native desktop window interface for visual control. | `pros-core`, `eframe` (egui) |

---

## Cross-Project Links

- **[Master OOPS Front Door](../README.md)** — Collection overview and building instructions.
- **[The OOPS Loop](../docs/THE_LOOP.md)** — Master ecosystem loop specification.
- **[oops-apps](../oops-apps/)** — Test applications deployed and supervised by Prosperous.
- **[obSCEne](../obscene/)** — Hardware conformance probe delivered by Prosperous.
- **[SELFish](../selfish/)** — Title packaging tool used prior to staging with `pros restore`.
- **[Orbistoun](../orbistoun/)** — Clean-room emulator consuming hardware telemetry.
