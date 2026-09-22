# Files

Remote filesystem navigation, directory staging, and asset transfers over `ftpsrv` (`:2121`).

Prosperous allows complete exploration of the console's internal storage partitions (`/data/`, `/user/`, `/system/`) and robust upload/download of title directories, saved games, and packages.

---

## GUI: Remote Storage Browser

Open the **Files** tab from the main navigation panel.

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

## CLI: `pros restore` and `pros backup`

The command line supports recursive staging and retrieval - `restore` uploads, `backup`
downloads:

```bash
# Upload a complete game folder to the target
pros restore build/title/GLCB00001 /data/homebrew/GLCB00001

# Re-deploy after a rebuild: only the files that changed are sent
pros restore build/title/GLCB00001 /data/homebrew/GLCB00001

# Force every file across, ignoring what was sent before
pros restore build/title/GLCB00001 /data/homebrew/GLCB00001 --all

# Retrieve a directory from the console to your PC
pros backup /data/homebrew/GLCB00001 --into local_backup/
```

**`restore` does not re-send a file it already put there unchanged.** It records what it verified
landing on each target and skips a file whose local bytes have not changed and which the target
still reports present, so re-deploying a large title after a one-file rebuild sends one file. It
never skips a changed file, and a file the target has lost is sent again; `--all` forces the whole
tree across. See [D034](../decisions/D034-a-restore-does-not-resend-an-unchanged-file.md).

