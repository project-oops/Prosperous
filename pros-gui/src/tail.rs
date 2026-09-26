//! Following the log, for as long as somebody wants to watch it.
//!
//! A log is a subscription with no end, so it runs on its own thread and channel beside the
//! worker: as a job it would block every other request while open, and a bounded read would
//! miss lines between reads. Dropping the [`Tail`] shuts the socket, because the thread is
//! blocked inside a read on a quiet log and would never see a flag.

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread;

/// How many lines are kept on screen.
///
/// Scrollback only: every line is also appended to the kept file ([`kept_at`]). The view lays
/// out only visible rows, so a large buffer costs the filter pass, not the render. The oldest
/// lines are dropped first.
pub(crate) const KEPT: usize = 20_000;

/// A log being followed.
pub(crate) struct Tail {
    /// Which target it is attached to, so a change of target can end it.
    pub(crate) target: String,
    lines: Receiver<String>,
    /// The handle that ends the follow.
    stopper: pros_link::log::Stopper,
    /// Whether the far end has gone.
    ended: bool,
}

impl std::fmt::Debug for Tail {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Tail")
            .field("target", &self.target)
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

impl Tail {
    /// Opens the log and starts following it.
    ///
    /// # Errors
    ///
    /// When the log service is not answering, which is ordinary for a target that has just
    /// come back; the service is optional.
    pub(crate) fn start(name: &str, source: &pros_link::Link) -> Result<Self, String> {
        let (stopper, reading) = pros_link::log::follow(source).map_err(|why| why.to_string())?;

        // Also written to disk, on the reading thread, so the log survives a target change or
        // a restart of the window.
        let mut kept = keeping(name);
        let (sender, lines) = channel();
        thread::spawn(move || {
            for line in reading {
                let Ok(line) = line else {
                    break;
                };
                if let Some(file) = kept.as_mut() {
                    // A write failure stops the writing, not the following.
                    use std::io::Write as _;
                    if writeln!(file, "{line}").is_err() {
                        kept = None;
                    }
                }
                // The window has gone.
                if sender.send(line).is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            target: name.to_owned(),
            lines,
            stopper,
            ended: false,
        })
    }

    /// Takes whatever has arrived since last time into `into`.
    ///
    /// Returns whether anything did, so the window repaints only when there is something new.
    pub(crate) fn drain(&mut self, into: &mut Vec<String>) -> bool {
        let mut had = false;
        loop {
            match self.lines.try_recv() {
                Ok(line) => {
                    into.push(line);
                    had = true;
                }
                Err(TryRecvError::Empty) => break,
                // The far end closed; recorded so it is not mistaken for a quiet log.
                Err(TryRecvError::Disconnected) => {
                    self.ended = true;
                    break;
                }
            }
        }
        if into.len() > KEPT {
            into.drain(..into.len() - KEPT);
        }
        had
    }

    /// Whether the connection has closed on its own.
    pub(crate) const fn has_ended(&self) -> bool {
        self.ended
    }
}

impl Drop for Tail {
    fn drop(&mut self) {
        // Shutting the connection makes the blocked read return, which ends the thread.
        self.stopper.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::Tail;

    /// Attaching to a target that is not there fails rather than looking like a quiet log.
    #[test]
    fn a_log_that_is_not_answering_does_not_look_like_a_quiet_one() {
        // Port 9 discards and nothing listens on it here; the connection is refused.
        let refused = Tail::start("nowhere", &pros_link::Link::to("127.0.0.1:9"));
        assert!(refused.is_err(), "it should not have connected");
    }

    /// The buffer is bounded, and it is the oldest lines that go.
    #[test]
    fn a_long_watch_forgets_the_oldest_lines() {
        let mut kept: Vec<String> = (0..super::KEPT + 10)
            .map(|at| format!("line {at}"))
            .collect();
        // The same trim `drain` applies, checked directly: the reading side needs a socket.
        if kept.len() > super::KEPT {
            kept.drain(..kept.len() - super::KEPT);
        }
        assert_eq!(kept.len(), super::KEPT);
        assert_eq!(kept[0], "line 10");
    }
}

/// How big one target's log is allowed to get before the previous one is displaced.
///
/// Two files: the current log and the one before it, enough to compare this boot with the last.
const ROLL_AT: u64 = 4 * 1024 * 1024;

/// Where a target's log is kept.
///
/// Beside the registry and the manifest, one file per target.
#[must_use]
pub(crate) fn kept_at(target: &str) -> Option<std::path::PathBuf> {
    let mut path = pros_core::target::directory()?;
    path.push("logs");
    // A registered name is user text; anything but `[A-Za-z0-9_-]` becomes `_` so it cannot
    // name a path outside the directory.
    let safe: String = target
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    path.push(format!("{safe}.log"));
    Some(path)
}

/// Opens the file to append to, rolling the previous one out of the way when it is large.
///
/// `None` when there is nowhere to write, which stops the keeping and nothing else.
fn keeping(target: &str) -> Option<std::fs::File> {
    let path = kept_at(target)?;
    std::fs::create_dir_all(path.parent()?).ok()?;
    if std::fs::metadata(&path).is_ok_and(|about| about.len() >= ROLL_AT) {
        let _ = std::fs::rename(&path, path.with_extension("log.1"));
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
}

#[cfg(test)]
mod keeping_tests {
    use super::kept_at;

    /// A registered name cannot place the log outside the log directory.
    #[test]
    fn a_name_cannot_escape_the_log_directory() {
        let Some(path) = kept_at("../../etc/passwd") else {
            return;
        };
        let file = path.file_name().expect("a file name").to_string_lossy();
        assert!(!file.contains(".."), "{file}");
        assert!(!file.contains('/') && !file.contains('\\'), "{file}");
        assert!(file.ends_with(".log"), "{file}");
        assert!(path.parent().is_some_and(|at| at.ends_with("logs")));
    }

    /// An ordinary name is used unchanged, so the file is easy to find.
    #[test]
    fn an_ordinary_name_is_kept_as_it_is() {
        let Some(path) = kept_at("ps5") else {
            return;
        };
        assert!(path.ends_with("ps5.log"), "{}", path.display());
    }
}
