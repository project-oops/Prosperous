//! Watching the stand-in stream: read it, count it, and pipe it to a player.
//!
//! # Why this both reads and does not decode
//!
//! `docs/VIDEO.md` part three hands encoded frames to a media player, because decoding here
//! would mean a large C or C++ dependency reached through FFI in a workspace that **forbids**
//! unsafe code - to show a picture `mpv` shows for free.
//!
//! But a player answers exactly one question and it is the wrong one. *No picture* is what it
//! says whether nothing arrived, something arrived that was not video, or video arrived with
//! no keyframe in it - and those are three different faults in three different places.
//!
//! So the bytes come **through** this rather than past it. One socket, read here, counted
//! here, and written to the player's own input. The player shows the picture and this says
//! what went by, and neither has to be trusted about the other's job.
//!
//! # Why a thread
//!
//! A stream is a socket that blocks, and a window that stops repainting is indistinguishable
//! from one that has crashed. So the pump runs on its own thread and the window reads a
//! snapshot of counters, which is the same arrangement every long job in this project uses.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The port the stand-in serves video on.
///
/// From `docs/VIDEO.md` part three. **Ours to choose**, because both ends are ours.
pub const PORT: u16 = 9805;

/// How long to wait for a target to accept.
pub const PATIENCE: Duration = Duration::from_secs(3);

/// How much to read at once.
///
/// A frame is far larger than this, so a read is a piece of one. Sized to be a useful write to
/// a player rather than to match anything about the codec.
const MOUTHFUL: usize = 32 * 1024;

/// How long a rate is measured over.
///
/// **Short enough that a stall shows up while somebody is still looking**, long enough that a
/// figure does not jitter with every read.
const WINDOW: Duration = Duration::from_secs(1);

/// How long a read waits before looping.
///
/// **Not a timeout in the sense of a failure.** A payload between frames produces exactly this
/// and treating it as an end would close a working stream. It is short because the loop is
/// also where the rate window closes, and a stalled stream should say so in about a second
/// rather than in five.
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
    /// **Includes ending cleanly.** A stream that stopped because the payload stopped is not a
    /// failure, and it is not the same as one nobody started - so it says which.
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
    /// **`None` until a window has closed**, which is not the same as zero. A rate of nothing
    /// is a stalled stream; no rate yet is a stream that has not been watched long enough to
    /// have one, and showing the second as the first would accuse a healthy stream.
    pub rate: Option<Rate>,
}

/// How fast it is arriving, over one window.
///
/// # Why cumulative counts are not enough
///
/// Bytes and units only ever climb, so a stream delivering sixty frames a second and one
/// delivering a frame every four seconds look the same in a panel - both are *going up*. The
/// difference between those two is the difference between a stream and a slideshow, and it is
/// invisible without a rate.
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
        // A stalled stream says so in words rather than as `0.0/s`, which reads as a figure
        // somebody has to interpret rather than a fault.
        if self.units < 0.05 && self.bytes < 1.0 {
            return "nothing arriving".to_owned();
        }
        format!("{:.1}/s  {}/s", self.units, size(self.bytes))
    }

    /// Whether this is a stream rather than a slideshow.
    ///
    /// **Ten a second**, which is well under any real frame rate and well over what raw grabs
    /// could ever manage - so it separates the two designs in `docs/VIDEO.md` rather than
    /// grading the picture.
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
    /// # The whole reason this counts anything
    ///
    /// A player says *no picture* for at least four different reasons and cannot tell them
    /// apart. This can, because it saw the bytes:
    ///
    /// - nothing arrived at all - the payload is not sending
    /// - bytes arrived and none of them framed - it is not this kind of stream
    /// - units arrived with no keyframe - a decoder has nothing to start from, and this looks
    ///   *exactly* like no stream at all
    /// - everything arrived and the player is gone - the fault is on this side
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
            // **The counts still climb here**, which is why this needs saying: everything
            // above is satisfied and the thing on screen is a slideshow. This is exactly what
            // the raw-grab fallback in `docs/VIDEO.md` part two looks like if it were ever
            // mistaken for the stand-in - about two frames a second.
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
    /// # Why this exists
    ///
    /// **A test whose verdict depends on how busy the machine is, is a test that lies.** The
    /// claim *"a run shorter than a window has no rate"* was checked by serving frames with no
    /// delay and trusting that to finish inside a second - which it does, until eleven other
    /// socket tests run beside it, and then a window closes, a rate appears, and a correct
    /// pump is reported as broken. Naming the window makes the premise true by construction.
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
    /// **Asks rather than kills**, so the player is closed the way it expects and the socket
    /// is shut rather than abandoned.
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
/// # Why this lives here now
///
/// It was in the module that launched a remote-play client, and Porthole borrowed it for the
/// player. That module is gone - this project serves its own stream and does not drive
/// somebody else's client - so the one piece of it Porthole actually used comes with it.
///
/// Split on spaces, which is the whole of it. **Not a shell**: no quoting, no expansion, no
/// pipelines. A command that needs those needs a script, and a script is one word.
///
/// # Errors
///
/// When the line is empty once comments and spaces are gone - there is nothing to run, and
/// launching nothing quietly would look exactly like launching something that failed.
pub fn parts(template: &str, address: &str) -> Result<(String, Vec<String>), String> {
    let filled = template.replace("{address}", address);
    let mut words = filled.split_whitespace().map(str::to_owned);
    let program = words.next().ok_or_else(|| {
        format!(
            "nothing to run - put a command in {}",
            command_path().map_or_else(
                || "the configuration".to_owned(),
                |path| path.display().to_string()
            )
        )
    })?;
    Ok((program, words.collect()))
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
    // `-` is the convention for reading from standard input, and the low-latency options are
    // what stop a player buffering several seconds of a live stream before showing anything.
    "# The player the stand-in stream is piped into. One line, split on spaces.\n\
     #\n\
     # It reads the stream on its standard input, so the last word is usually a dash. The\n\
     # low-latency options matter: a player left to itself buffers seconds of a live stream\n\
     # before showing anything, which reads as a stream that is not working.\n\
     #\n\
     # This project does not decode video. It pipes it to something that does, counts what\n\
     # went past, and can therefore say which of several reasons there is no picture.\n\
     mpv --demuxer=h264 --profile=low-latency --untimed --no-cache -\n"
        .to_owned()
}

/// Writes the example, without overwriting one somebody has edited.
///
/// # Errors
///
/// When the file cannot be written.
pub fn write_example() -> Result<PathBuf, String> {
    let path = command_path().ok_or("no home directory, so there is nowhere to keep it")?;
    if path.exists() {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|why| why.to_string())?;
    }
    std::fs::write(&path, example()).map_err(|why| why.to_string())?;
    Ok(path)
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
        Err(why) => return give_up(counts, why),
    };
    let mut player = match Command::new(&program)
        .args(&arguments)
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        // Naming the program matters: *could not start* almost always means it is not
        // installed where the line says, and the line is the thing to edit.
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

    // Closing the pipe is what tells a player the stream is over; killing it first would take
    // the picture away before it had finished with what it already had.
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
/// # Why the ends are borrowed rather than owned
///
/// Because the interesting half of this is what it *counts*, and every one of the four faults
/// [`Counts::diagnose`] distinguishes can be produced by a source that is not a socket and
/// observed through a sink that is not a player.
///
/// A version of this that could only be exercised by opening a real connection and starting a
/// real media player would be a version nobody exercised. **The claims that stream a player
/// sees is byte-identical to the stream that arrived, and that a start code split across two
/// reads is still one unit, are exactly the kind that go untested and turn out to be false.**
///
/// `alive` is asked rather than passed, because whether a player is still running is a
/// question with an answer that changes while this runs.
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
    /// # Why the ending needs its own step
    ///
    /// A unit is only known to be whole when the **next** start code arrives, so at any moment
    /// the last one read is still being held. That is right while a stream is running - it is
    /// one frame of lag in a counter nobody is timing against.
    ///
    /// At the end it is a lie, and a specific one. **A payload that sent a single keyframe and
    /// stopped would be reported as having sent no keyframe**, which `diagnose` reads as *a
    /// decoder has nothing to start from* - accusing a stream that was correct.
    ///
    /// Found by the fake payload on the first run against it, which is what it was built for.
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
                // **Written on before the counters are updated.** A frame delayed by
                // bookkeeping is latency, and the counters are for a person reading a panel
                // rather than for anything that has to be exact.
                if let Err(why) = to.write_all(got) {
                    bytes = bytes.saturating_add(some as u64);
                    settle!(format!("the player stopped reading: {why}"));
                }
                bytes = bytes.saturating_add(some as u64);
            }
            Err(why) if why.kind() == std::io::ErrorKind::WouldBlock => {
                // Nothing this window. Not an end - a payload between frames looks exactly
                // like this, and treating it as a stop would close a working stream. It still
                // falls through to the bookkeeping below, so a stalled stream's rate goes to
                // zero rather than freezing at whatever it last managed.
            }
            Err(why) => settle!(why.to_string()),
        }

        let elapsed = window.elapsed();
        let rate = (elapsed >= over).then(|| {
            let seconds = elapsed.as_secs_f64();
            // Through `u32` because that conversion cannot lose anything, where `u64` to
            // `f64` can. A window is about a second, so saturating would need four gigabytes
            // in one - and a figure that saturated would read as *very fast*, which at that
            // point it would be.
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
            // Kept when the window has not closed, so the figure on screen does not blink out
            // between measurements.
            if rate.is_some() {
                held.rate = rate;
            }
        }
    }
}

/// Watches a stream that is already open, writing it somewhere that is not a player.
///
/// **For tests and for anything that wants the counting without the picture.** It is the same
/// pump the window uses, so what a test exercises is the code that runs in the window
/// rather than a second implementation that agrees with it today.
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

    /// **Each reason for no picture is told apart from the others.**
    ///
    /// This is the whole argument for reading a stream nobody decodes: a player says *no
    /// picture* for all four of these and cannot say which.
    #[test]
    fn the_reasons_for_no_picture_are_distinguished() {
        let watching = |bytes, units, keyframes, player_alive| Counts {
            status: Status::Watching,
            bytes,
            units,
            keyframes,
            pending: 0,
            player_alive,
            // No window has closed yet, which is deliberately not the same as a rate of zero.
            rate: None,
        };

        let said = watching(0, 0, 0, true).diagnose().expect("nothing arrived");
        assert!(said.contains("nothing has arrived"), "{said}");

        let said = watching(9_000, 0, 0, true).diagnose().expect("no framing");
        assert!(said.contains("none of it framed"), "{said}");

        // The one that hides: a stream carrying data that decodes to nothing.
        let said = watching(9_000, 40, 0, true)
            .diagnose()
            .expect("no keyframe");
        assert!(said.contains("no keyframe"), "{said}");
        assert!(said.contains("looks exactly like no stream"), "{said}");

        let said = watching(9_000, 40, 2, false).diagnose().expect("no player");
        assert!(said.contains("player has gone"), "{said}");

        // Everything working says nothing at all.
        assert_eq!(watching(9_000, 40, 2, true).diagnose(), None);
    }

    /// **Ending is not the same as never having started.**
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

    /// **A stream and a slideshow are told apart**, even though every count is climbing.
    ///
    /// This is the fault that survives all four of the checks above: bytes arrive, they frame,
    /// there are keyframes, the player is alive - and what is on screen is two frames a
    /// second. Cumulative counters cannot see it, because both cases only ever go up.
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

        // The raw-grab fallback's measured ceiling: about two frames a second.
        let crawling = super::Rate {
            bytes: 16_600_000.0,
            units: 2.0,
        };
        assert!(!crawling.is_moving());
        let said = with(crawling).diagnose().expect("two a second is a fault");
        assert!(said.contains("slideshow"), "{said}");

        // Note this is a *fast* slideshow. Sixteen megabytes a second and still wrong, which
        // is why the rate that matters is units rather than bytes.
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

    /// The example names a player and reads from standard input, which is the whole contract
    /// between this and whatever shows the picture.
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
    }
}
