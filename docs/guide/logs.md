# Logs

The target's system log arrives over `klogsrv` (port 3232): the kernel, the system services,
and whatever a running payload or title prints. It is usually the fastest way to learn why a
payload died rather than only that it did.

The log is a stream with no end, so a reader says how long to listen. A quiet log is a result,
not a failure: it means the target had nothing to say.

## Command line

```bash
pros logs --name living-room               # listen for 10 seconds
pros logs --name living-room --seconds 60  # listen for a minute
```

Each line is printed as it arrives. When nothing arrives, `pros logs` says the log was quiet
and exits 0. Redirect or pipe the output to keep it:

```bash
pros logs --seconds 120 > session.log
```

## Window

The **log** section starts following when it is opened, once per target. Its toolbar:

| Control | Does |
|---|---|
| **follow** / **stop** | open the log and show lines as they arrive, or close the connection |
| **clear** | forget the lines shown; the log keeps arriving |
| **filter** | keep only matching lines: plain text ignoring case, or a regular expression with **regex** ticked. A pattern that does not compile shows every line and says `invalid regex` |
| line count | `90 lines`, or `10 of 90 lines` while a filter is on |
| **copy** | copy what is shown, filter and all, to the clipboard |
| **save** | write what is shown, filter and all, to a `.log` file you choose |
| **kept** / **open folder** | where every line is also written, and a button to show that folder |

When the target closes the connection the toolbar says so in words, because a log that has
ended and a log that has gone quiet look the same.

The view holds the newest 20,000 lines and draws only the rows on screen. It stays pinned to
the newest line while following.

## The kept file

Separately from **save**, every line the window receives is appended to
`logs/<target>.log` in the data directory ([getting started](getting-started.md)), whether or
not anyone is looking. When following starts and the file has reached 4 MiB, it becomes
`<target>.log.1` and a new one begins. A reason a run failed therefore survives a restart of
the window or a change of target.

## Capturing one title

To launch a title with the log already attached, so nothing it prints in its first moments is
missed, use the probe loop in [titles](titles.md).
