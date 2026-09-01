//! Following the log, for as long as somebody wants to watch it.
//!
//! # Why this is not a job
//!
//! Every other thing this window asks is a request: it goes, it comes back, and while it is
//! out nothing else may run - which is right, because two of them racing would interleave
//! their answers.
//!
//! A log is not that. It is a subscription with no end, and putting it through the same rule
//! would mean **the whole program is unusable for as long as somebody is watching the log** -
//! no checking, no browsing, no sending the payload whose failure they are reading about.
//!
//! It was a five-second window for exactly that reason: a request has to finish. But five
//! seconds of reading is five seconds of not reading, and a line that arrives in the gap is a
//! line nobody sees. For a log that is the whole failure - its only job is to say what
//! happened when something went wrong, and the moment something goes wrong is the moment the
//! gap matters.
//!
//! So this runs beside the worker rather than inside it, on its own thread, with its own
//! channel.
//!
//! # Stopping
//!
//! Dropping it. The thread is blocked reading a socket and cannot be asked anything, so the
//! socket is shut down under it - the read returns, the loop ends, the thread exits. **A flag
//! it checked between lines would never be looked at**, because a quiet log means it is
//! blocked inside the read and not between anything.

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread;

/// How many lines are kept.
///
/// **A bound rather than a belief.** A log left running all afternoon would otherwise grow
/// without limit, and the oldest lines are the ones nobody is scrolled to.
const KEPT: usize = 2000;

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
    /// When the log service is not answering, which is the ordinary state of a target that
    /// has just come back - it is optional, and its absence costs visibility rather than
    /// capability.
    pub(crate) fn start(name: &str, source: &pros_link::Link) -> Result<Self, String> {
        let (stopper, reading) = pros_link::log::follow(source).map_err(|why| why.to_string())?;

        // **Kept on disk as well as on screen.** A log is read to find out why something
        // failed, and the answer is usually wanted *after* the thing failed - by which point
        // an in-memory list has been cleared by a target change or lost to a restart. Written
        // on the reading thread so it captures what arrived whether or not anybody was
        // looking, and whether or not the window survives.
        let mut kept = keeping(name);
        let (sender, lines) = channel();
        thread::spawn(move || {
            for line in reading {
                let Ok(line) = line else {
                    break;
                };
                if let Some(file) = kept.as_mut() {
                    // A log that cannot be written is still a log worth watching, so a failure
                    // here stops the writing and not the following.
                    use std::io::Write as _;
                    if writeln!(file, "{line}").is_err() {
                        kept = None;
                    }
                }
                // The window has gone: nobody to tell, nothing to do about it.
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
    /// Returns whether anything did, so the window can repaint only when there is something
    /// to repaint.
    pub(crate) fn drain(&mut self, into: &mut Vec<String>) -> bool {
        let mut had = false;
        loop {
            match self.lines.try_recv() {
                Ok(line) => {
                    into.push(line);
                    had = true;
                }
                Err(TryRecvError::Empty) => break,
                // **The far end closed.** Said rather than shown as a log that has simply gone
                // quiet: those look identical on screen and mean opposite things.
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
        // The reading thread is blocked inside a read and cannot be asked to stop. Shutting
        // the connection makes that read return, which ends the loop and the thread.
        self.stopper.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::Tail;

    /// **Attaching to a target that is not there fails rather than appearing to work.**
    ///
    /// A tail that opened against nothing and then sat silent is indistinguishable from a
    /// target with a quiet log, which is the one thing this must not be.
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
/// **Two files and a cap, rather than a scheme.** Rotation with numbered generations is a
/// thing to get wrong; this keeps the current log and the one before it, which is what covers
/// *"it worked last boot and not this one"* - the question a kept log is actually for.
const ROLL_AT: u64 = 4 * 1024 * 1024;

/// Where a target's log is kept.
///
/// Beside the registry and the manifest, so somebody looking for any of this project's files
/// finds all of them - and named for the target, because two of them are two logs.
#[must_use]
pub(crate) fn kept_at(target: &str) -> Option<std::path::PathBuf> {
    let mut path = pros_core::target::directory()?;
    path.push("logs");
    // A registered name is somebody's text and reaches a filename here, so anything that is
    // not plainly a name becomes an underscore rather than a directory somewhere else.
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

    /// **A registered name reaches a filename**, so it is reduced to something that cannot be
    /// a path of its own.
    #[test]
    fn a_name_cannot_escape_the_log_directory() {
        let Some(path) = kept_at("../../etc/passwd") else {
            return;
        };
        let file = path.file_name().expect("a file name").to_string_lossy();
        // The properties, not the exact spelling: nothing that could climb out of the
        // directory or point at another one survives.
        assert!(!file.contains(".."), "{file}");
        assert!(!file.contains('/') && !file.contains('\\'), "{file}");
        assert!(file.ends_with(".log"), "{file}");
        assert!(path.parent().is_some_and(|at| at.ends_with("logs")));
    }

    /// An ordinary name is left alone, because mangling one nobody needed mangled makes the
    /// file harder to find than the thing it was protecting against.
    #[test]
    fn an_ordinary_name_is_kept_as_it_is() {
        let Some(path) = kept_at("ps5") else {
            return;
        };
        assert!(path.ends_with("ps5.log"), "{}", path.display());
    }
}
