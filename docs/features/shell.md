# Shell

Direct remote shell interaction and script execution via `shsrv` (`:2323`).

Prosperous connects directly to the target's remote root command shell, allowing operators to run diagnostics, inspect process trees, check free disk space, and manage directories without deploying a binary payload.

---

## GUI: Remote Shell Console

Open the **Shell** tab from the main navigation panel (or press `Ctrl+S`).

```text
+-------------------------------------------------------------------------------+
|  Remote Command Shell - living-room                              [_][O][X]    |
+-------------------------------------------------------------------------------+
| > uname -a                                                                    |
| FreeBSD ps5-target 12.4-RELEASE Prospero kernel x86_64                        |
| > df -h /data                                                                 |
| Filesystem    Size    Used   Avail Capacity  Mounted on                       |
| /dev/da0p7    667G    255G    412G    38%    /data                            |
| >                                                                             |
|-------------------------------------------------------------------------------|
| Command: [                                                          ] [Send]  |
+-------------------------------------------------------------------------------+
| [Clear Console]  [Export Output]                                              |
+-------------------------------------------------------------------------------+
```

![Prosperous Remote Shell](screenshots/shell.png)
*(Screenshot placeholder: Remote Shell Console)*

### GUI Controls:
- **Command Input**: Type any standard FreeBSD/Prospero shell command and hit `Enter`.
- **History Navigation**: Use `Up` / `Down` arrow keys to recall previous commands.
- **Clear Console**: Resets the shell output view.

---

## CLI: `pros sh`

Execute shell commands non-interactively or in a script:

```bash
# Query kernel version
pros sh "uname -a"

# List directory contents
pros sh "ls -la /data/homebrew"

# Inspect free storage space
pros sh "df -h"
```

