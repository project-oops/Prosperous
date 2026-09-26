//! Watching the stand-in stream: read it, count it, and pipe it to a player.
//!
//! Decoding stays out of this crate (`docs/VIDEO.md`, Porthole): it would need a large C or
//! C++ dependency through FFI in a workspace that forbids unsafe code. A player alone says
//! "no picture" for several different faults, so the bytes pass through here on their way to
//! the player's standard input, and the counts say which fault it is.
//!
//! The pump runs on its own thread because the socket blocks; the window reads a snapshot of
//! the counters.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The port the stand-in serves video on (`docs/VIDEO.md`, Porthole).
///
/// Both ends are ours, so the value is a choice rather than a measurement.
pub const PORT: u16 = 9805;

/// How long to wait for a target to accept.
pub const PATIENCE: Duration = Duration::from_secs(3);

/// How much to read at once.
///
/// Sized to be a useful write to a player, not to match anything about the codec.
const MOUTHFUL: usize = 32 * 1024;

/// How long a rate is measured over.
///
/// Short enough that a stall shows while somebody is looking, long enough not to jitter.
const WINDOW: Duration = Duration::from_secs(1);

/// How long a read waits before looping.
///
/// A timeout here is a pause, not a failure: a stream between frames produces it. It is short
/// because the loop also closes the rate window, so a stall shows within about a second.
/// The read reports it as `WouldBlock` on Unix and `TimedOut` on Windows; the pump takes both.
const BREATH: Duration = Duration::from_millis(500);

/// What the watcher is doing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Status {
    /// Nothing has been started.
    #[default]
    Idle,
    /// Connected, and bytes are going through.
    Watching,
    /// It ended, for this reason.
    ///
    /// Includes ending cleanly, which is distinct from never having started.
    Ended(String),
    /// It could not start at all.
    Failed(String),
}

impl Status {
    /// Whether bytes are moving.
    #[must_use]
    pub const fn is_watching(&self) -> bool {
        matches!(self, Self::Watching)
    }

    /// How to put it to somebody.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Idle => "not watching".to_owned(),
            Self::Watching => "watching".to_owned(),
            Self::Ended(why) => format!("the stream ended: {why}"),
            Self::Failed(why) => format!("could not start: {why}"),
        }
    }
}

/// What has gone past.
#[derive(Debug, Clone, Default)]
pub struct Counts {
    /// Where it has got to.
    pub status: Status,
    /// Bytes read from the target.
    pub bytes: u64,
    /// Whole units seen.
    pub units: u64,
    /// How many of those a decoder could have started from.
    pub keyframes: u64,
    /// Bytes held, waiting for a boundary that has not arrived.
    pub pending: usize,
    /// Whether the player is still running.
    pub player_alive: bool,
    /// Bytes a second, over the last window.
    ///
    /// `None` until a window has closed, which is not the same as zero: zero is a stalled
    /// stream, `None` is one not watched long enough to have a rate.
    pub rate: Option<Rate>,
}

/// How fast it is arriving, over one window.
///
/// Cumulative counts climb for a stream and for a slideshow alike; only a rate separates them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rate {
    /// Bytes a second.
    pub bytes: f64,
    /// Units a second - frames, near enough, for this purpose.
    pub units: f64,
}

impl Rate {
    /// How to put it to somebody.
    #[must_use]
    pub fn describe(&self) -> String {
        // A stall is said in words, because `0.0/s` reads as a figure rather than a fault.
        if self.units < 0.05 && self.bytes < 1.0 {
            return "nothing arriving".to_owned();
        }
        format!("{:.1}/s  {}/s", self.units, size(self.bytes))
    }

    /// Whether this is a stream rather than a slideshow.
    ///
    /// Ten a second is well under any real frame rate and well over what raw grabs manage, so
    /// it separates the two designs in `docs/VIDEO.md` rather than grading the picture.
    #[must_use]
    pub fn is_moving(&self) -> bool {
        self.units >= 10.0
    }
}

/// Bytes as something readable.
#[must_use]
fn size(bytes: f64) -> String {
    if bytes >= 1_000_000.0 {
        format!("{:.1} MB", bytes / 1_000_000.0)
    } else if bytes >= 1_000.0 {
        format!("{:.0} kB", bytes / 1_000.0)
    } else {
        format!("{bytes:.0} B")
    }
}

impl Counts {
    /// What to tell somebody looking at a window with no picture in it.
    ///
    /// Separates the causes a player cannot: nothing arrived; bytes arrived and none framed;
    /// units arrived with no keyframe; everything arrived and the player is gone; or the
    /// stream arrives too slowly to be one.
    #[must_use]
    pub fn diagnose(&self) -> Option<String> {
        if !self.status.is_watching() {
            return None;
        }
        if self.bytes == 0 {
            return Some("connected, and nothing has arrived yet".to_owned());
        }
        if self.units == 0 {
            return Some(format!(
                "{} bytes arrived and none of it framed - this is not the stream this reads",
                self.bytes
            ));
        }
        if self.keyframes == 0 {
            return Some(format!(
                "{} units and no keyframe - a decoder has nothing to start from, which looks \
                 exactly like no stream at all",
                self.units
            ));
        }
        if !self.player_alive {
            return Some("the stream is fine and the player has gone".to_owned());
        }
        if let Some(rate) = self.rate
            && !rate.is_moving()
        {
            // Every check above passes and the counts still climb; this is what the raw-grab
            // fallback in `docs/VIDEO.md` (Diffing) looks like, about two frames a second.
            return Some(format!(
                "arriving at {} - that is not a stream, it is a slideshow",
                rate.describe()
            ));
        }
        None
    }
}

/// A running watch.
#[derive(Debug)]
pub struct Watching {
    counts: Arc<Mutex<Counts>>,
    stopping: Arc<AtomicBool>,
    /// How long a rate is measured over, for the pump this handle drives.
    ///
    /// Held rather than taken from [`WINDOW`] so a test can state its own premise. See
    /// [`Watching::idle_measuring_over`].
    window: Duration,
}

impl Default for Watching {
    fn default() -> Self {
        Self::idle()
    }
}

impl Watching {
    /// A watcher that has not been started.
    #[must_use]
    pub fn idle() -> Self {
        Self::idle_measuring_over(WINDOW)
    }

    /// The same, with the rate window stated rather than assumed.
    ///
    /// A test that depends on a run finishing inside one window would depend on machine load;
    /// naming the window makes that premise hold by construction.
    #[must_use]
    pub fn idle_measuring_over(window: Duration) -> Self {
        Self {
            counts: Arc::new(Mutex::new(Counts::default())),
            stopping: Arc::new(AtomicBool::new(false)),
            window,
        }
    }

    /// Where it has got to.
    ///
    /// A snapshot rather than a borrow, so the window never holds the pump's lock while it
    /// draws.
    #[must_use]
    pub fn counts(&self) -> Counts {
        self.counts
            .lock()
            .map(|held| held.clone())
            .unwrap_or_default()
    }

    /// Asks the pump to stop.
    ///
    /// Asks rather than kills, so the player is closed the way it expects and the socket is
    /// shut rather than abandoned.
    pub fn stop(&self) {
        self.stopping.store(true, Ordering::Relaxed);
    }

    /// Whether this watch has been asked to stop.
    #[must_use]
    pub fn stopping(&self) -> bool {
        self.stopping.load(Ordering::Relaxed)
    }

    /// Connects, starts a player, and pumps the stream into it.
    ///
    /// Returns immediately; everything happens on a thread and shows up in [`Watching::counts`].
    #[must_use]
    pub fn start(address: &str, port: u16, template: &str) -> Self {
        let watching = Self::idle();
        let counts = Arc::clone(&watching.counts);
        let stopping = Arc::clone(&watching.stopping);
        let address = address.to_owned();
        let template = template.to_owned();

        std::thread::spawn(move || {
            pump(&address, port, &template, &counts, &stopping);
        });
        watching
    }
}

/// Splits a configured command line into a program and its arguments.
///
/// Split on spaces only - not a shell: no quoting, no expansion, no pipelines. A command that
/// needs those belongs in a script.
///
/// # Errors
///
/// When the line is empty once comments and spaces are gone, since launching nothing quietly
/// would look like launching something that failed.
pub fn parts(template: &str, address: &str) -> crate::Result<(String, Vec<String>)> {
    let filled = template.replace("{address}", address);
    // Demuxer h264 was absorbed into libavformat in modern mpv; translate legacy configurations.
    let mut words: Vec<String> = filled
        .split_whitespace()
        .map(|word| {
            if word == "--demuxer=h264" {
                "--demuxer-lavf-format=h264".to_owned()
            } else {
                word.to_owned()
            }
        })
        .collect();
    let program = words.first().cloned().ok_or_else(|| {
        crate::Error::failed(format!(
            "nothing to run - put a command in {}",
            command_path().map_or_else(
                || "the configuration".to_owned(),
                |path| path.display().to_string()
            )
        ))
    })?;
    words.remove(0);
    // mpv stays invisible until a frame decodes; force the window open immediately
    // and keep it alive across stream gaps or decoding errors.
    if program == "mpv" || program == "mpv.exe" {
        if !words.iter().any(|w| w.starts_with("--force-window")) {
            let pos = words.len().saturating_sub(1);
            words.insert(pos, "--force-window=immediate".to_owned());
        }
        if !words.iter().any(|w| w.starts_with("--title")) {
            let pos = words.len().saturating_sub(1);
            words.insert(pos, "--title=Porthole-Stream".to_owned());
        }
        if !words.iter().any(|w| w.starts_with("--idle")) {
            let pos = words.len().saturating_sub(1);
            words.insert(pos, "--idle=yes".to_owned());
        }
        if !words.iter().any(|w| w.starts_with("--keep-open")) {
            let pos = words.len().saturating_sub(1);
            words.insert(pos, "--keep-open=yes".to_owned());
        }
    }
    Ok((program, words))
}

/// Where the player command is kept.
#[must_use]
pub fn command_path() -> Option<PathBuf> {
    let mut path = crate::target::directory()?;
    path.push("player.txt");
    Some(path)
}

/// The command, when one has been written down.
#[must_use]
pub fn configured() -> Option<String> {
    let text = std::fs::read_to_string(command_path()?).ok()?;
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))?;
    Some(line.to_owned())
}

/// What to write into that file so somebody can edit it.
#[must_use]
pub fn example() -> String {
    // `-` reads standard input; the low-latency options stop the player buffering seconds of
    // a live stream before showing anything.
    "# The player the stand-in stream is piped into. One line, split on spaces.\n\
     #\n\
     # It reads the stream on its standard input, so the last word is usually a dash. The\n\
     # low-latency options matter: a player left to itself buffers seconds of a live stream\n\
     # before showing anything, which reads as a stream that is not working.\n\
     #\n\
     # This project does not decode video. It pipes it to something that does, counts what\n\
     # went past, and can therefore say which of several reasons there is no picture.\n\
     mpv --demuxer-lavf-format=h264 --profile=low-latency --untimed --no-cache --force-window=immediate --title=Porthole-Stream --idle=yes --keep-open=yes -\n"
        .to_owned()
}

/// Writes the example, without overwriting one somebody has edited.
///
/// # Errors
///
/// When the file cannot be written.
pub fn write_example() -> crate::Result<PathBuf> {
    let path = command_path()
        .ok_or_else(|| crate::Error::failed("no home directory, so there is nowhere to keep it"))?;
    if path.exists() {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, example())?;
    Ok(path)
}

/// Where a vendored copy of mpv is kept under the application data directory.
///
/// In a portable run this sits in `.portable/tools/mpv`; otherwise in `%APPDATA%/OOPS/tools/mpv`.
#[must_use]
pub fn vendored_mpv_dir() -> Option<PathBuf> {
    let mut path = crate::target::directory()?;
    path.push("tools");
    path.push("mpv");
    Some(path)
}

/// The executable path for a vendored copy of mpv, if present.
#[must_use]
pub fn vendored_mpv_path() -> Option<PathBuf> {
    let dir = vendored_mpv_dir()?;
    let exe = if cfg!(windows) {
        dir.join("mpv.exe")
    } else {
        dir.join("mpv")
    };
    exe.is_file().then_some(exe)
}

/// Resolves a program name to its executable path.
///
/// If `program` is `mpv`, checks for a vendored copy in the application data directory,
/// beside the binary, or in standard portable locations before falling back to `PATH`.
#[must_use]
pub fn resolve_program(program: &str) -> PathBuf {
    let as_path = PathBuf::from(program);
    if as_path.is_absolute() || as_path.exists() {
        return as_path;
    }
    if program == "mpv" || program == "mpv.exe" {
        if let Some(vendored) = vendored_mpv_path() {
            return vendored;
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            let sibling = dir.join("mpv.exe");
            if sibling.is_file() {
                return sibling;
            }
            let in_tools = dir.join("tools").join("mpv").join("mpv.exe");
            if in_tools.is_file() {
                return in_tools;
            }
        }
    }
    as_path
}

/// Whether a player program is available to run.
#[must_use]
pub fn is_program_available(program: &str) -> bool {
    let resolved = resolve_program(program);
    if resolved.is_file() {
        return true;
    }
    if let Ok(paths) = std::env::var("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(program);
            if candidate.is_file() {
                return true;
            }
            #[cfg(windows)]
            if candidate.with_extension("exe").is_file() {
                return true;
            }
        }
    }
    false
}

/// Whether the currently configured player program is available.
#[must_use]
pub fn configured_player_available() -> bool {
    configured().is_some_and(|cmd| {
        cmd.split_whitespace()
            .next()
            .is_some_and(is_program_available)
    })
}

/// The download address for the Windows portable mpv release.
#[cfg(windows)]
const MPV_WIN_URL: &str = "https://github.com/zhongfly/mpv-winbuild/releases/download/2026-09-25-35af06172b/mpv-x86_64-20260925-git-35af06172b.7z";

/// The SHA-256 digest of that archive.
#[cfg(windows)]
const MPV_WIN_SHA256: &str = "c5008d622edae62416d87f99a282d390456112e04f94758ad4574faad013ad09";

/// Downloads and installs a vendored copy of mpv into the application directory.
///
/// On Windows, fetches the portable archive, verifies its SHA-256 checksum, and extracts
/// it with `tar` into `<data_root>/tools/mpv`.
///
/// # Errors
///
/// When the download, checksum verification, or extraction fails.
pub fn install_vendored_mpv() -> crate::Result<PathBuf> {
    #[cfg(windows)]
    {
        let dest = vendored_mpv_dir()
            .ok_or_else(|| crate::Error::failed("no application data directory found for tools"))?;
        let cache_dir = crate::target::cache_directory()
            .ok_or_else(|| crate::Error::failed("no cache directory found for downloads"))?;
        std::fs::create_dir_all(&cache_dir)?;
        let archive = cache_dir.join("mpv-download.7z");

        let template = crate::fetch::configured();
        let (fetch_prog, fetch_args) = crate::fetch::parts(&template, MPV_WIN_URL, &archive)?;
        let status = Command::new(&fetch_prog)
            .args(&fetch_args)
            .status()
            .map_err(|why| crate::Error::failed(format!("failed to run {fetch_prog}: {why}")))?;
        if !status.success() {
            return Err(crate::Error::failed(format!(
                "download of mpv failed with exit code {status}"
            )));
        }

        let bytes = std::fs::read(&archive)?;
        let digest = crate::checksum::Checksum::of(&bytes);
        if digest.digest() != MPV_WIN_SHA256 {
            let _ = std::fs::remove_file(&archive);
            return Err(crate::Error::failed(format!(
                "mpv download failed digest verification (expected {MPV_WIN_SHA256}, got {})",
                digest.digest()
            )));
        }

        std::fs::create_dir_all(&dest)?;
        let extract = Command::new("tar")
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(&dest)
            .status()
            .map_err(|why| crate::Error::failed(format!("failed to run tar: {why}")))?;
        let _ = std::fs::remove_file(&archive);
        if !extract.success() {
            return Err(crate::Error::failed(format!(
                "extraction of mpv failed with exit code {extract}"
            )));
        }

        let exe = dest.join("mpv.exe");
        if !exe.exists() {
            return Err(crate::Error::failed(
                "extracted archive did not contain mpv.exe",
            ));
        }
        Ok(exe)
    }
    #[cfg(not(windows))]
    {
        Err(crate::Error::failed(
            "automated mpv installation is currently supported on Windows",
        ))
    }
}

/// Sets the status and returns, for the several ways starting can fail.
fn give_up(counts: &Arc<Mutex<Counts>>, why: String) {
    if let Ok(mut held) = counts.lock() {
        held.status = Status::Failed(why);
    }
}

/// The thread body: connect, start the player, move bytes.
fn pump(
    address: &str,
    port: u16,
    template: &str,
    counts: &Arc<Mutex<Counts>>,
    stopping: &Arc<AtomicBool>,
) {
    let target = format!("{address}:{port}");
    let stream = match target.parse() {
        Ok(at) => TcpStream::connect_timeout(&at, PATIENCE),
        Err(_) => TcpStream::connect(&target),
    };
    let mut stream = match stream {
        Ok(stream) => stream,
        Err(why) => return give_up(counts, format!("{target}: {why}")),
    };
    let _ = stream.set_read_timeout(Some(BREATH));

    let (program, arguments) = match parts(template, address) {
        Ok(split) => split,
        Err(why) => return give_up(counts, why.to_string()),
    };
    let program_path = resolve_program(&program);
    let mut player = match Command::new(&program_path)
        .args(&arguments)
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        // Named, because a failed start almost always means the program is not where the
        // configured line says, and that line is the thing to edit.
        Err(why) => return give_up(counts, format!("could not run {program}: {why}")),
    };
    let Some(mut sink) = player.stdin.take() else {
        let _ = player.kill();
        return give_up(counts, format!("{program} would not take a stream"));
    };

    if let Ok(mut held) = counts.lock() {
        held.status = Status::Watching;
        held.player_alive = true;
    }

    let ended = carry(
        &mut stream,
        &mut sink,
        counts,
        stopping,
        WINDOW,
        &mut || matches!(player.try_wait(), Ok(None)),
    );

    // Closing the pipe tells the player the stream is over; killing it first would drop the
    // frames it already holds.
    drop(sink);
    let _ = player.wait();
    if let Ok(mut held) = counts.lock() {
        held.status = Status::Ended(ended);
        held.player_alive = false;
    }
}

/// Moves bytes from one place to another, counting what goes by, until something stops it.
///
/// Returns why it stopped, in words.
///
/// The ends are borrowed trait objects so that every fault [`Counts::diagnose`] separates can
/// be produced and checked without a socket or a real player. `alive` is a closure because
/// whether the player runs changes while this runs.
fn carry(
    from: &mut dyn Read,
    to: &mut dyn Write,
    counts: &Arc<Mutex<Counts>>,
    stopping: &Arc<AtomicBool>,
    over: Duration,
    alive: &mut dyn FnMut() -> bool,
) -> String {
    let mut reader = pros_link::stream::Reader::new();
    let mut buffer = vec![0_u8; MOUTHFUL];
    // The rate window: where it started, and what had arrived by then.
    let mut window = std::time::Instant::now();
    let (mut was_bytes, mut was_units) = (0_u64, 0_u64);
    let mut bytes = 0_u64;

    /// Records the last unit and reports the final counts.
    ///
    /// A unit is known to be whole only when the next start code arrives, so the last one is
    /// always held. At the end it must be flushed, or a stream that sent a single keyframe
    /// would be reported as having sent none.
    macro_rules! settle {
        ($why:expr) => {{
            reader.finish();
            if let Ok(mut held) = counts.lock() {
                held.bytes = bytes;
                held.units = reader.units;
                held.keyframes = reader.keyframes;
                held.pending = reader.pending();
            }
            return $why;
        }};
    }

    loop {
        if stopping.load(Ordering::Relaxed) {
            settle!("stopped".to_owned());
        }
        match from.read(&mut buffer) {
            Ok(0) => settle!("the target closed the connection".to_owned()),
            Ok(some) => {
                let got = &buffer[..some];
                reader.feed(got);
                // Written on before the counters are updated: the frame's latency matters,
                // the counters' does not.
                if let Err(why) = to.write_all(got) {
                    bytes = bytes.saturating_add(some as u64);
                    let explanation = if why.kind() == std::io::ErrorKind::BrokenPipe
                        || why.raw_os_error() == Some(232)
                    {
                        "the player exited (window closed or decoder error)".to_owned()
                    } else {
                        format!("the player stopped reading: {why}")
                    };
                    settle!(explanation);
                }
                bytes = bytes.saturating_add(some as u64);
            }
            // A read timeout: `WouldBlock` on Unix, `TimedOut` on Windows.
            Err(why)
                if matches!(
                    why.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                // A pause between frames, not an end. It falls through to the bookkeeping so a
                // stalled stream's rate drops to zero rather than freezing.
            }
            Err(why) => settle!(why.to_string()),
        }

        let elapsed = window.elapsed();
        let rate = (elapsed >= over).then(|| {
            let seconds = elapsed.as_secs_f64();
            // Through `u32`, which converts to `f64` losslessly where `u64` does not. Saturating
            // would need four gigabytes in one window, and would still read as very fast.
            let over = |delta: u64| f64::from(u32::try_from(delta).unwrap_or(u32::MAX)) / seconds;
            let measured = Rate {
                bytes: over(bytes - was_bytes),
                units: over(reader.units - was_units),
            };
            window = std::time::Instant::now();
            was_bytes = bytes;
            was_units = reader.units;
            measured
        });

        if let Ok(mut held) = counts.lock() {
            held.bytes = bytes;
            held.units = reader.units;
            held.keyframes = reader.keyframes;
            held.pending = reader.pending();
            held.player_alive = alive();
            // Kept between windows, so the figure on screen does not blink out.
            if rate.is_some() {
                held.rate = rate;
            }
        }
    }
}

/// Watches a stream that is already open, writing it somewhere that is not a player.
///
/// For tests and for anything that wants the counting without the picture. It is the same pump
/// the window uses, not a second implementation.
///
/// Blocks until the stream ends. Returns why it stopped.
pub fn carry_into(from: &mut dyn Read, to: &mut dyn Write, watching: &Watching) -> String {
    if let Ok(mut held) = watching.counts.lock() {
        held.status = Status::Watching;
        held.player_alive = true;
    }
    let ended = carry(
        from,
        to,
        &watching.counts,
        &watching.stopping,
        watching.window,
        &mut || true,
    );
    if let Ok(mut held) = watching.counts.lock() {
        held.status = Status::Ended(ended.clone());
        held.player_alive = false;
    }
    ended
}

#[cfg(test)]
mod tests {
    use super::{Counts, Status, Watching};

    /// A watcher that has not started says so, and diagnoses nothing.
    #[test]
    fn an_idle_watch_has_no_complaint() {
        let watching = Watching::idle();
        let counts = watching.counts();
        assert_eq!(counts.status, Status::Idle);
        assert_eq!(counts.diagnose(), None, "nothing has been tried");
    }

    /// Each reason for no picture is told apart from the others.
    #[test]
    fn the_reasons_for_no_picture_are_distinguished() {
        let watching = |bytes, units, keyframes, player_alive| Counts {
            status: Status::Watching,
            bytes,
            units,
            keyframes,
            pending: 0,
            player_alive,
            // No window has closed yet, which is not the same as a rate of zero.
            rate: None,
        };

        let said = watching(0, 0, 0, true).diagnose().expect("nothing arrived");
        assert!(said.contains("nothing has arrived"), "{said}");

        let said = watching(9_000, 0, 0, true).diagnose().expect("no framing");
        assert!(said.contains("none of it framed"), "{said}");

        let said = watching(9_000, 40, 0, true)
            .diagnose()
            .expect("no keyframe");
        assert!(said.contains("no keyframe"), "{said}");
        assert!(said.contains("looks exactly like no stream"), "{said}");

        let said = watching(9_000, 40, 2, false).diagnose().expect("no player");
        assert!(said.contains("player has gone"), "{said}");

        assert_eq!(watching(9_000, 40, 2, true).diagnose(), None);
    }

    /// Ending is not the same as never having started.
    #[test]
    fn a_stream_that_ended_is_not_a_stream_nobody_began() {
        let ended = Status::Ended("the target closed the connection".to_owned());
        assert_ne!(ended, Status::Idle);
        assert!(!ended.is_watching());
        assert!(ended.describe().contains("closed the connection"));

        let failed = Status::Failed("could not run mpv".to_owned());
        assert_ne!(failed, ended, "failing to start is its own thing");
        assert!(failed.describe().contains("could not start"));
    }

    /// Stopping is asked for, and is visible before the thread has noticed.
    #[test]
    fn stopping_is_asked_rather_than_done() {
        let watching = Watching::idle();
        assert!(!watching.stopping());
        watching.stop();
        assert!(watching.stopping());
    }

    /// A stream and a slideshow are told apart even though every count climbs.
    #[test]
    fn a_slideshow_is_not_a_stream() {
        let with = |rate| Counts {
            status: Status::Watching,
            bytes: 900_000,
            units: 40,
            keyframes: 2,
            pending: 0,
            player_alive: true,
            rate: Some(rate),
        };

        // The raw-grab fallback's ceiling: about two frames a second.
        let crawling = super::Rate {
            bytes: 16_600_000.0,
            units: 2.0,
        };
        assert!(!crawling.is_moving());
        let said = with(crawling).diagnose().expect("two a second is a fault");
        assert!(said.contains("slideshow"), "{said}");

        // A fast slideshow in bytes, which is why the rate that matters is units.
        assert!(
            crawling.describe().contains("MB/s"),
            "{}",
            crawling.describe()
        );

        let running = super::Rate {
            bytes: 900_000.0,
            units: 59.9,
        };
        assert!(running.is_moving());
        assert_eq!(with(running).diagnose(), None, "sixty a second is fine");
    }

    /// A stalled stream says so in words rather than as a figure to interpret.
    #[test]
    fn nothing_arriving_is_said_rather_than_shown_as_zero() {
        let stalled = super::Rate {
            bytes: 0.0,
            units: 0.0,
        };
        assert_eq!(stalled.describe(), "nothing arriving");
        assert!(!stalled.is_moving());
    }

    /// The example names a player that reads the stream from standard input.
    #[test]
    fn the_example_command_reads_a_stream_from_its_input() {
        let example = super::example();
        assert!(example.contains("mpv"));
        assert!(
            example.trim_end().ends_with(" -"),
            "it must read standard input: {example}"
        );
        assert!(
            example.contains("low-latency"),
            "a buffering player reads as a broken stream"
        );
        assert!(
            example.contains("--demuxer-lavf-format=h264"),
            "must specify lavf demuxer format for mpv: {example}"
        );
    }

    /// Legacy demuxer option is translated to ffmpeg format for modern mpv builds.
    #[test]
    fn legacy_h264_demuxer_flag_is_translated() {
        let (prog, args) = super::parts("mpv --demuxer=h264 -", "127.0.0.1").unwrap();
        assert_eq!(prog, "mpv");
        assert!(args.contains(&"--demuxer-lavf-format=h264".to_owned()));
        assert!(args.contains(&"--force-window=immediate".to_owned()));
    }

    /// A read timeout is a pause under either name (`WouldBlock`, `TimedOut`), not an end.
    #[test]
    fn a_read_that_timed_out_is_a_pause_under_either_name() {
        use std::io::ErrorKind;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        /// A source that is connected and quiet, and asks to stop after a few reads.
        struct Quiet {
            kind: ErrorKind,
            reads: usize,
            stopping: Arc<AtomicBool>,
        }

        impl std::io::Read for Quiet {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                self.reads += 1;
                if self.reads == 3 {
                    self.stopping.store(true, Ordering::Relaxed);
                }
                Err(std::io::Error::from(self.kind))
            }
        }

        for kind in [ErrorKind::WouldBlock, ErrorKind::TimedOut] {
            let watching = Watching::idle();
            let mut quiet = Quiet {
                kind,
                reads: 0,
                stopping: Arc::clone(&watching.stopping),
            };
            let mut into: Vec<u8> = Vec::new();
            let why = super::carry_into(&mut quiet, &mut into, &watching);
            assert_eq!(
                why, "stopped",
                "{kind:?} must be waited through, not reported"
            );
            assert_eq!(
                quiet.reads, 3,
                "{kind:?} must be gone round rather than settled on"
            );
            assert_eq!(
                watching.counts().status,
                Status::Ended("stopped".to_owned())
            );
        }
    }
}
