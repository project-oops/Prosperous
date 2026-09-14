# Files

Remote filesystem navigation, directory staging, and asset transfers over `ftpsrv` (`:2121`).

Prosperous allows complete exploration of the console's internal storage partitions (`/data/`, `/user/`, `/system/`) and robust upload/download of title directories, saved games, and packages.

---

## GUI: Remote Storage Browser

Open the **Files** tab from the main navigation panel (or press `Ctrl+F`).

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

![Prosperous Remote Storage Browser](screenshots/files.png)
*(Screenshot placeholder: Remote Storage Browser)*

### GUI Controls:
- **Breadcrumb Navigation**: Click any directory in the path bar to navigate upward immediately.
- **Upload Directory**: Upload an entire staged title folder (produced by `selfish --format title` or `make title`) directly into `/data/homebrew/`.
- **Download Selected**: Pull logs, saved game directories, or core dumps from the target to your local machine.
- **RFC 959 Robustness**: Automatically handles non-standard embedded FTP responses (such as `226 Directory created`).

---

## CLI: `pros restore`

The command line supports recursive staging and restoration:

```bash
# Upload a complete game folder to the target
pros restore build/title/GLCB00001 /data/homebrew/GLCB00001

# Retrieve a directory from the console to your PC
pros restore /data/homebrew/GLCB00001 local_backup/
```

