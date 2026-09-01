//! The watching half of the stand-in, against a payload that is not a payload.
//!
//! # What this is for
//!
//! `pros-core::watch` reads a stream it does not decode, so that it can say **which** of
//! several faults produced no picture. Every one of those claims was, until this file, an
//! assertion about code that had never read a socket.
//!
//! So each test here produces one real fault, over a real connection, and checks that the
//! client says the right thing about it - including the two that are indistinguishable from
//! outside, which is the entire reason any of this exists.
//!
//! Nothing here starts a media player. The pump takes its sink as an argument precisely so
//! this can watch a stream into a buffer, which also makes the strongest claim checkable:
//! **what the player is handed is byte-for-byte what arrived.**

use std::io::Read as _;
use std::net::TcpStream;
use std::time::Duration;

use pros_link::standin::{Serves, Standin, video};

/// How long a test waits for a loopback stream that should take no time at all.
const PATIENCE: Duration = Duration::from_secs(5);

/// Connects to a fake and watches it into a buffer, returning what arrived and what was
/// counted.
fn watch(serves: Serves) -> (Vec<u8>, pros_core::watch::Counts, String) {
    watch_with(serves, &pros_core::watch::Watching::idle())
}

/// The same, with the watcher supplied - for a test that has something to say about the rate
/// window and must not be at the mercy of how busy the machine is while it runs.
fn watch_with(
    serves: Serves,
    watching: &pros_core::watch::Watching,
) -> (Vec<u8>, pros_core::watch::Counts, String) {
    let fake = Standin::start(serves).expect("the loopback interface must exist");
    let mut from = TcpStream::connect((fake.address(), fake.video_port()))
        .expect("a fake that is listening must accept");
    from.set_read_timeout(Some(PATIENCE))
        .expect("a socket must take a timeout");

    let mut into: Vec<u8> = Vec::new();
    let why = pros_core::watch::carry_into(&mut from, &mut into, watching);
    (into, watching.counts(), why)
}

/// Puts an ended run back into the state it was in while it ran.
///
/// # Why this is needed, and why it is not cheating
///
/// [`pros_core::watch::Counts::diagnose`] only speaks while a stream is **running**, which is
/// right: there is nothing to diagnose about a stream nobody started. But these tests read
/// their counts after the fake has closed, so the snapshot says ended.
///
/// Setting the status back is not enough on its own. A finished run also records that the
/// player has gone, and *watching with a dead player* is a state that never occurs - so a
/// snapshot with both would be diagnosed on the player rather than on the stream, and the test
/// would be asking about something it did not mean to ask about.
///
/// **Caught by the first run of these tests**, which is the mistake this file exists to make
/// cheap.
fn as_if_still_running(counts: &mut pros_core::watch::Counts) {
    counts.status = pros_core::watch::Status::Watching;
    counts.player_alive = true;
}

/// **A working stream arrives whole, and the player gets exactly what the target sent.**
///
/// The claim worth testing first: this reads every byte on the way past, and a reader that
/// consumed, buffered or reordered anything would show up here and nowhere else.
#[test]
fn a_working_stream_arrives_byte_for_byte() {
    let serves = Serves::Video {
        units: 24,
        apart: Duration::ZERO,
    };
    let (arrived, counts, why) = watch(serves.clone());

    assert_eq!(
        arrived,
        video(&serves),
        "the player must be handed what the target sent, unchanged"
    );
    assert_eq!(
        u64::try_from(arrived.len()).unwrap_or(u64::MAX),
        counts.bytes
    );
    assert_eq!(counts.units, 24, "every unit sent was counted");
    assert_eq!(counts.keyframes, 3, "one in eight, so three in twenty-four");
    assert_eq!(
        counts.diagnose(),
        None,
        "a working stream has nothing to complain about"
    );
    assert!(
        why.contains("closed"),
        "it ended because the target ended it"
    );
}

/// **A stream of exactly one keyframe is not reported as having none.**
///
/// This is the defect the fake found on its first run, and it is worth its own test because it
/// is not a miscount - it is the diagnosis **lying**.
///
/// A unit is only known to be whole when the next start code arrives, so the last one read is
/// always still held. Live, that is one frame of lag nobody times against. At the end of a
/// stream it meant a payload that sent one keyframe and stopped was reported as having sent
/// none - and `diagnose` reads no keyframe as *a decoder has nothing to start from*, which
/// would have accused a stream that was completely correct.
#[test]
fn a_single_keyframe_is_counted_rather_than_left_held() {
    let (arrived, counts, _) = watch(Serves::Video {
        units: 1,
        apart: Duration::ZERO,
    });

    assert!(!arrived.is_empty(), "one unit did arrive");
    assert_eq!(counts.units, 1, "and the last unit must not be left held");
    assert_eq!(counts.keyframes, 1, "it was a keyframe, and it must say so");
    assert_eq!(
        counts.pending, 0,
        "nothing may still be waiting for a boundary once the stream has ended"
    );

    // The claim that matters: this stream must not be accused.
    let mut watching = counts.clone();
    as_if_still_running(&mut watching);
    assert_eq!(
        watching.diagnose(),
        None,
        "a stream carrying one perfectly good keyframe must not be reported as having none"
    );
}

/// **The fault that hides.** Every count climbs, the framing is valid, a player attaches
/// happily - and there is nothing for a decoder to start from, so the screen stays black.
///
/// Indistinguishable from a dead socket without counting, which is why this project counts.
#[test]
fn a_stream_with_no_keyframe_is_told_apart_from_a_dead_one() {
    let (arrived, counts, _) = watch(Serves::Dependent { units: 40 });

    assert!(!arrived.is_empty(), "it genuinely arrived");
    assert_eq!(counts.units, 40, "and it genuinely framed");
    assert_eq!(counts.keyframes, 0, "with nothing to begin at");

    // The counts are healthy on every measure a player could see. Only this says otherwise.
    let mut watching = counts.clone();
    as_if_still_running(&mut watching);
    let said = watching.diagnose().expect("this must not pass silently");
    assert!(said.contains("no keyframe"), "{said}");
    assert!(
        said.contains("looks exactly like no stream at all"),
        "the point is that it is deceptive, and the message should say so: {said}"
    );
}

/// Bytes that never frame are a socket serving something else, and say so.
#[test]
fn bytes_that_never_frame_are_named_as_such() {
    let (arrived, counts, _) = watch(Serves::Noise { bytes: 8192 });

    assert_eq!(arrived.len(), 8192, "it all arrived");
    assert_eq!(counts.units, 0, "and none of it was a unit");

    let mut watching = counts.clone();
    as_if_still_running(&mut watching);
    let said = watching.diagnose().expect("this must not pass silently");
    assert!(said.contains("none of it framed"), "{said}");
}

/// **A start code split across two reads is still one unit.**
///
/// A real network cuts wherever it likes. A reader that assumed a read contains whole units
/// would pass every other test in this file, because every other test hands it tidy pieces.
#[test]
fn a_start_code_split_across_reads_is_still_one_unit() {
    let (arrived, counts, _) = watch(Serves::Awkward);

    assert_eq!(
        arrived,
        video(&Serves::Awkward),
        "cut badly and reassembled, it is still the same stream"
    );
    // Eight units went out in pieces as small as one byte, with start codes straddling the
    // cuts. All eight must be counted, not seven and not fifteen.
    assert_eq!(counts.units, 8, "the cuts must not create or destroy units");
    assert_eq!(counts.keyframes, 1);
    assert_eq!(
        counts.diagnose(),
        None,
        "a badly cut stream is still a stream"
    );
}

/// Connected with nothing arriving is its own state, and not an end.
#[test]
fn connected_and_silent_is_not_the_same_as_ended() {
    let fake = Standin::start(Serves::Silence).expect("the loopback interface must exist");
    let mut from = TcpStream::connect((fake.address(), fake.video_port()))
        .expect("a fake that is listening must accept");
    // Short, because the point is that nothing comes - waiting the full patience would only
    // make the test slow.
    from.set_read_timeout(Some(Duration::from_millis(200)))
        .expect("a socket must take a timeout");

    let mut buffer = [0_u8; 64];
    let read = from.read(&mut buffer);
    assert!(
        matches!(&read, Err(why) if why.kind() == std::io::ErrorKind::WouldBlock
            || why.kind() == std::io::ErrorKind::TimedOut),
        "a payload that is running and not producing must not look like a closed socket: \
         {read:?}"
    );

    let mut counts = pros_core::watch::Counts {
        status: pros_core::watch::Status::Watching,
        ..pros_core::watch::Counts::default()
    };
    counts.bytes = 0;
    let said = counts.diagnose().expect("silence must be reported");
    assert!(said.contains("nothing has arrived"), "{said}");
}

/// A target that goes away ends the stream, and the reason names what happened.
///
/// **Ended is not idle.** A stream that stopped because the payload stopped and a stream
/// nobody ever started are different situations, and the second is what a bare `Idle` would
/// have implied.
#[test]
fn a_target_that_closes_ends_the_stream_and_says_why() {
    let (_, counts, why) = watch(Serves::Video {
        units: 4,
        apart: Duration::ZERO,
    });

    assert!(why.contains("closed the connection"), "{why}");
    match counts.status {
        pros_core::watch::Status::Ended(said) => {
            assert!(said.contains("closed"), "{said}");
        }
        other => panic!("a finished stream must report as ended, not {other:?}"),
    }
    assert_ne!(
        counts.units, 0,
        "and it must keep what it counted before it ended"
    );
}

/// Asking it to stop stops it, and that reason is distinct from a target going away.
#[test]
fn stopping_is_reported_as_stopping_rather_than_as_a_fault() {
    let fake = Standin::start(Serves::Silence).expect("the loopback interface must exist");
    let mut from = TcpStream::connect((fake.address(), fake.video_port()))
        .expect("a fake that is listening must accept");
    from.set_read_timeout(Some(Duration::from_millis(50)))
        .expect("a socket must take a timeout");

    let watching = pros_core::watch::Watching::idle();
    // Asked before it begins, so the first pass through the loop sees it. The intent is the
    // reason it gives, not the timing.
    watching.stop();
    let mut into: Vec<u8> = Vec::new();
    let why = pros_core::watch::carry_into(&mut from, &mut into, &watching);

    assert_eq!(why, "stopped", "a deliberate stop is not a failure");
    assert!(
        !watching.counts().status.is_watching(),
        "and it must not still claim to be watching"
    );
}

/// A refused connection names the port, which is what makes the state of the payload legible.
///
/// **This is what pressing *watch* does today**, with no payload written - so the message it
/// produces is the one somebody will actually read most often.
#[test]
fn a_port_nothing_serves_refuses_and_names_itself() {
    // Bound and dropped, so the port is real and certainly free.
    let port = {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback must exist");
        taken
            .local_addr()
            .expect("a bound socket has an address")
            .port()
    };

    let watching = pros_core::watch::Watching::start("127.0.0.1", port, "no-such-player -");
    let until = std::time::Instant::now() + PATIENCE;
    while std::time::Instant::now() < until {
        if !matches!(watching.counts().status, pros_core::watch::Status::Idle) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    match watching.counts().status {
        pros_core::watch::Status::Failed(why) => {
            assert!(
                why.contains(&port.to_string()),
                "the message has to name the port, or it says nothing useful: {why}"
            );
        }
        other => panic!("nothing is serving that port, so this must fail: {other:?}"),
    }
}

/// **A stream fast enough to be a stream is measured as one.**
///
/// # Why this test is allowed to be slow
///
/// A rate is measured over a real window - one second - and a stream that finishes in
/// milliseconds never closes one. So this runs for as long as it takes to produce a rate, and
/// making the window injectable to speed it up would mean testing a window that never ships.
///
/// # The margins, and why they are what they are
///
/// `thread::sleep` never sleeps for *less* than it is asked, so eighty units twenty
/// milliseconds apart cannot take under 1.6 seconds - the window is certain to close, and
/// `rate` is certain not to be `None`. It can take longer, because the operating system's
/// timer is coarse, so the assertion is on the **threshold** rather than on a figure: between
/// roughly thirty and fifty a second, against a bar of ten.
#[test]
fn a_stream_fast_enough_to_be_a_stream_is_measured_as_one() {
    let (_, counts, _) = watch(Serves::Video {
        units: 80,
        apart: Duration::from_millis(20),
    });

    let rate = counts
        .rate
        .expect("1.6 seconds is longer than the window, so a rate must have been measured");
    assert!(
        rate.is_moving(),
        "eighty units over about two seconds is a stream, not a slideshow: {}",
        rate.describe()
    );
    assert!(rate.bytes > 0.0, "and bytes were moving too");

    let mut watching = counts.clone();
    as_if_still_running(&mut watching);
    assert_eq!(
        watching.diagnose(),
        None,
        "a stream arriving at a real rate has nothing wrong with it"
    );
}

/// **A stream too slow to be a stream is named as a slideshow**, with every other count healthy.
///
/// This is the fifth fault, and the only one that passes all four of the others: bytes arrive,
/// they frame, there are keyframes, the player is alive - and what is on screen is a few
/// pictures a second.
///
/// It is what `docs/VIDEO.md` part two's raw-grab fallback would look like if it were ever
/// mistaken for the stand-in, and cumulative counters cannot see it at all, because both a
/// stream and a slideshow only ever go up.
#[test]
fn a_stream_too_slow_to_be_a_stream_is_named_as_a_slideshow() {
    let (_, counts, _) = watch(Serves::Video {
        units: 10,
        apart: Duration::from_millis(150),
    });

    // Every count a player could see is healthy.
    assert!(counts.units > 0, "it framed");
    assert!(counts.keyframes > 0, "and a decoder had somewhere to start");

    let rate = counts
        .rate
        .expect("1.5 seconds is longer than the window, so a rate must have been measured");
    assert!(
        !rate.is_moving(),
        "under seven a second is a slideshow: {}",
        rate.describe()
    );

    let mut watching = counts.clone();
    as_if_still_running(&mut watching);
    let said = watching
        .diagnose()
        .expect("a slideshow must not pass as a working stream");
    assert!(said.contains("slideshow"), "{said}");
}

/// **No rate yet is not a rate of nothing**, over a real socket.
///
/// A stream that ended before a window closed has not been watched long enough to have a rate.
/// Reporting that as zero would accuse a healthy stream of having stalled - and the panel draws
/// the two differently on purpose.
#[test]
fn a_stream_shorter_than_the_window_has_no_rate_rather_than_a_rate_of_zero() {
    // An hour, so *shorter than the window* is true by construction rather than by hoping the
    // run finishes inside a second. With the default second, eleven other socket tests running
    // beside this one were enough to close a window and fail a correct pump.
    let (_, counts, _) = watch_with(
        Serves::Video {
            units: 24,
            apart: Duration::ZERO,
        },
        &pros_core::watch::Watching::idle_measuring_over(Duration::from_hours(1)),
    );

    assert!(counts.units > 0, "it did arrive, and quickly");
    assert_eq!(
        counts.rate, None,
        "nobody measured a second, so there is no rate to report"
    );

    let mut watching = counts.clone();
    as_if_still_running(&mut watching);
    assert_eq!(
        watching.diagnose(),
        None,
        "and it must not be accused of stalling for want of a measurement"
    );
}
